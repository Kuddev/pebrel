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

/// Continue one live AI conversation in a fresh tab with a new session id.
    ///
    /// This deliberately recreates the shell instead of cloning a PTY/process.
    /// Profile/SSH tabs are excluded: injecting into a profile that starts the
    /// agent directly, or into an SSH authentication prompt, would turn the
    /// command into user input at the wrong protocol layer.
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

    /// Draw the window.
    pub fn draw(&mut self, scheduler: &mut Scheduler) {
        self.display.window.requested_redraw = false;
        self.sync_chrome_tabs();
        // The drawer follows the focused pane: its VIEW routes to SFTP only
        // while an SSH pane with the matching destination is focused, and the
        // directory tree follows the focused pane's cwd (throttled inside).
        let focused_ssh =
            self.pane(self.focused_pane_id()).and_then(|pane| pane.ssh_destination.clone());
        self.display.route_side_panel(focused_ssh.as_deref());
        let panel_cwd = self.focused_cwd().or_else(|| self.focused_wsl_cwd());
        // 命令面板的「工作目录」组也认这个值（WSL 路径已映射成 `\\wsl$\…`，
        // 复制出去和丢给资源管理器都能用）。抽屉是节流的，这里不能顺手复用
        // 它的内部状态——面板要的是**当前**目录，不是抽屉上次同步到的那个。
        self.display.nebula_focused_cwd = panel_cwd.clone();
        self.display.side_panel_sync(panel_cwd);

        // Chrome clock: unfocused/idle windows keep only the 1 Hz state watchdog;
        // visible animations use 12.5 fps. Re-arm whenever the cadence class changes.
        let clock_timer = TimerId::new(Topic::NebulaClock, self.display.window.id());
        let interval = chrome_clock_interval(
            self.display.window.has_focus(),
            self.display.any_tab_running()
                || self.display.ssh_test_running()
                || self.display.any_tab_flashing(),
            self.display.chrome_editor_active(),
            self.display.chrome_animating(),
        );
        if self.clock_interval != interval {
            scheduler.unschedule(clock_timer);
            self.clock_interval = interval;
        }
        if !scheduler.scheduled(clock_timer) {
            let event = Event::new(EventType::NebulaTick, self.display.window.id());
            scheduler.schedule(event, interval, true, clock_timer);
        }

        if self.occluded {
            return;
        }
        self.dirty = false;

        // Force the display to process any pending display update.
        self.display.process_renderer_update();

        // Request immediate re-draw if visual bell animation is not finished yet.
        if !self.display.visual_bell.completed() {
            // We can get an OS redraw which bypasses nebula's frame throttling, thus
            // marking the window as dirty when we don't have frame yet.
            if self.display.window.has_frame {
                self.display.window.request_redraw();
            } else {
                self.dirty = true;
            }
        }

        // Chrome sidebar/drawer transitions need display-rate frames until settled.
        if self.display.chrome_animating() {
            if self.display.window.has_frame {
                self.display.window.request_redraw();
            } else {
                self.dirty = true;
            }
        }

        // Redraw the window: walk the active tab's layout tree and draw each
        // pane in its rectangle. A single-pane tab uses the simple full-window
        // path; multi-pane tabs draw every leaf then overlay dividers + dimming.
        let pane_rects = self.layout_geometry(false).0;
        let divider_rects = self.layout_geometry(true).1;
        let focused = self.focused_pane_id();
        // 助手建议条（spec 001）跟随焦点 pane：绘制层只认 Display 自己的
        // 快照字段（SSH 撤销条同款模式），此处每帧同步一次。
        self.display.nebula_ai_fix_bar =
            self.pane_index(focused).and_then(|idx| self.panes[idx].nebula_state.ai_fix.clone());

        // Settings is rendered inside the normal tab content card; it is not
        // a modal and therefore keeps the tab/sidebar chrome fully usable.
        if self.tabs.get(self.active_tab).is_some_and(|tab| tab.settings) {
            self.display.begin_pane_frame(&self.config);
            self.display.draw_settings_frame(scheduler);
            return;
        }

        // Document-viewer tab: no pane, no grid. Draw the doc into the tab's
        // content rect; `present_frame` inside lays the normal chrome on top.
        if let Some(image) = self.tabs.get(self.active_tab).and_then(|tab| tab.image.clone()) {
            let view = pane_rects.first().map(|(_, view)| *view).unwrap_or(self.display.size_info);
            self.display.begin_pane_frame(&self.config);
            self.display.draw_image_frame(&image, view, scheduler);
            return;
        }

        if let Some(doc) = self.tabs.get_mut(self.active_tab).and_then(|tab| tab.doc.as_mut()) {
            let view = pane_rects.first().map(|(_, view)| *view).unwrap_or(self.display.size_info);
            self.display.begin_pane_frame(&self.config);
            self.display.draw_doc_frame(doc, view, scheduler);
            return;
        }

        // 连接卡片只画在聚焦 pane 里，display 侧只有几何、没有身份。
        self.display.set_focused_pane(focused);

        // 焦点 pane 变了：blink 定时器还按旧终端的样式在跑（或没跑）。给
        // 新聚焦终端补发一次 CursorBlinkingChange，让它按自己的样式起表。
        if self.blink_focus_pane != Some(focused) {
            self.blink_focus_pane = Some(focused);
            if let Some(idx) = self.pane_index(focused) {
                let pane = &self.panes[idx];
                EventProxy::new_tab(self.proxy.clone(), pane.window_route.clone(), pane.id)
                    .send_event(TerminalEvent::CursorBlinkingChange.into());
            }
        }

        if pane_rects.len() <= 1 {
            let id = pane_rects.first().map(|(id, _)| *id).unwrap_or(focused);
            if let Some(idx) = self.pane_index(id) {
                let pane = &mut self.panes[idx];
                let terminal_arc = pane.terminal.clone();
                let terminal = terminal_arc.lock();
                self.display.draw(
                    terminal,
                    scheduler,
                    &self.message_buffer,
                    &self.config,
                    &mut pane.search_state,
                    &mut pane.nebula_state,
                );
            }
        } else {
            self.display.begin_pane_frame(&self.config);
            let mut dim_rects = Vec::new();
            // The whole-window clear must not be tied to pane_rects[0]: a
            // layout leaf whose pane is gone (or a doc sentinel) is skipped
            // below, and skipping the clearing pane would leave every later
            // frame compositing over stale buffer contents (ghost frames).
            let mut cleared = false;
            // Pane focus AND window focus together decide the cursor's
            // focused look — a focused pane in an unfocused window must show
            // the hollow unfocused cursor, exactly like the single-pane path.
            let window_focused = self.display.window.has_focus();
            for (id, view) in pane_rects.iter() {
                let Some(idx) = self.pane_index(*id) else { continue };
                let is_focused = *id == focused;
                if !is_focused {
                    dim_rects.push((
                        view.padding_x(),
                        view.padding_y(),
                        // Split views use asymmetric padding: the sidebar is
                        // included on the left while the right keeps only the
                        // normal content margin. Using `2 * padding_x` here
                        // dropped the entire asymmetric difference from the
                        // dim veil, leaving a bright uncovered strip.
                        view.width() - view.padding_x() - view.padding_right(),
                        view.height() - view.padding_y() - view.padding_bottom(),
                    ));
                }
                let pane = &mut self.panes[idx];
                let terminal_arc = pane.terminal.clone();
                let terminal = terminal_arc.lock();
                self.display.draw_pane_view(
                    terminal,
                    &self.message_buffer,
                    &self.config,
                    &mut pane.search_state,
                    &mut pane.nebula_state,
                    *view,
                    is_focused && window_focused,
                    !cleared,
                );
                cleared = true;
            }
            if !cleared {
                crate::display::nebula_debug_log(format!(
                    "render_clear_missing active_tab={} layout_panes={} live_panes={} focused={focused}",
                    self.active_tab,
                    pane_rects.len(),
                    self.panes.len(),
                ));
            }
            self.display.draw_split_overlays(&dim_rects, &divider_rects);
            self.display.finish_pane_frame(scheduler);
        }

        // Startup profiling: the process-wide first completed frame.
        {
            use std::sync::atomic::AtomicBool;
            static FIRST_FRAME: AtomicBool = AtomicBool::new(false);
            if !FIRST_FRAME.swap(true, Ordering::Relaxed) {
                crate::boot_trace("first frame drawn");
            }
        }
    }

    /// Reorder the tab bar by moving the tab at index `from` to index `to`.
    /// With the pane pool the bar always lists every tab in storage order
    /// (displayed == storage index), so this is unconditional.
    fn move_tab(&mut self, from: usize, to: usize) {
        let len = self.tabs.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let entry = self.tabs.remove(from);
        self.tabs.insert(to, entry);
        // Keep the same tab focused: remap the active index through the move.
        self.active_tab = Self::shifted_index(self.active_tab, from, to);
        self.sync_chrome_tabs();
        self.dirty = true;
    }

    /// New position of `idx` after the element at `from` is removed and
    /// re-inserted at `to` (a single-element move within the vector).
    fn shifted_index(idx: usize, from: usize, to: usize) -> usize {
        if idx == from {
            to
        } else if from < to && idx > from && idx <= to {
            idx - 1
        } else if from > to && idx >= to && idx < from {
            idx + 1
        } else {
            idx
        }
    }

    fn sync_chrome_tabs(&mut self) {
        let special = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.doc.is_some() || tab.image.is_some() || tab.settings);
        self.display.set_special_tab_active(special);
        self.display.set_settings_tab_active(
            self.tabs.get(self.active_tab).is_some_and(|tab| tab.settings),
        );
        // The visible tab's activity is seen by definition — consume its
        // flag before it can render (dots are for background tabs only).
        if let Some(id) = self.tabs.get(self.active_tab).map(|t| t.active_pane) {
            if let Some(i) = self.pane_index(id) {
                self.panes[i].nebula_state.finished_unseen = false;
                self.panes[i].nebula_state.needs_attention = false;
                self.panes[i].nebula_state.failed_unseen = false;
            }
        }

        let mut labels = Vec::with_capacity(self.tabs.len());
        let mut colors = Vec::with_capacity(self.tabs.len());
        let mut dots = Vec::with_capacity(self.tabs.len());
        let mut running = Vec::with_capacity(self.tabs.len());
        let mut attention = Vec::with_capacity(self.tabs.len());
        let mut failed = Vec::with_capacity(self.tabs.len());
        let mut flashing = Vec::with_capacity(self.tabs.len());
        let mut logos = Vec::with_capacity(self.tabs.len());
        let mut shells = Vec::with_capacity(self.tabs.len());
        let mut ai_fork = Vec::with_capacity(self.tabs.len());
        // 静默行右侧的 shell 短标；Default 启动的 tab 用当前默认 shell 的。
        let default_tag = self.display.default_shell_tag();
        let ui_language = self.display.ui_language();
        for tab in &self.tabs {
            let pane = self.pane(tab.active_pane);
            let state = pane.map(|p| &p.nebula_state);
            // Use custom name if set, otherwise derive from cwd/title
            let mut label = if tab.settings {
                format!("\u{eb51} {}", ui_language.pick("设置", "Settings"))
            } else if let Some(custom) = &tab.custom_name {
                custom.clone()
            } else {
                pane.map(Self::chrome_tab_label).unwrap_or_default()
            };
            // Program icon (Nerd Font) in front of the label while a command
            // runs — the sidebar shows WHAT each tab is busy with. AI clients
            // with a real brand logo skip the glyph: the
            // display layer textures the actual mark into the icon slot.
            let logo =
                state.and_then(|s| s.running_program.as_deref()).and_then(crate::display::ai_logo);
            if let Some(program) = state.and_then(|s| s.running_program.as_deref()) {
                if logo.is_none() {
                    label = format!("{} {label}", crate::display::program_icon(program));
                }
            }
            logos.push(logo);
            labels.push(label);
            colors.push(tab.custom_color);
            shells.push(match &tab.launch {
                TabLaunch::Default => default_tag.clone(),
                TabLaunch::Shell { name, .. } => crate::shell_detect::shell_short_tag(name),
                // SSH 行的身份是目标主机（标签本身就写着），短标只说环境。
                TabLaunch::Ssh(_) => "ssh".to_owned(),
                TabLaunch::Profile(_)
                | TabLaunch::Document(_)
                | TabLaunch::Image(_)
                | TabLaunch::Settings => String::new(),
            });
            ai_fork.push(
                matches!(&tab.launch, TabLaunch::Default | TabLaunch::Shell { .. })
                    && state
                        .and_then(|state| state.ai_session.as_ref())
                        .and_then(|identity| {
                            crate::ai_agents::AgentKind::parse(&identity.source)
                                .and_then(|agent| agent.fork_command(&identity.session_id))
                        })
                        .is_some(),
            );
            // Unseen-result dot: bell in a background tab, a tracked command
            // that finished unseen, or a tracked program parked at "waiting
            // for input" (claude between turns). The ring collapsing into a
            // dot IS the "turn finished, your move" signal — also on the
            // visible tab, where a merely-paused ring still read as busy.
            dots.push(
                tab.has_bell
                    || state.is_some_and(|s| {
                        s.finished_unseen || (s.command_started.is_some() && s.awaiting_input)
                    }),
            );
            // Spinner only while the command actually works; once it rang BEL
            // and waits for input the dot above takes over.
            running.push(state.is_some_and(|s| s.command_started.is_some() && !s.awaiting_input));
            attention.push(state.is_some_and(|s| s.needs_attention));
            failed.push(state.is_some_and(|s| s.failed_unseen));
            // 对勾只在成功收尾后的一小段里亮着，随后落回圆点。
            flashing.push(state.is_some_and(|s| {
                s.finished_at.is_some_and(|at| at.elapsed() < crate::display::BADGE_FLASH)
            }));
        }
        let active = self.active_tab.min(labels.len().saturating_sub(1));
        // displayed == storage index always holds now, so the bar is reorderable.
        self.display.set_chrome_tabs(
            labels, colors, dots, running, attention, failed, flashing, logos, shells, ai_fork,
            active, true,
        );
    }

    fn chrome_tab_label(pane: &Pane) -> String {
        let cwd = pane.nebula_state.cwd.trim();
        if !cwd.is_empty() {
            // Just the directory's own name: a full path wall-to-walls the
            // sidebar row and kills the design's breathing room. The last
            // meaningful component is what identifies the workspace anyway.
            let name = cwd
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(cwd);
            return name.to_owned();
        }

        if pane.title != "shell" && !pane.title.trim().is_empty() {
            return pane.title.clone();
        }

        std::env::current_dir()
            .ok()
            .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| ".".to_owned())
    }

    /// Commit the final DPI and physical size held by the native move tracker.
    /// Applying the factor first keeps the logical windowed bounds correct and
    /// collapses the cross-monitor work into one display update.
    pub fn apply_pending_native_transition(&mut self) {
        if self.display.window.native_live_move() {
            return;
        }

        if let Some(scale_factor) = self.display.window.take_pending_scale_factor() {
            let start = Instant::now();
            self.display.apply_scale_factor_change(scale_factor, &self.config);
            crate::display::nebula_debug_log(format!(
                "winmove pending_scale {scale_factor} applied in {:?}",
                start.elapsed()
            ));
            self.dirty = true;
        }

        if let Some(size) = self.display.window.take_pending_inner_size() {
            crate::display::nebula_debug_log(format!(
                "winmove pending_size {}x{} applied",
                size.width, size.height
            ));
            if self.display.window.allows_drag_resize() {
                self.windowed_size = size.to_logical(self.display.window.scale_factor);
            }
            self.display.pending_update.set_dimensions(size);
            self.dirty = true;
        }
    }

    /// Process events for this terminal window.
    pub fn handle_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event_proxy: &EventLoopProxy<Event>,
        clipboard: &mut Clipboard,
        scheduler: &mut Scheduler,
        event: WinitEvent<Event>,
    ) {
        // `Window::theme()` can retain a stale manual override. The event-loop
        // query is system-wide and lets automatic mode react immediately.
        self.display.sync_system_theme(event_loop.system_theme());

        match event {
            WinitEvent::AboutToWait
            | WinitEvent::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                // Skip further event handling with no staged updates.
                // A native DPI transition can stage a Display update without
                // adding a synthetic winit event, so the pending flag is part
                // of this fast-path decision.
                if self.event_queue.is_empty() && !self.display.pending_update.dirty {
                    return;
                }

                // Continue to process all pending events.
            },
            event => {
                self.event_queue.push(event);
                return;
            },
        }

        self.preprocess_split_mouse();

        // Flag background tabs whose panes rang a bell (🔔 in the tab bar).
        let bell_panes: Vec<u64> = self
            .event_queue
            .iter()
            .filter_map(|e| match e {
                WinitEvent::UserEvent(ev) => ev.terminal_bell_pane(),
                _ => None,
            })
            .collect();
        for pane_id in bell_panes {
            self.mark_pane_bell(pane_id);
        }

        // Any key press means the user is interacting again: resume the
        // focused pane's sidebar spinner (claude's next turn after its
        // wait-for-input bell). A stray clear is harmless — the next bell
        // pauses it again.
        let key_pressed = self.event_queue.iter().any(|e| {
            matches!(
                e,
                WinitEvent::WindowEvent {
                    event: WindowEvent::KeyboardInput { event: key, .. },
                    ..
                } if key.state == ElementState::Pressed
            )
        });
        if key_pressed {
            let focused = self.focused_pane_id();
            if let Some(i) = self.pane_index(focused) {
                self.panes[i].nebula_state.awaiting_input = false;
                // 打字即表态：人已经在这个 pane 上动手了，徽章再催就是噪声。
                self.panes[i].nebula_state.needs_attention = false;
            }
        }

        // In a split, a terminal-content mouse press moves keyboard focus to
        // the clicked pane. Right-click paste and middle-click selection paste
        // must target the pane under the pointer as well; otherwise they use
        // the previous keyboard focus and write into a neighbouring terminal.
        // Resolve focus from the click position before routing this batch so the
        // click lands on the pane the user aimed at.
        if self.display.nebula_confirm.is_none() && !matches!(self.active_layout(), Layout::Leaf(_))
        {
            let ffm = self.config.mouse.focus_follows_mouse;
            // The click's real position is the latest CursorMoved in THIS batch:
            // winit's MouseInput carries no coordinates, and `self.mouse` still
            // holds the PREVIOUS batch's position — this batch's CursorMoved that
            // moved the pointer to the click hasn't been routed to the input
            // processor yet. Using the stale `self.mouse` here focuses the wrong
            // pane, so typed input lands in it (the "split typing bleeds into the
            // other pane" bug). Fall back to `self.mouse` only when the pointer
            // didn't move this batch (then it is already the current position).
            let latest_pos = self.event_queue.iter().rev().find_map(|e| match e {
                WinitEvent::WindowEvent {
                    event: WindowEvent::CursorMoved { position, .. },
                    ..
                } => Some((position.x as f32, position.y as f32)),
                _ => None,
            });
            let clicked = self.event_queue.iter().any(|e| {
                matches!(
                    e,
                    WinitEvent::WindowEvent {
                        event: WindowEvent::MouseInput { state: ElementState::Pressed, button, .. },
                        ..
                    } if pane_focus_button(button)
                )
            });
            // A terminal mouse press always refocuses the clicked pane;
            // focus-follows-mouse also refocuses on plain pointer motion.
            let target = if clicked {
                latest_pos.or(Some((self.mouse.x as f32, self.mouse.y as f32)))
            } else if ffm {
                latest_pos
            } else {
                None
            };
            if let Some((px, py)) = target {
                if let Some(id) = self.pane_at_position(px, py) {
                    if self.tabs[self.active_tab].active_pane != id {
                        self.tabs[self.active_tab].active_pane = id;
                        self.dirty = true;
                    }
                }
            }
        }

        // Route each event to its own pane. A Terminal event names the pane
        // that produced it and must update THAT pane's state; window input
        // (keyboard, mouse) always belongs to the focused pane of the active
        // tab. Resolving one target for the whole batch let a background
        // pane's output drag the batch — keystrokes included — to itself,
        // typing into the wrong PTY.
        // Multi-line paste confirmation is a transaction bound to the pane
        // that opened it. Route both keyboard Enter and a modal-button click
        // to that pane even when the centered button lies over another split.
        let normal_focus = self.focused_pane_id();
        let focused_id =
            routed_input_pane(self.display.nebula_confirm.as_ref(), normal_focus, |pane_id| {
                self.pane_index(pane_id).is_some()
            });
        // A doc tab has no pane: its events run against `doc_pane` below so
        // chrome interaction (tab switching, closing, the sidebar) keeps
        // working; anything typed lands in the sink notifier.
        let special_tab = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.doc.is_some() || tab.image.is_some() || tab.settings);
        let focused = match self.pane_index(focused_id) {
            Some(index) => Some(index),
            None if special_tab => None,
            None => return,
        };

        // Point input/hint hit-testing at the focused pane's rectangle so mouse
        // coordinates map into its (possibly partial) grid. `None` → full window.
        let pane_rects = self.layout_geometry(false).0;
        let pane_view = if pane_rects.len() > 1 {
            pane_rects.iter().find(|(id, _)| *id == focused_id).map(|(_, v)| *v)
        } else {
            None
        };
        self.display.nebula_pane_view = pane_view;

        let old_is_searching =
            focused.is_some_and(|index| self.panes[index].search_state.history_index.is_some());

        let target_of = |event: &WinitEvent<Event>| match event {
            WinitEvent::UserEvent(event) => event.terminal_tab_id().unwrap_or(focused_id),
            _ => focused_id,
        };
        // Consume the batch in order, one processor per run of consecutive
        // events sharing a target pane.
        let mut events = mem::take(&mut self.event_queue).into_iter().peekable();
        while let Some(event) = events.next() {
            let target_id = target_of(&event);
            let (pane, doc, image) = match self.pane_index(target_id) {
                Some(pane_idx) => (&mut self.panes[pane_idx], None, None),
                None if target_id == DOC_PANE_ID && special_tab => {
                    let tab = &mut self.tabs[self.active_tab];
                    (&mut self.doc_pane, tab.doc.as_mut(), tab.image.as_mut())
                },
                None => {
                    // Source pane is gone (closed with output still in flight):
                    // drop its events, keep the rest of the batch.
                    while events.next_if(|event| target_of(event) == target_id).is_some() {}
                    continue;
                },
            };

            let terminal_arc = pane.terminal.clone();
            let mut terminal = terminal_arc.lock();
            let context = ActionContext {
                pane_id: pane.id,
                cursor_blink_timed_out: &mut self.cursor_blink_timed_out,
                prev_bell_cmd: &mut self.prev_bell_cmd,
                message_buffer: &mut self.message_buffer,
                inline_search_state: &mut pane.inline_search_state,
                search_state: &mut pane.search_state,
                nebula_state: &mut pane.nebula_state,
                ssh_destination: pane.ssh_destination.as_deref(),
                doc,
                image,
                modifiers: &mut self.modifiers,
                notifier: &mut pane.notifier,
                display: &mut self.display,
                windowed_size: &mut self.windowed_size,
                mouse: &mut self.mouse,
                touch: &mut self.touch,
                dirty: &mut self.dirty,
                occluded: &mut self.occluded,
                terminal: &mut terminal,
                #[cfg(not(windows))]
                master_fd: pane.master_fd,
                #[cfg(not(windows))]
                shell_pid: pane.shell_pid,
                preserve_title: self.preserve_title,
                config: &self.config,
                event_proxy,
                #[cfg(target_os = "macos")]
                event_loop,
                clipboard,
                scheduler,
            };
            let mut processor = input::Processor::new(context);
            processor.handle_event(event);
            while let Some(event) = events.next_if(|event| target_of(event) == target_id) {
                processor.handle_event(event);
            }
        }

        if self.display.pending_update.terminal_colors_dirty() {
            // 主题切换必须覆盖所有 tab、分屏和文档占位终端。这里尚未取得焦点
            // terminal 的锁，可逐个清理 OSC 覆盖而不产生重复加锁死锁。
            //
            // 顺带告诉订阅了 DECSET 2031 的子进程新的亮暗（`CSI ? 997;N n`）：
            // 上面那行 `reset_dynamic_colors` 只是让 OSC 11 **下次被问到**时报出
            // 新背景，而已经跑着的 TUI 不会再问第二次。少了这条通知，深色切浅色
            // 之后 nvim/codex 会继续用为深底挑的配色画在白底上。
            let dark = {
                let bg = self.display.colors[nebula_terminal::vte::ansi::NamedColor::Background];
                nebula_terminal::term::background_is_dark(bg.r, bg.g, bg.b)
            };
            for pane in &self.panes {
                let mut terminal = pane.terminal.lock();
                terminal.reset_dynamic_colors();
                terminal.set_color_scheme(dark);
            }
            let mut doc = self.doc_pane.terminal.lock();
            doc.reset_dynamic_colors();
            doc.set_color_scheme(dark);
            drop(doc);
            self.dirty = true;
        }

        // Post-batch display housekeeping reads the focused pane's terminal
        // (the doc stub when a doc tab is active).
        let terminal_arc = match focused {
            Some(index) => self.panes[index].terminal.clone(),
            None => self.doc_pane.terminal.clone(),
        };
        let mut terminal = terminal_arc.lock();

        // Process DisplayUpdate events.
        if self.display.pending_update.dirty {
            let update_start = Instant::now();
            let pane = match focused {
                Some(index) => &mut self.panes[index],
                None => &mut self.doc_pane,
            };
            Self::submit_display_update(
                &mut terminal,
                &mut self.display,
                &mut pane.notifier,
                &self.message_buffer,
                &mut pane.search_state,
                old_is_searching,
                &self.config,
            );
            crate::display::nebula_debug_log(format!(
                "winmove display_update in {:?}",
                update_start.elapsed()
            ));
            self.dirty = true;

            // Deferred PTY resize: a lone resize (startup, maximize, sidebar
            // toggle) passes through IMMEDIATELY — startup latency is the
            // first principle. Only a rapid follow-up within the coalescing
            // window (an interactive drag) defers to the trailing-edge settle
            // timer, so ConPTY's per-resize viewport repaint fires once at
            // drag end instead of per tick.
            if self.display.nebula_pty_resize_pending {
                let now = Instant::now();
                let dragging = self
                    .last_pty_resize
                    .is_some_and(|t| now.duration_since(t) < Duration::from_millis(300));
                if dragging {
                    let timer = TimerId::new(Topic::NebulaResizeSettle, self.display.window.id());
                    scheduler.unschedule(timer);
                    let event =
                        Event::new(EventType::NebulaResizeSettled, self.display.window.id());
                    scheduler.schedule(event, Duration::from_millis(150), false, timer);
                } else {
                    // Leading edge: commit every grid before its PTY.  This is
                    // the first size in the drag sequence, so committing it
                    // immediately preserves startup/single-resize latency
                    // while every following tick can remain visual-only.
                    self.display.nebula_pty_resize_pending = false;
                    self.last_pty_resize = Some(now);
                    drop(terminal);
                    self.resize_active_layout();
                    terminal = terminal_arc.lock();
                }
            }

            // During a drag the renderer uses each pane's new visual viewport,
            // while its grid deliberately remains at the last ConPTY-committed
            // size. Do not reflow split grids here: that would recreate the
            // width-history divergence this debounce exists to prevent.
        }

        if self.dirty || self.mouse.hint_highlight_dirty {
            let view = self.display.pane_view();
            let visual_point = self.mouse.point(&view, &*terminal);
            let pane = match focused {
                Some(index) => &self.panes[index],
                None => &self.doc_pane,
            };
            let hint_point = pane
                .nebula_state
                .terminal_math_source_point(
                    visual_point,
                    self.mouse.cell_side,
                    terminal.viewport_origin_for(view.screen_lines()),
                )
                .0;
            self.dirty |= self.display.update_highlighted_hints(
                &terminal,
                &self.config,
                &self.mouse,
                hint_point,
                self.modifiers.state(),
            );
            self.mouse.hint_highlight_dirty = false;
        }

        // Don't call `request_redraw` when event is `RedrawRequested` since the `dirty` flag
        // represents the current frame, but redraw is for the next frame.
        if self.dirty
            && self.display.window.has_frame
            && !self.occluded
            && !matches!(event, WinitEvent::WindowEvent { event: WindowEvent::RedrawRequested, .. })
        {
            self.display.window.request_redraw();
        }
    }

    /// ID of this terminal context.
    pub fn id(&self) -> WindowId {
        self.display.window.id()
    }

    /// Write the ref test results to the disk.
    pub fn write_ref_test_results(&self) {
        // Dump grid state.
        let focused = self.focused_pane_id();
        let mut grid =
            self.pane(focused).expect("focused pane exists").terminal.lock().grid().clone();
        grid.initialize_all();
        grid.truncate();

        let serialized_grid = json::to_string(&grid).expect("serialize grid");

        let size_info = &self.display.size_info;
        let size = TermSize::new(size_info.columns(), size_info.screen_lines());
        let serialized_size = json::to_string(&size).expect("serialize size");

        let serialized_config = format!("{{\"history_size\":{}}}", grid.history_size());

        File::create("./grid.json")
            .and_then(|mut f| f.write_all(serialized_grid.as_bytes()))
            .expect("write grid.json");

        File::create("./size.json")
            .and_then(|mut f| f.write_all(serialized_size.as_bytes()))
            .expect("write size.json");

        File::create("./config.json")
            .and_then(|mut f| f.write_all(serialized_config.as_bytes()))
            .expect("write config.json");
    }

    /// Flush the deferred PTY resize once an interactive resize settles
    /// (`Topic::NebulaResizeSettle` fired): every pane's PTY learns its final
    /// size in one shot, and pristine panes re-print the welcome intro once —
    /// instead of per drag tick, which flooded the scrollback with ConPTY's
    /// per-resize viewport repaints.
    pub fn apply_settled_pty_resize(&mut self) {
        if !mem::take(&mut self.display.nebula_pty_resize_pending) {
            return;
        }
        self.last_pty_resize = Some(Instant::now());
        // Commit the final geometry in the same ordering as the leading edge:
        // output parsed after the PTY resize now sees the exact grid reflow
        // history used by ConPTY, without paying for per-tick resize storms.
        self.resize_active_layout();
    }

    /// Submit the pending changes to the `Display`.
    fn submit_display_update(
        terminal: &mut Term<EventProxy>,
        display: &mut Display,
        notifier: &mut Notifier,
        message_buffer: &MessageBuffer,
        search_state: &mut SearchState,
        old_is_searching: bool,
        config: &UiConfig,
    ) {
        // Compute cursor positions before resize.
        let num_lines = terminal.screen_lines();
        let cursor_at_bottom = terminal.grid().cursor.point.line + 1 == num_lines;
        let origin_at_bottom = if terminal.mode().contains(TermMode::VI) {
            terminal.vi_mode_cursor.point.line == num_lines - 1
        } else {
            search_state.direction == Direction::Left
        };

        display.handle_update(terminal, notifier, message_buffer, search_state, config);

        let new_is_searching = search_state.history_index.is_some();
        if !old_is_searching && new_is_searching {
            // Scroll on search start to make sure origin is visible with minimal viewport motion.
            let display_offset = terminal.grid().display_offset();
            if display_offset == 0 && cursor_at_bottom && !origin_at_bottom {
                terminal.scroll_display(Scroll::Delta(1));
            } else if display_offset != 0 && origin_at_bottom {
                terminal.scroll_display(Scroll::Delta(-1));
            }
        }
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
