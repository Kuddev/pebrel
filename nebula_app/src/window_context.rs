//! Terminal window context.

use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::mem;
#[cfg(not(windows))]
use std::os::unix::io::{AsRawFd, RawFd};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use glutin::config::Config as GlutinConfig;
use glutin::display::GetGlDisplay;
#[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
use glutin::platform::x11::X11GlConfigExt;
use log::{error, info, warn};
use serde_json as json;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event as WinitEvent, Modifiers, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::raw_window_handle::HasDisplayHandle;
use winit::window::WindowId;

use nebula_terminal::event::{Event as TerminalEvent, Notify};
use nebula_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, Notifier};
use nebula_terminal::grid::{Dimensions, Scroll};
use nebula_terminal::index::{Column, Direction, Line, Point};
use nebula_terminal::sync::FairMutex;
use nebula_terminal::term::test::TermSize;
use nebula_terminal::term::{Term, TermMode};
use nebula_terminal::tty;

use crate::cli::{ParsedOptions, WindowOptions};
use crate::clipboard::Clipboard;
use crate::config::UiConfig;
use crate::config::ui_config::Profile;
use crate::display::window::Window;
use crate::display::{Display, NebulaPaneState};
use crate::event::{
    ActionContext, Event, EventProxy, EventType, Mouse, SearchState, TabRequest, TouchPurpose,
};
#[cfg(unix)]
use crate::logging::LOG_TARGET_IPC_CONFIG;
use crate::message_bar::MessageBuffer;
use crate::scheduler::{Scheduler, TimerId, Topic};
use crate::{input, renderer, session};

mod agents;
mod model;
mod nebula_fetch_art;
mod runtime;
mod session_persistence;
mod ssh_panes;
mod tab_duplication;
/// New-tab welcome page (Windows logo + fastfetch intro). Stateless helpers.
pub(crate) mod welcome;
use welcome::nebula_fastfetch_intro_command_for;

use model::{DOC_PANE_ID, Layout, PaneId, TabEntry, TabLaunch};
pub use model::{DetachedWindow, Pane, WindowBoot};

/// Split-pane behaviour (toggle/resize/drag/focus); `impl WindowContext`.
mod split;
mod tabs;
mod chrome;
mod events;

/// Mouse buttons whose press is an interaction with terminal content and must
/// therefore update split focus before the event is routed to a pane.
fn pane_focus_button(button: &MouseButton) -> bool {
    matches!(button, MouseButton::Left | MouseButton::Middle | MouseButton::Right)
}

/// Resolve window input while a modal is open. Multi-line paste is the only
/// confirmation that carries terminal data, so it stays bound to its source
/// pane; if that pane was reaped, the caller routes to normal focus and the
/// write-boundary pane-id guard drops the stale transaction.
fn routed_input_pane(
    confirm: Option<&crate::display::NebulaConfirm>,
    focused: PaneId,
    pane_exists: impl Fn(PaneId) -> bool,
) -> PaneId {
    confirm
        .and_then(crate::display::NebulaConfirm::paste_pane_id)
        .filter(|pane_id| pane_exists(*pane_id))
        .unwrap_or(focused)
}

fn select_initial_shell(
    configured: Option<tty::Shell>,
    user_default: Option<tty::Shell>,
    cli: Option<tty::Shell>,
) -> Option<tty::Shell> {
    cli.or(user_default).or(configured)
}

/// Validate a tree-provided cwd immediately before process creation. The tree
/// can disappear between drawing the action and handling its click; rejecting
/// that race avoids a platform-specific PTY/CreateProcess startup failure.
fn valid_new_tab_directory(path: &std::path::Path) -> bool {
    path.is_dir()
}

/// 一次标签插入的落点来源。由调用方声明意图，而不是让插入点自己猜：
/// 真正创建标签走 [`TabPlacement::Created`]（读新标签插入策略），会话恢复与
/// 工作区导入走 [`TabPlacement::AfterActive`]（保持各自记录的顺序）。
///
/// 这个区别必须由类型承载。恢复路径复用 `spawn_tab_*` 创建函数，光靠注释
/// 约定「恢复时别读策略」，下一个新增入口就会漏掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabPlacement {
    Created,
    AfterActive,
}

/// 新标签在标签顺序中的落点。所有插入点共用它，因此
/// `(active_tab + 1).min(len)` 这条计算只存在一处。
fn tab_insert_index(
    placement: TabPlacement,
    position: crate::display::NewTabPosition,
    active_tab: usize,
    tab_count: usize,
) -> usize {
    let after_active = active_tab.saturating_add(1).min(tab_count);
    match placement {
        TabPlacement::AfterActive => after_active,
        TabPlacement::Created => match position {
            crate::display::NewTabPosition::AfterCurrent => after_active,
            crate::display::NewTabPosition::End => tab_count,
        },
    }
}

/// Resolve a fresh tab's directory without allowing the global setting to
/// overwrite an explicit profile/command directory.
fn preferred_tab_cwd(
    explicit: Option<std::path::PathBuf>,
    startup: Option<std::path::PathBuf>,
    focused: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    explicit.or(startup).or(focused)
}

/// Initial-window precedence is deliberately different from a normal new
/// tab: an explicit CLI path and a restored session must survive a global
/// startup-directory change, while the setting still outranks static config.
fn preferred_initial_cwd(
    cli: Option<std::path::PathBuf>,
    restored: Option<std::path::PathBuf>,
    startup: Option<std::path::PathBuf>,
    configured: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    cli.or(restored).or(startup).or(configured)
}

/// Keep idle redraw costs low while giving editor and spinner chrome a 12.5 Hz
/// clock. At an 800 ms revolution that gives the spinner exactly ten positions,
/// while avoiding display-rate redraw of a window with a system backdrop.
#[inline]
fn chrome_clock_interval(
    window_focused: bool,
    spinner_running: bool,
    editor_active: bool,
    chrome_animating: bool,
) -> Duration {
    if !window_focused {
        Duration::from_secs(1)
    } else if spinner_running || editor_active || chrome_animating {
        Duration::from_millis(80)
    } else {
        Duration::from_secs(1)
    }
}

/// Event context for one individual Nebula window.
pub struct WindowContext {
    pub message_buffer: MessageBuffer,
    pub display: Display,
    pub dirty: bool,
    event_queue: Vec<WinitEvent<Event>>,
    /// Pool of all live panes in this window, indexed by lookup on `Pane::id`.
    panes: Vec<Pane>,
    /// Tab bar entries; `active_tab` indexes the visible one. Each tab owns a
    /// pane layout tree whose leaves reference panes in `panes`.
    tabs: Vec<TabEntry>,
    active_tab: usize,
    next_pane_id: PaneId,
    /// When set, this pane of the active tab is zoomed to fill the window
    /// (other panes hidden). Cleared by any layout/focus change.
    zoom: Option<PaneId>,
    /// Live divider-drag state: which split node (by tree path) is being
    /// resized, its orientation and content rect. `None` when not dragging.
    split_drag: Option<split::SplitDragState>,
    proxy: EventLoopProxy<Event>,
    cursor_blink_timed_out: bool,
    /// 上一帧的聚焦 pane。变化时给新聚焦终端补发 CursorBlinkingChange，
    /// blink 定时器按它的样式重新起表——否则从"不闪"的 pane 切到"该闪"
    /// 的 pane 后表根本没开，光标永远常亮。
    blink_focus_pane: Option<PaneId>,
    prev_bell_cmd: Option<Instant>,
    /// When the PTYs last learned their size. Drives the leading-edge check of
    /// the resize debounce: a lone resize (startup, maximize, sidebar toggle)
    /// passes through instantly; only a rapid follow-up — i.e. an interactive
    /// drag — defers to the settle timer.
    last_pty_resize: Option<Instant>,
    /// Current chrome clock cadence (1 Hz idle, 8 fps for finite chrome
    /// transitions, 60 fps while a task spinner runs).
    clock_interval: Duration,
    /// Last session snapshot written to disk, so the 1 Hz autosave can skip
    /// the write when nothing changed. `None` forces the next tick to write.
    last_saved_session: Option<session::Session>,
    /// Last normal inner size. Maximized/fullscreen resize events must not
    /// overwrite the dimensions Windows restores when leaving that state.
    windowed_size: LogicalSize<u32>,
    /// Excluded from session persistence (the quick/Quake terminal is scratch
    /// space; its tabs must never overwrite the main window's session).
    pub session_exempt: bool,
    modifiers: Modifiers,
    mouse: Mouse,
    touch: TouchPurpose,
    occluded: bool,
    preserve_title: bool,
    window_config: ParsedOptions,
    config: Rc<UiConfig>,
    /// Stand-in pane for document-viewer tabs (see [`Self::create_doc_pane`]).
    /// Lives outside `panes` so id lookups keep treating doc tabs as
    /// pane-less; only the event pipeline borrows it.
    doc_pane: Pane,
}

impl WindowContext {
    /// Create initial window context that does bootstrapping the graphics API we're going to use.
    pub fn initial(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<Event>,
        config: Rc<UiConfig>,
        mut options: WindowOptions,
        boot: WindowBoot,
    ) -> Result<Self, Box<dyn Error>> {
        let raw_display_handle = event_loop.display_handle().unwrap().as_raw();

        let mut identity = config.window.identity.clone();
        options.window_identity.override_identity_config(&mut identity);

        // Windows has different order of GL platform initialization compared to any other platform;
        // it requires the window first.
        #[cfg(windows)]
        let window = Window::new(event_loop, &config, &identity, &mut options)?;
        #[cfg(windows)]
        crate::boot_trace("os window created");
        #[cfg(windows)]
        let raw_window_handle = Some(window.raw_window_handle());

        #[cfg(not(windows))]
        let raw_window_handle = None;

        let gl_display = renderer::platform::create_gl_display(
            raw_display_handle,
            raw_window_handle,
            config.debug.prefer_egl,
        )?;
        crate::boot_trace("gl display created (WGL ext probe)");
        let gl_config = renderer::platform::pick_gl_config(&gl_display, raw_window_handle)?;
        crate::boot_trace("gl display+config picked");

        #[cfg(not(windows))]
        let window = Window::new(
            event_loop,
            &config,
            &identity,
            &mut options,
            #[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
            gl_config.x11_visual(),
        )?;

        // Create context.
        let gl_context =
            renderer::platform::create_gl_context(&gl_display, &gl_config, raw_window_handle)?;
        crate::boot_trace("gl context created");

        let display = Display::new(window, gl_context, &config, event_loop.system_theme(), false)?;
        crate::boot_trace("display ready (fonts rasterized)");

        Self::new(display, config, options, proxy, boot)
    }

    /// Create additional context with the graphics platform other windows are using.
    pub fn additional(
        gl_config: &GlutinConfig,
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<Event>,
        config: Rc<UiConfig>,
        mut options: WindowOptions,
        config_overrides: ParsedOptions,
        boot: WindowBoot,
    ) -> Result<Self, Box<dyn Error>> {
        let gl_display = gl_config.display();

        let mut identity = config.window.identity.clone();
        options.window_identity.override_identity_config(&mut identity);

        // Check if new window will be opened as a tab.
        // This must be done before `Window::new()`, which unsets `window_tabbing_id`.
        #[cfg(target_os = "macos")]
        let tabbed = options.window_tabbing_id.is_some();
        #[cfg(not(target_os = "macos"))]
        let tabbed = false;

        let window = Window::new(
            event_loop,
            &config,
            &identity,
            &mut options,
            #[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
            gl_config.x11_visual(),
        )?;

        // Create context.
        let raw_window_handle = window.raw_window_handle();
        let gl_context =
            renderer::platform::create_gl_context(&gl_display, gl_config, Some(raw_window_handle))?;

        let display = Display::new(window, gl_context, &config, event_loop.system_theme(), tabbed)?;

        let mut window_context = Self::new(display, config, options, proxy, boot)?;

        // Set the config overrides at startup.
        //
        // These are already applied to `config`, so no update is necessary.
        window_context.window_config = config_overrides;

        Ok(window_context)
    }

    /// Create a new terminal window context.
    fn new(
        display: Display,
        config: Rc<UiConfig>,
        options: WindowOptions,
        proxy: EventLoopProxy<Event>,
        boot: WindowBoot,
    ) -> Result<Self, Box<dyn Error>> {
        let preserve_title = options.window_identity.title.is_some();

        info!(
            "PTY dimensions: {:?} x {:?}",
            display.size_info.screen_lines(),
            display.size_info.columns()
        );

        let window_id = display.window.id();
        // Startup no longer replays the saved window size (the session file's
        // window record is write-only), so the live inner size IS the last
        // normal size at this point.
        let windowed_size = display.window.inner_size().to_logical(display.window.scale_factor);

        // Bootstrap the tab set: fresh/restored windows spawn their first
        // pane here; an attach adopts the detached panes wholesale.
        let mut restore = None;
        let mut seed_pinned = false;
        let (panes, tabs, active_tab, next_pane_id, fresh_first) = match boot {
            WindowBoot::Attach(mut detached) => {
                // Re-point every pane's PTY events at this window before any
                // of them fires again; the leftover DetachedWindow drops with
                // empty panes, so its PTY-shutdown Drop is a no-op.
                for pane in &detached.panes {
                    pane.window_route.store(window_id.into(), Ordering::Relaxed);
                }
                (
                    mem::take(&mut detached.panes),
                    mem::take(&mut detached.tabs),
                    detached.active_tab,
                    detached.next_pane_id,
                    None,
                )
            },
            other => {
                if let WindowBoot::Restore(session) = other {
                    restore = Some(session);
                }
                let mut pty_config = config.pty_config();
                let configured_cwd = pty_config.working_directory.clone();
                let cli_cwd = options
                    .terminal_options
                    .working_directory
                    .as_ref()
                    .filter(|path| path.is_dir())
                    .cloned();
                // A CLI-pinned directory means the user asked for this exact
                // tab: the restore below keeps it instead of dismantling it.
                seed_pinned = cli_cwd.is_some();
                let cli_shell = options.terminal_options.command().map(Into::into);
                pty_config.shell = select_initial_shell(
                    pty_config.shell.take(),
                    Self::default_shell_override(&config),
                    cli_shell,
                );
                options.terminal_options.override_pty_config(&mut pty_config);
                let restored_cwd = restore.as_ref().and_then(|session| {
                    session.tabs.first().and_then(|tab| session::valid_dir(&tab.cwd))
                });
                pty_config.working_directory = preferred_initial_cwd(
                    cli_cwd,
                    restored_cwd,
                    display.startup_directory(),
                    configured_cwd,
                );
                let first_pane = Self::create_pane(
                    &display.size_info,
                    window_id,
                    &config,
                    pty_config,
                    &proxy,
                    0,
                )?;
                let first_id = first_pane.id;
                (
                    vec![first_pane],
                    vec![TabEntry {
                        layout: Layout::Leaf(first_id),
                        active_pane: first_id,
                        has_bell: false,
                        custom_name: None,
                        custom_color: None,
                        launch: TabLaunch::Default,
                        doc: None,
                        image: None,
                        settings: false,
                    }],
                    0,
                    1,
                    Some(first_id),
                )
            },
        };
        let attached = fresh_first.is_none();

        // The pane stub every doc tab's events run against (never in `panes`).
        let doc_pane =
            Self::create_doc_pane(&display.size_info, display.window.id(), &config, &proxy);

        // Create context for the Nebula window.
        let context = WindowContext {
            preserve_title,
            panes,
            tabs,
            active_tab,
            next_pane_id,
            zoom: None,
            split_drag: None,
            proxy,
            display,
            config,
            doc_pane,
            cursor_blink_timed_out: Default::default(),
            blink_focus_pane: None,
            prev_bell_cmd: Default::default(),
            last_pty_resize: None,
            clock_interval: Duration::from_secs(1),
            last_saved_session: None,
            windowed_size,
            session_exempt: false,
            message_buffer: Default::default(),
            window_config: Default::default(),
            event_queue: Default::default(),
            modifiers: Default::default(),
            occluded: Default::default(),
            mouse: Default::default(),
            touch: Default::default(),
            dirty: Default::default(),
        };
        let mut context = context;
        if let Some(first_id) = fresh_first {
            context.run_fastfetch_intro(first_id);
        }
        if let Some(session) = restore {
            context.restore_session_tabs(&session, seed_pinned);
        }
        if attached {
            context.finish_attach();
        }
        Ok(context)
    }

    /// Spawn a new terminal session (PTY + grid + I/O loop) as a pane.
    fn create_pane(
        size_info: &crate::display::SizeInfo,
        window_id: WindowId,
        config: &UiConfig,
        mut pty_config: tty::Options,
        proxy: &EventLoopProxy<Event>,
        pane_id: PaneId,
    ) -> Result<Pane, Box<dyn Error>> {
        crate::platform::environment::prepare_local_pty(&mut pty_config);
        // Per-pane identity for AI-CLI lifecycle hooks: nebula-hook.exe reads
        // it and stamps its pipe messages, so turn state lands on the right
        // tab dot (see `ai_hook`). The same call also exports the terminal
        // identity and control-plane path an in-pane agent needs to find us
        // (see `agent_env`).
        crate::agent_env::apply(&mut pty_config.env, pane_id);

        let window_route = Arc::new(AtomicU64::new(window_id.into()));
        let event_proxy = EventProxy::new_tab(proxy.clone(), window_route.clone(), pane_id);

        // The terminal holds all display state, wrapped in a clonable mutex shared
        // with the PTY I/O loop.
        let terminal = Term::new(config.term_options(), size_info, event_proxy.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        // A working directory that no longer exists — deleted, on an unmounted
        // drive, or a PowerShell non-filesystem PSDrive (Cert:\, HKLM:\, Env:\)
        // reported over OSC — makes CreateProcessW fail with ERROR_DIRECTORY
        // (os error 267) and aborts the whole spawn. Fall back to the process
        // default cwd instead of failing the pane.
        if let Some(dir) = pty_config.working_directory.as_ref() {
            if !dir.is_dir() {
                log::warn!("Ignoring invalid working directory {dir:?}; using default");
                pty_config.working_directory = None;
            }
        }

        let initial_cwd = pty_config
            .working_directory
            .as_ref()
            .cloned()
            .or_else(|| std::env::current_dir().ok())
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let exec_context = crate::runtime_exec::PaneExecContext::from_pty_options(&pty_config);

        // The PTY forks the shell process and retains the master side.
        crate::boot_trace("conpty spawn begin");
        let pty = tty::new(&pty_config, (*size_info).into(), window_id.into())?;
        crate::boot_trace("conpty spawn done");

        #[cfg(not(windows))]
        let master_fd = pty.file().as_raw_fd();
        #[cfg(not(windows))]
        let shell_pid = pty.child().id();
        #[cfg(windows)]
        let shell_pid = pty.child_watcher().pid().map(|p| p.get()).unwrap_or(0);

        // PTY I/O runs on its own thread and updates the shared terminal state.
        let event_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_proxy.clone(),
            pty,
            pty_config.drain_on_exit,
            config.debug.ref_test,
        )?;

        let loop_tx = event_loop.channel();
        let _io_thread = event_loop.spawn();

        // Start cursor blinking, in case `Focused` isn't sent on startup.
        if config.cursor.style().blinking {
            event_proxy.send_event(TerminalEvent::CursorBlinkingChange.into());
        }

        let mut nebula_state = NebulaPaneState::default();
        nebula_state.cwd = initial_cwd;

        Ok(Pane {
            terminal,
            notifier: Notifier(loop_tx),
            search_state: Default::default(),
            inline_search_state: Default::default(),
            id: pane_id,
            title: String::from("shell"),
            exec_context: Some(exec_context),
            ssh_destination: None,
            nebula_state,
            intro_cols: None,
            shell_pid,
            window_route,
            #[cfg(not(windows))]
            master_fd,
        })
    }

    /// A pane-shaped stub for document-viewer tabs: a real (empty) `Term` so
    /// the shared event pipeline has state to borrow, but NO PTY behind it —
    /// the notifier is a sink, so keystrokes routed here are swallowed
    /// instead of reaching some other tab's shell. Never inserted into
    /// `panes`: every `pane(DOC_PANE_ID)` lookup stays `None`, keeping all
    /// the "no pane" degradations (no spinner, no close confirm, …) intact.
    fn create_doc_pane(
        size_info: &crate::display::SizeInfo,
        window_id: WindowId,
        config: &UiConfig,
        proxy: &EventLoopProxy<Event>,
    ) -> Pane {
        let window_route = Arc::new(AtomicU64::new(window_id.into()));
        let event_proxy = EventProxy::new_tab(proxy.clone(), window_route.clone(), DOC_PANE_ID);
        let terminal = Term::new(config.term_options(), size_info, event_proxy);
        Pane {
            terminal: Arc::new(FairMutex::new(terminal)),
            notifier: Notifier(nebula_terminal::event_loop::EventLoopSender::sink()),
            search_state: Default::default(),
            inline_search_state: Default::default(),
            id: DOC_PANE_ID,
            title: String::from("doc"),
            exec_context: None,
            ssh_destination: None,
            nebula_state: NebulaPaneState::default(),
            intro_cols: None,
            shell_pid: 0,
            window_route,
            #[cfg(not(windows))]
            master_fd: -1,
        }
    }

    /// Handle a Nebula tab request. Returns `true` if the window should close
    /// (i.e. the last tab was closed).
    /// 渲染门控看门狗（1 Hz 心跳调用）：Windows 的 `Occluded(false)` 与帧
    /// 回调都可能失约，卡住的 `occluded` / `has_frame` 会让整条 draw 路径
    /// 熄火——窗口"点什么都没反应"，最小化再复原才活过来（issue #21）。
    /// 窗口明明没最小化时强制解锁两道门；正常情况下它们本来就是开的，
    /// 这里是幂等空操作。
    pub fn unstick_render_gates_if_visible(&mut self) {
        if self.display.window.is_minimized().unwrap_or(false) {
            return;
        }
        if self.occluded || !self.display.window.has_frame {
            self.occluded = false;
            self.display.window.has_frame = true;
            self.dirty = true;
        }
    }

    /// Show a fastfetch-style welcome screen in a freshly-created pane.
    fn run_fastfetch_intro(&mut self, pane_id: PaneId) {
        if !self.display.nebula_fetch_enabled {
            return;
        }
        let cols = self.display.size_info.columns();
        if let Some(i) = self.panes.iter().position(|p| p.id == pane_id) {
            let pane = &mut self.panes[i];
            pane.intro_cols = Some(cols);
            pane.notifier
                .notify(nebula_fastfetch_intro_command_for(cols, self.display.nebula_shell));
        }
    }

    /// Spawn a new pane into the pool without attaching it to any tab. `cwd`
    /// overrides the shell's startup directory when set. Returns the new pane's
    /// id, or `None` if the shell failed to start.
    fn spawn_pane_detached(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        size_info: crate::display::SizeInfo,
    ) -> Option<PaneId> {
        self.spawn_pane_detached_with(cwd, size_info, None)
    }

    /// Like [`Self::spawn_pane_detached`] with an optional shell override
    /// (quick-launch profiles run their own command instead of the default).
    fn spawn_pane_detached_with(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        size_info: crate::display::SizeInfo,
        shell: Option<nebula_terminal::tty::Shell>,
    ) -> Option<PaneId> {
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        let window_id = self.display.window.id();
        let mut pty_config = self.config.pty_config();
        // NOTE: the executor choice (PowerShell/Bash) is applied inside
        // `tty::windows::cmdline` from `nebula_settings.txt` whenever
        // `pty_config.shell` is `None` — it must NOT be overridden here, or the
        // bash path would lose its Nebula rcfile (OSC 7 cwd / prompt contract).
        // A profile override (`shell` param) intentionally bypasses that.
        if shell.is_some() {
            pty_config.shell = shell;
        }
        if cwd.is_some() {
            pty_config.working_directory = cwd;
        }
        match Self::create_pane(
            &size_info,
            window_id,
            &self.config,
            pty_config,
            &self.proxy,
            pane_id,
        ) {
            Ok(pane) => {
                self.panes.push(pane);
                Some(pane_id)
            },
            Err(err) => {
                error!("Failed to spawn pane: {err}");
                None
            },
        }
    }

    /// Look up a pane in the pool by id.
    fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    /// Index of a pane in the pool by id.
    fn pane_index(&self, id: PaneId) -> Option<usize> {
        self.panes.iter().position(|p| p.id == id)
    }

    /// 把后台 SSH runtime 的阶段上报转给 display，顺带补上这个 pane 的目标
    /// 地址——事件本身只带 pane 身份，地址在 pane 上。
    pub fn ssh_connect_stage(&mut self, pane: PaneId, stage: crate::ssh_session::SshStage) {
        let destination = self
            .pane_index(pane)
            .and_then(|index| self.panes[index].ssh_destination.clone())
            .unwrap_or_default();
        self.display.ssh_connect_stage(pane, destination, stage);
    }

    /// Working directory of the focused pane (from the shell's `NEBULA|cwd|…`
    /// title report) for a new tab/split to inherit. `None` if unknown.
    fn focused_cwd(&self) -> Option<std::path::PathBuf> {
        let cwd = self.pane(self.focused_pane_id()).map(|p| p.nebula_state.cwd.clone())?;
        // Validate the shell-reported cwd still points at a real directory. A
        // stale or non-filesystem path would otherwise make the new pane's
        // CreateProcessW fail with ERROR_DIRECTORY.
        session::valid_dir(&cwd)
    }

    /// The focused pane's cwd mapped through `\\wsl.localhost` when the pane
    /// belongs to a WSL tab reporting a Linux path — so the directory tree can
    /// follow a WSL shell. Only the drawer uses this: spawning terminals in a
    /// UNC directory has its own semantics and is deliberately not affected.
    ///
    /// 判定与拼接走 [`crate::shell_detect`] 的公共入口，与 GPUI 壳同一份
    /// 规则。注意它在 9P 重定向不可用的机器上一律返回 `None`（见
    /// [`crate::shell_detect::wsl_unc_cwd`]），届时目录树保持上一个已知根，
    /// 不会跳到坏路径。GPUI 壳另有「在来宾里直接跑 git」的 Git 视图路径；
    /// 旧壳刻意不接那条链——新功能只在主壳（GPUI）上长。
    fn focused_wsl_cwd(&self) -> Option<std::path::PathBuf> {
        let raw = self.pane(self.focused_pane_id()).map(|p| p.nebula_state.cwd.clone())?;
        let tab = self.tabs.iter().find(|tab| {
            let mut ids = Vec::new();
            tab.layout.leaves(&mut ids);
            ids.contains(&self.focused_pane_id())
        })?;
        let TabLaunch::Shell { shell, .. } = &tab.launch else { return None };
        let located = crate::shell_detect::wsl_cwd(&raw, shell.program(), shell.args())?;
        crate::shell_detect::wsl_unc_cwd(&located)
    }

    /// Name of the first busy program under any of `pane_ids`, for the close
    /// confirm modal — or `None` when every pane is safe to kill.
    ///
    /// 2026-07-27 用户反馈"node.exe 仍在运行"：`busy_child` 只认得进程快照里
    /// 的 exe 名，而 Claude Code / codex 这类 CLI 是被 node 托管的，快照里就
    /// 只剩宿主解释器。Nebula 另有一份更准的身份——pane 的 `running_program`
    /// （AI hook 直报 `claude`，或 OSC 133 从命令行解析），侧栏图标画的就是
    /// 它。所以：由 `busy_child` 判定"忙不忙"，由 `running_program` 决定"叫
    /// 什么"；后者缺席（无 shell 集成）时退回擦掉 `.exe` 的进程名。
    fn busy_process_in(&self, pane_ids: &[PaneId]) -> Option<String> {
        pane_ids.iter().filter_map(|id| self.pane(*id)).find_map(|pane| {
            let exe = crate::process_tree::busy_child(pane.shell_pid)?;
            let known = pane.nebula_state.running_program.as_deref();
            Some(known.map_or_else(
                || crate::process_tree::display_name(&exe),
                |program| program.to_owned(),
            ))
        })
    }

    /// Update the terminal window to the latest config.
    pub fn update_config(&mut self, new_config: Rc<UiConfig>) {
        let old_config = mem::replace(&mut self.config, new_config);

        // Apply ipc config if there are overrides.
        self.config = self.window_config.override_config_rc(self.config.clone());

        self.display.update_config(&self.config);
        let focused = self.focused_pane_id();
        if let Some(pane) = self.pane(focused) {
            ssh_panes::apply_terminal_config(pane, &self.config);
        }

        // Reload cursor if its thickness has changed.
        if (old_config.cursor.thickness() - self.config.cursor.thickness()).abs() > f32::EPSILON {
            self.display.pending_update.set_cursor_dirty();
        }

        if old_config.font != self.config.font {
            let scale_factor = self.display.window.scale_factor as f32;
            // Do not update font size if it has been changed at runtime.
            if self.display.font_size == old_config.font.size().scale(scale_factor) {
                self.display.font_size = self.config.font.size().scale(scale_factor);
            }

            let font =
                self.display.effective_font(&self.config.font).with_size(self.display.font_size);
            self.display.pending_update.set_font(font);
        }

        // Keep the decoration override in sync without suppressing winit's
        // OS theme events while Nebula's automatic theme mode is enabled.
        self.display.update_window_theme_override(self.config.window.theme());

        // Update display if either padding options or resize increments were changed.
        let window_config = &old_config.window;
        if window_config.padding(1.) != self.config.window.padding(1.)
            || window_config.dynamic_padding != self.config.window.dynamic_padding
            || window_config.resize_increments != self.config.window.resize_increments
        {
            self.display.pending_update.dirty = true;
        }

        // Update title on config reload according to the following table.
        //
        // │cli │ dynamic_title │ current_title == old_config ││ set_title │
        // │ Y  │       _       │              _              ││     N     │
        // │ N  │       Y       │              Y              ││     Y     │
        // │ N  │       Y       │              N              ││     N     │
        // │ N  │       N       │              _              ││     Y     │
        if !self.preserve_title
            && (!self.config.window.dynamic_title
                || self.display.window.title() == old_config.window.identity.title)
        {
            self.display.window.set_title(self.config.window.identity.title.clone());
        }

        let opaque = self.config.window_opacity() >= 1.;

        // Disable shadows for transparent windows on macOS.
        #[cfg(target_os = "macos")]
        self.display.window.set_has_shadow(opaque);

        #[cfg(target_os = "macos")]
        self.display.window.set_option_as_alt(self.config.window.option_as_alt());

        // Change opacity and blur state.
        self.display.window.set_transparent(!opaque);
        // 模糊开关的权威在 nebula_settings.txt（设置面板写的就是它），
        // 基础配置侧的 `window.blur` 只是同名字段，跟它没有同步。
        self.display.window.set_blur(self.display.nebula_blur);

        // Update hint keys.
        self.display.hint_state.update_alphabet(self.config.hints.alphabet());

        // Update cursor blinking.
        let event = Event::new(TerminalEvent::CursorBlinkingChange.into(), None);
        self.event_queue.push(event.into());

        self.dirty = true;
    }

    /// Get reference to the window's configuration.
    #[cfg(unix)]
    pub fn config(&self) -> &UiConfig {
        &self.config
    }

    /// Clear the window config overrides.
    #[cfg(unix)]
    pub fn reset_window_config(&mut self, config: Rc<UiConfig>) {
        // Clear previous window errors.
        self.message_buffer.remove_target(LOG_TARGET_IPC_CONFIG);

        self.window_config.clear();

        // Reload current config to pull new IPC config.
        self.update_config(config);
    }

    /// Add new window config overrides.
    #[cfg(unix)]
    pub fn add_window_config(&mut self, config: Rc<UiConfig>, options: &ParsedOptions) {
        // Clear previous window errors.
        self.message_buffer.remove_target(LOG_TARGET_IPC_CONFIG);

        self.window_config.extend_from_slice(options);

        // Reload current config to pull new IPC config.
        self.update_config(config);
    }

}

impl Drop for WindowContext {
    fn drop(&mut self) {
        // Final session snapshot at teardown. Quitting by closing every tab
        // one by one reaches this with `tabs` already empty — persisting that
        // empty list is exactly what makes the next launch start clean.
        // Closing the whole window (X / Alt+F4 / shortcut) keeps the tabs, so
        // they restore. Crash/kill paths never get here and are covered by
        // the 1 Hz autosave instead — which is also why this one (and only
        // this one) stamps `clean_exit`: reaching Drop IS the definition of
        // a clean exit.
        if !self.session_exempt {
            session::save_final(&mut self.session_snapshot());
        }

        // Shutdown every pane's PTY.
        for pane in &self.panes {
            let _ = pane.notifier.0.send(Msg::Shutdown);
        }
    }
}

#[cfg(test)]
mod startup_shell_tests;
