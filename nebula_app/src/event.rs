//! Process window events.

use crate::ConfigMonitor;
use glutin::config::GetGlConfig;
use std::borrow::Cow;
use std::cmp::min;
use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Debug;
#[cfg(not(windows))]
use std::os::unix::io::RawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};
use std::{env, f32, mem};

use ahash::RandomState;
use crossfont::Size as FontSize;
use glutin::config::Config as GlutinConfig;
use glutin::display::GetGlDisplay;
use log::{debug, error, info, warn};
use serde_json::Value;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event as WinitEvent, Ime, Modifiers, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, DeviceEvents, EventLoop, EventLoopProxy};
use winit::raw_window_handle::HasDisplayHandle;
use winit::window::WindowId;

use global_hotkey::hotkey::{Code, HotKey, Modifiers as HotKeyModifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use nebula_terminal::event::{Event as TerminalEvent, EventListener, Notify};
use nebula_terminal::event_loop::Notifier;
use nebula_terminal::grid::{BidirectionalIterator, Dimensions, Scroll};
use nebula_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use nebula_terminal::selection::{Selection, SelectionType};
use nebula_terminal::term::cell::Flags;
use nebula_terminal::term::search::{Match, RegexSearch};
use nebula_terminal::term::{ClipboardType, Term, TermMode};
use nebula_terminal::vte::ansi::NamedColor;

#[cfg(unix)]
use crate::cli::ParsedOptions;
use crate::cli::{Options as CliOptions, WindowOptions};
use crate::clipboard::Clipboard;
use crate::config::reload::ReloadWorker;
use crate::config::ui_config::{HintAction, HintInternalAction};
use crate::config::{self, UiConfig};
#[cfg(not(windows))]
use crate::daemon::foreground_process_path;
use crate::daemon::spawn_daemon;
use crate::display::NebulaPaneState;
use crate::display::color::Rgb;
use crate::display::hint::HintMatch;
use crate::display::window::{ImeInhibitor, Window};
use crate::display::{Display, Preedit, SizeInfo, ToastKind, UiLanguage};
use crate::input::{self, ActionContext as _};
use crate::logging::{LOG_TARGET_CONFIG, LOG_TARGET_WINIT};
use crate::message_bar::{Message, MessageBuffer, MessageType};
#[cfg(unix)]
use crate::polling::ipc::{self, SocketReply};
use crate::runtime_api::{ApiError, RuntimeCommand, RuntimeDispatch, RuntimeHub, RuntimeSnapshot};
use crate::scheduler::{Scheduler, TimerId, Topic};
use crate::window_context::{DetachedWindow, WindowBoot, WindowContext};
use crate::window_transition::{NativeWindowStage, NativeWindowStageTracker};

mod action_context;
mod agent_runtime;
mod input_dispatch;
mod input_state;
mod proxy;
mod quick_hotkey;
mod runtime_control;
mod search_state;
mod types;

pub use action_context::ActionContext;

pub use input_state::{ClickState, Mouse, TouchPurpose, TouchZoom};
pub use proxy::EventProxy;
pub use search_state::{InlineSearchState, SearchState};
pub use types::{Event, EventType, TabRequest};

/// Duration after the last user input until an unlimited search is performed.
pub const TYPING_SEARCH_DELAY: Duration = Duration::from_millis(500);

/// Maximum number of lines for the blocking search while still typing the search regex.
const MAX_SEARCH_WHILE_TYPING: Option<usize> = Some(1000);

/// Maximum number of search terms stored in the history.
const MAX_SEARCH_HISTORY_SIZE: usize = 255;

/// Cooldown between invocations of the bell command.
const BELL_CMD_COOLDOWN: Duration = Duration::from_millis(100);

/// The event processor.
///
/// Stores some state from received events and dispatches actions when they are
/// triggered.
pub struct Processor {
    pub config_monitor: Option<ConfigMonitor>,

    clipboard: Clipboard,
    scheduler: Scheduler,
    initial_window_options: Option<WindowOptions>,
    initial_window_error: Option<Box<dyn Error>>,
    windows: HashMap<WindowId, WindowContext, RandomState>,
    native_window_stages: NativeWindowStageTracker,
    proxy: EventLoopProxy<Event>,
    gl_config: Option<GlutinConfig>,
    #[cfg(unix)]
    global_ipc_options: ParsedOptions,
    cli_options: CliOptions,
    config: Rc<UiConfig>,
    // Lua 回调与模块状态属于当前成功代次，失败重载不能提前释放它。
    lua_generation: Option<config::lua::LuaGeneration>,
    config_source: Option<config::source::ConfigSource>,
    config_reload_worker: ReloadWorker,
    /// The quick (Quake) terminal window, once created.
    quick_terminal: Option<WindowId>,
    /// Whether the quick terminal is currently shown (target state).
    quick_visible: bool,
    /// Renderer-independent quick-terminal motion state.
    quick_motion: crate::motion::Tween,
    quick_motion_clock: crate::motion::MotionClock,
    /// Global hotkey manager, kept alive so its registration stays active.
    global_hotkey: Option<GlobalHotKeyManager>,
    /// Registered quick-terminal toggle hotkey and the persisted spelling shown
    /// in settings. Keeping the full value lets failed replacements restore it.
    quick_hotkey: Option<HotKey>,
    quick_hotkey_combo: String,
    /// Tabs of closed windows kept alive for re-attach (multiplexer-style): their
    /// PTYs never stopped, so `claude` and friends survive the window. LIFO —
    /// an attach request adopts the most recently closed window first.
    detached: Vec<DetachedWindow>,
    /// Canonical state projection observed by CLI clients and subscribers.
    runtime_hub: RuntimeHub,
}

impl Processor {
    /// Create a new event processor.
    pub fn new(
        loaded_config: config::LoadedConfig,
        cli_options: CliOptions,
        event_loop: &EventLoop<Event>,
        native_window_stages: NativeWindowStageTracker,
        runtime_hub: RuntimeHub,
    ) -> Processor {
        let proxy = event_loop.create_proxy();
        let reload_proxy = proxy.clone();
        let config_reload_worker = ReloadWorker::new(move || {
            let event = Event::new(EventType::ConfigReloadReady, None);
            let _ = reload_proxy.send_event(event);
        });
        let scheduler = Scheduler::new(proxy.clone());
        let initial_window_options = Some(cli_options.window_options.clone());

        // Disable all device events, since we don't care about them.
        event_loop.listen_device_events(DeviceEvents::Never);

        // SAFETY: Since this takes a pointer to the winit event loop, it MUST be dropped first,
        // which is done in `loop_exiting`.
        let clipboard = unsafe { Clipboard::new(event_loop.display_handle().unwrap().as_raw()) };

        // Create a config monitor.
        //
        // The monitor watches the config file for changes and reloads it. Pending
        // config changes are processed in the main loop.
        let mut config_monitor = None;
        if loaded_config.live_config_reload() {
            config_monitor =
                ConfigMonitor::new(loaded_config.config_paths.clone(), event_loop.create_proxy());
        }

        let config::LoadedConfig { config, source: config_source, lua_generation } = loaded_config;

        // Register the persisted global quick-terminal toggle hotkey before any
        // window is shown. Invalid hand-edited values fall back to the default.
        let quick_hotkey_combo = crate::display::quick_terminal_hotkey_from_settings(&config);
        let (global_hotkey, quick_hotkey) = Self::init_quick_hotkey(&quick_hotkey_combo);

        Processor {
            initial_window_options,
            initial_window_error: None,
            cli_options,
            proxy,
            scheduler,
            gl_config: None,
            config: Rc::new(config),
            lua_generation,
            config_source,
            config_reload_worker,
            clipboard,
            windows: Default::default(),
            native_window_stages,
            #[cfg(unix)]
            global_ipc_options: Default::default(),
            config_monitor,
            quick_terminal: None,
            quick_visible: false,
            quick_motion: crate::motion::Tween::new(1.0),
            quick_motion_clock: crate::motion::MotionClock::default(),
            global_hotkey,
            quick_hotkey,
            quick_hotkey_combo,
            detached: Vec::new(),
            runtime_hub,
        }
    }

    /// Apply native move/resize stages before handling the next winit event.
    /// The native hook only records these two low-frequency markers; all
    /// window scans and state changes stay on the normal application path.
    fn drain_native_window_stages(&mut self) {
        self.native_window_stages.drain(|event| {
            for window_context in self.windows.values_mut() {
                if window_context.display.window.native_window_handle_id() != Some(event.hwnd) {
                    continue;
                }

                match event.stage {
                    NativeWindowStage::EnterSizeMove => {
                        crate::display::nebula_debug_log("winmove enter_size_move");
                        window_context.display.window.set_native_live_move(true);
                    },
                    NativeWindowStage::ExitSizeMove => {
                        crate::display::nebula_debug_log("winmove exit_size_move");
                        window_context.display.window.set_native_live_move(false);
                        window_context.apply_pending_native_transition();
                    },
                }
                break;
            }
        });
    }

    /// Create initial window and load GL platform.
    ///
    /// This will initialize the OpenGL Api and pick a config that
    /// will be used for the rest of the windows.
    pub fn create_initial_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_options: WindowOptions,
    ) -> Result<(), Box<dyn Error>> {
        // Session restore (tab list + cwds) for a plain launch. An explicit
        // -e/--working-directory means the user asked for something specific:
        // start exactly that instead of yesterday's tabs.
        let plain_launch = window_options.terminal_options.working_directory.is_none()
            && window_options.terminal_options.command().is_none();
        // 恢复行为归设置·高级→会话管（默认开）。关掉只是不回放——快照照写，
        // 工作区导出与崩溃现场诊断都还在。
        //
        // 两类提示分流：恢复成功是**已经结束、没有待办**的事实，走自动消失的
        // toast；断路器那条带着隔离文件路径，用户可能要去把它捞出来，必须留在
        // 消息栏等他自己关掉。
        let mut restored_notice = None;
        let mut blocked_notice = None;
        let restore = if plain_launch && crate::display::restore_session_enabled() {
            match crate::session::load() {
                Some(mut session) if crate::session::should_restore(&session) => {
                    if crate::session::was_crash(&session) {
                        restored_notice = Some(format!(
                            "已从上次异常退出恢复 {} 个标签（进程未正常收尾）。",
                            session.tabs.len()
                        ));
                    }
                    // Count this launch against the crash-loop breaker; the
                    // first successful autosave (1 Hz tick) resets it.
                    crate::session::mark_boot_attempt(&mut session);
                    Some(session)
                },
                // 断路器跳闸：连续三次启动都没活到第一次自动保存。把这份
                // 会话隔离出去再干净启动，否则一秒后的自动保存就会盖掉这份
                // 「一恢复就崩」的唯一现场。
                Some(session) if !session.tabs.is_empty() => {
                    blocked_notice = Some(match crate::session::quarantine() {
                        Some(path) => format!(
                            "连续三次启动失败，已跳过会话恢复；上次的会话保存在 {}。",
                            path.display()
                        ),
                        None => "连续三次启动失败，已跳过会话恢复。".to_owned(),
                    });
                    None
                },
                _ => None,
            }
        } else {
            None
        };
        let boot = restore.map_or(WindowBoot::Fresh, WindowBoot::Restore);

        let mut window_context = WindowContext::initial(
            event_loop,
            self.proxy.clone(),
            self.config.clone(),
            window_options,
            boot,
        )?;

        // 恢复成功：说完就走。断路器：留在消息栏，路径要能被读到。
        if let Some(text) = restored_notice {
            window_context.display.push_toast(text, ToastKind::Success);
        }
        if let Some(text) = blocked_notice {
            window_context.message_buffer.push(Message::new(text, MessageType::Warning));
        }

        self.gl_config = Some(window_context.display.gl_context().config());
        self.windows.insert(window_context.id(), window_context);

        Ok(())
    }

    /// Create a new terminal window.
    pub fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        options: WindowOptions,
    ) -> Result<WindowId, Box<dyn Error>> {
        self.create_window_boot(event_loop, options, WindowBoot::Fresh)
    }

    /// Create a new terminal window with an explicit boot mode (fresh shell
    /// or adopting detached panes).
    fn create_window_boot(
        &mut self,
        event_loop: &ActiveEventLoop,
        options: WindowOptions,
        boot: WindowBoot,
    ) -> Result<WindowId, Box<dyn Error>> {
        let gl_config = self.gl_config.as_ref().unwrap();

        // Override config with CLI/IPC options.
        let mut config_overrides = options.config_overrides();
        #[cfg(unix)]
        config_overrides.extend_from_slice(&self.global_ipc_options);
        let mut config = self.config.clone();
        config = config_overrides.override_config_rc(config);

        let window_context = WindowContext::additional(
            gl_config,
            event_loop,
            self.proxy.clone(),
            config,
            options,
            config_overrides,
            boot,
        )?;

        let id = window_context.id();
        self.windows.insert(id, window_context);

        // Arm the 1 Hz chrome clock / render-gate watchdog right now, before
        // the first frame. `draw()` also (re)schedules it, but `draw()` only
        // runs on a `RedrawRequested`, and a redraw request is gated behind
        // `has_frame && !occluded`. If a startup occlusion misreport or a lost
        // frame callback closes one of those gates before the first draw ever
        // lands, the very watchdog that exists to reopen them
        // (`unstick_render_gates_if_visible`) would never be armed — the window
        // stays visible but frozen, repainting only after a manual
        // minimize/restore (issues #21 and #32). Scheduling here breaks that
        // bootstrap deadlock so recovery always happens within one tick. The
        // interval matches the idle chrome-clock cadence, so the first `draw()`
        // finds the timer already in place and leaves it untouched.
        let clock_timer = TimerId::new(Topic::NebulaClock, id);
        if !self.scheduler.scheduled(clock_timer) {
            let tick = Event::new(EventType::NebulaTick, id);
            self.scheduler.schedule(tick, Duration::from_secs(1), true, clock_timer);
        }

        Ok(id)
    }

    /// A second launch (via the mux socket) asked this resident instance to
    /// surface. Priority: re-attach detached tabs > focus an existing window
    /// > open a fresh one.
    fn handle_attach_request(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(detached) = self.detached.pop() {
            match self.create_window_boot(
                event_loop,
                WindowOptions::default(),
                WindowBoot::Attach(detached),
            ) {
                Ok(_) => return,
                // The panes are gone with the failed boot (their PTYs shut
                // down by DetachedWindow's Drop); still surface SOMETHING.
                Err(err) => error!("Failed to re-attach detached tabs: {err}"),
            }
        }
        if let Some(window_context) = self.windows.values().find(|w| !w.session_exempt) {
            window_context.display.window.focus_window();
            return;
        }
        if self.gl_config.is_some() {
            if let Err(err) = self.create_window(event_loop, WindowOptions::default()) {
                error!("Could not open window on attach request: {err:?}");
            }
        }
    }

    /// Drop a detached pane whose shell exited while its window was closed,
    /// pruning residency entries that have nothing left alive.
    fn reap_detached_pane(&mut self, pane_id: Option<u64>) {
        let Some(pane_id) = pane_id else { return };
        for detached in &mut self.detached {
            detached.reap_pane(pane_id);
        }
        self.detached.retain(|detached| !detached.is_empty());
    }

    /// Show/hide the quick (Quake) terminal with a slide animation, creating it
    /// on first use.
    fn toggle_quick_terminal(&mut self, event_loop: &ActiveEventLoop) {
        // Existing quick terminal: flip the target state and start a slide.
        if let Some(id) = self.quick_terminal {
            if self.windows.contains_key(&id) {
                self.quick_visible = !self.quick_visible;
                let show = self.quick_visible;
                if show {
                    if let Some(wc) = self.windows.get(&id) {
                        // A fully hidden window starts above the edge. Reversing
                        // an active exit keeps its current position.
                        if !self.quick_motion.is_active() {
                            wc.display.window.set_quick_terminal_slide(1.0);
                            self.quick_motion.snap_to(1.0);
                        }
                        wc.display.window.set_visible(true);
                        wc.display.window.focus_window();
                    }
                }
                // Slide-out keeps the window visible until the animation ends.
                self.quick_motion.animate_role(
                    if show { 0.0 } else { 1.0 },
                    if show {
                        crate::motion::MotionRole::Enter
                    } else {
                        crate::motion::MotionRole::Exit
                    },
                    crate::motion::MotionPolicy::Full,
                );
                return;
            }
            // The window was closed by the user; fall through and recreate it.
            self.quick_terminal = None;
        }

        // The shared GL config only exists after the first normal window.
        if self.gl_config.is_none() {
            return;
        }

        match self.create_window(event_loop, WindowOptions::default()) {
            Ok(id) => {
                self.quick_terminal = Some(id);
                self.quick_visible = true;
                if let Some(wc) = self.windows.get_mut(&id) {
                    // Scratch space: the quick terminal never reads or writes
                    // the session file.
                    wc.session_exempt = true;
                    wc.display.window.configure_quick_terminal();
                    wc.display.window.set_quick_terminal_slide(1.0);
                    wc.display.window.focus_window();
                }
                self.quick_motion.snap_to(1.0);
                self.quick_motion.animate_role(
                    0.0,
                    crate::motion::MotionRole::Enter,
                    crate::motion::MotionPolicy::Full,
                );
            },
            Err(err) => error!("Failed to create quick terminal: {err}"),
        }
    }

    /// Advance the quick-terminal slide one frame. Returns `true` while
    /// animating (so the loop keeps polling). Motion Runtime owns timing and
    /// easing; this path only applies the resulting normalized position.
    fn animate_quick_terminal(&mut self) -> bool {
        if !self.quick_motion.is_active() {
            return false;
        }
        let Some(id) = self.quick_terminal else {
            self.quick_motion.snap_to(1.0);
            return false;
        };
        self.quick_motion.step(self.quick_motion_clock.tick());
        let hidden = self.quick_motion.value().clamp(0.0, 1.0);

        if let Some(wc) = self.windows.get(&id) {
            wc.display.window.set_quick_terminal_slide(hidden);
        }

        if !self.quick_motion.is_active() {
            if self.quick_motion.target() >= 1.0 {
                // Slide-out finished: actually hide the window.
                if let Some(wc) = self.windows.get(&id) {
                    wc.display.window.set_visible(false);
                }
            }
            return false;
        }
        true
    }

    /// Run the event loop.
    ///
    /// The result is exit code generate from the loop.
    pub fn run(&mut self, event_loop: EventLoop<Event>) -> Result<(), Box<dyn Error>> {
        let result = event_loop.run_app(self);
        match self.initial_window_error.take() {
            Some(initial_window_error) => Err(initial_window_error),
            _ => result.map_err(Into::into),
        }
    }

    /// Check if an event is irrelevant and can be skipped.
    fn skip_window_event(event: &WindowEvent) -> bool {
        matches!(
            event,
            WindowEvent::KeyboardInput { is_synthetic: true, .. }
                | WindowEvent::ActivationTokenDone { .. }
                | WindowEvent::DoubleTapGesture { .. }
                | WindowEvent::TouchpadPressure { .. }
                | WindowEvent::RotationGesture { .. }
                | WindowEvent::CursorEntered { .. }
                | WindowEvent::PinchGesture { .. }
                | WindowEvent::AxisMotion { .. }
                | WindowEvent::PanGesture { .. }
                | WindowEvent::HoveredFileCancelled
                | WindowEvent::Destroyed
                | WindowEvent::HoveredFile(_)
                | WindowEvent::Moved(_)
        )
    }
}

impl ApplicationHandler<Event> for Processor {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if cause != StartCause::Init || self.cli_options.daemon {
            return;
        }

        if let Some(window_options) = self.initial_window_options.take() {
            if let Err(err) = self.create_initial_window(event_loop, window_options) {
                self.initial_window_error = Some(err);
                event_loop.exit();
                return;
            }
        }

        info!("Initialisation complete");
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // A native stage can precede the winit event it affects (notably DPI
        // changes), so consume it before filtering or routing this event.
        self.drain_native_window_stages();

        if self.config.debug.print_events {
            info!(target: LOG_TARGET_WINIT, "{event:?}");
        }

        // Ignore all events we do not care about.
        if Self::skip_window_event(&event) {
            return;
        }

        let window_context = match self.windows.get_mut(&window_id) {
            Some(window_context) => window_context,
            None => return,
        };

        let is_redraw = matches!(event, WindowEvent::RedrawRequested);

        window_context.handle_event(
            _event_loop,
            &self.proxy,
            &mut self.clipboard,
            &mut self.scheduler,
            WinitEvent::WindowEvent { window_id, event },
        );

        if is_redraw {
            let start = std::time::Instant::now();
            window_context.draw(&mut self.scheduler);
            crate::input::latency::frame_drawn();
            let elapsed = start.elapsed();
            if elapsed.as_millis() >= 8 {
                crate::display::nebula_debug_log(format!("winmove slow_draw {elapsed:?}"));
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        if self.config.debug.print_events {
            info!(target: LOG_TARGET_WINIT, "{event:?}");
        }

        // Handle events which don't mandate the WindowId.
        let tab_id = event.tab_id;
        match (event.payload, event.window_id.as_ref()) {
            (EventType::RuntimeControl(dispatch), _) => {
                self.handle_runtime_control(event_loop, &dispatch)
            },
            // AI-CLI lifecycle events (nebula-hook pipe) route by pane id, so
            // the windows resolve them themselves; the owner claims it.
            (EventType::AiHook(hook), _) => {
                self.route_ai_hook(&hook);
            },
            // Assistant fix results route by pane id the same way.
            (EventType::AiFixReady { pane, seq, fix }, _) => {
                for window_context in self.windows.values_mut() {
                    if window_context.handle_ai_fix(pane, seq, &fix) {
                        break;
                    }
                }
            },
            // WebDAV 同步（spec 003）：网络与 Argon2 派生都在后台 OS 线程
            // 阻塞完成，主循环只发起与收尾——终端渲染不等加密。
            (EventType::NebulaSync { push }, _) => {
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let result = if push { crate::sync::push() } else { crate::sync::pull() };
                    crate::sync::warn_result(&result);
                    let (message, error, history_changed) = match result {
                        Ok(outcome) => (outcome.message, false, outcome.history_changed),
                        Err(err) => (err, true, false),
                    };
                    let _ = proxy.send_event(crate::event::Event::new(
                        EventType::NebulaSyncDone { message, error, history_changed },
                        None,
                    ));
                });
            },
            (EventType::NebulaSyncDone { message, error, history_changed }, _) => {
                for window_context in self.windows.values_mut() {
                    window_context.handle_sync_done(&message, error, history_changed);
                }
            },
            // 远程备份（设置→备份）：打包、Argon2 派生与网络都在后台 OS
            // 线程阻塞完成，主循环只发起与收尾——与 WebDAV 同步同一模型。
            (EventType::NebulaBackupRemote { upload, passphrase, selection }, _) => {
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let result = if upload {
                        crate::encrypted_backup::collect(selection)
                            .and_then(|archive| {
                                crate::encrypted_backup::seal(&archive, &passphrase)
                            })
                            .and_then(|packet| crate::backup_remote::push(&packet))
                    } else {
                        crate::backup_remote::pull_latest().and_then(|(name, packet)| {
                            crate::encrypted_backup::restore(&packet, &passphrase)
                                .map(|()| format!("已从远端恢复 {name}，重启后应用全部设置"))
                        })
                    };
                    crate::backup_remote::warn_result(&result);
                    let (message, error) = match result {
                        Ok(message) => (message, false),
                        Err(err) => (err, true),
                    };
                    let _ = proxy.send_event(crate::event::Event::new(
                        EventType::NebulaBackupRemoteDone { message, error },
                        None,
                    ));
                });
            },
            (EventType::NebulaBackupRemoteDone { message, error }, _) => {
                for window_context in self.windows.values_mut() {
                    window_context.handle_backup_remote_done(&message, error);
                }
            },
            (EventType::LocalProxyScan, Some(window_id)) => {
                let proxy = self.proxy.clone();
                let window_id = *window_id;
                std::thread::spawn(move || {
                    let found = crate::ssh_proxy::scan_local_proxies(&[]);
                    let _ = proxy.send_event(crate::event::Event::new(
                        EventType::LocalProxyScanDone(found),
                        window_id,
                    ));
                });
            },
            (EventType::LocalProxyScanDone(found), Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.display.local_proxy_scan_done(found);
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            (EventType::ProxyTestDone { request_id, outcome, elapsed_ms }, Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.display.proxy_test_done(request_id, outcome, elapsed_ms);
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            (
                EventType::ProviderTestDone { request_id, provider_id, outcome, elapsed_ms },
                Some(window_id),
            ) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.display.provider_test_done(
                        request_id,
                        &provider_id,
                        &outcome,
                        elapsed_ms,
                    );
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            (EventType::QuickTerminalHotkeyChanged { hotkey }, Some(window_id)) => {
                let old = self.quick_hotkey_combo.clone();
                let result = self.apply_quick_terminal_hotkey(&hotkey);
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    match result {
                        Ok(()) => window_context
                            .display
                            .quick_hotkey_registration_done(&hotkey, true, None, &old),
                        Err(err) => window_context.display.quick_hotkey_registration_done(
                            &hotkey,
                            false,
                            Some(&err),
                            &old,
                        ),
                    }
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            (
                EventType::SshTestDone { request_id, destination, ok, message, elapsed_ms },
                Some(window_id),
            ) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.display.ssh_test_done(
                        request_id,
                        &destination,
                        ok,
                        &message,
                        elapsed_ms,
                    );
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            // 连接阶段推进：只更新拥有该 pane 的窗口。事件自带 tab_id，
            // 没有 tab_id 的（不该出现）直接丢弃而不是误绘到别的 pane 上。
            (EventType::SshConnect(stage), Some(window_id)) => {
                if let (Some(window_context), Some(pane)) =
                    (self.windows.get_mut(window_id), tab_id)
                {
                    window_context.ssh_connect_stage(pane, stage);
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            // Toast click: surface the window (and pane) the toast came from.
            // Must be consumed here — the generic Some(window_id) forwarding
            // below would park it in a window's event queue instead.
            (EventType::FocusWindow { pane }, window_id) => {
                let id = window_id.copied();
                let window_context = match id {
                    Some(id) if self.windows.contains_key(&id) => self.windows.get_mut(&id),
                    _ => self.windows.values_mut().next(),
                };
                if let Some(window_context) = window_context {
                    window_context.focus_from_toast(pane);
                }
            },
            // Process IPC config update.
            #[cfg(unix)]
            (EventType::IpcConfig(ipc_config), window_id) => {
                // Try and parse options as toml.
                let mut options = ParsedOptions::from_options(&ipc_config.options);

                // Override IPC config for each window with matching ID.
                for (_, window_context) in self
                    .windows
                    .iter_mut()
                    .filter(|(id, _)| window_id.is_none() || window_id == Some(*id))
                {
                    if ipc_config.reset {
                        window_context.reset_window_config(self.config.clone());
                    } else {
                        window_context.add_window_config(self.config.clone(), &options);
                    }
                }

                // Persist global options for future windows.
                if window_id.is_none() {
                    if ipc_config.reset {
                        self.global_ipc_options.clear();
                    } else {
                        self.global_ipc_options.append(&mut options);
                    }
                }
            },
            // Process IPC config requests.
            #[cfg(unix)]
            (EventType::IpcGetConfig(stream), window_id) => {
                // Get the config for the requested window ID.
                let config = match self.windows.iter().find(|(id, _)| window_id == Some(*id)) {
                    Some((_, window_context)) => window_context.config(),
                    None => &self.global_ipc_options.override_config_rc(self.config.clone()),
                };

                // Convert config to JSON format.
                let config_json = match serde_json::to_string(&config) {
                    Ok(config_json) => config_json,
                    Err(err) => {
                        error!("Failed config serialization: {err}");
                        return;
                    },
                };

                // Send JSON config to the socket.
                if let Ok(mut stream) = stream.try_clone() {
                    ipc::send_reply(&mut stream, SocketReply::GetConfig(config_json));
                }
            },
            (EventType::ConfigReload(path), _) => {
                // Clear config logs from message bar for all terminals.
                for window_context in self.windows.values_mut() {
                    if !window_context.message_buffer.is_empty() {
                        window_context.message_buffer.remove_target(LOG_TARGET_CONFIG);
                        window_context.display.pending_update.dirty = true;
                    }
                }

                match config::source::source_for_path(path, true) {
                    Ok(source) => {
                        self.config_reload_worker.request(source);
                    },
                    Err(error) => error!("Unable to reload configuration: {error}"),
                }
            },
            (EventType::ConfigReloadReady, _) => {
                // 失败结果只产生诊断；当前配置和 Lua 代次保持不变。
                if let Some(result) = self.config_reload_worker.take_latest()
                    && let Ok(mut loaded) = result.loaded
                {
                    self.cli_options.override_config(&mut loaded.config);
                    config::merge_terminal_profiles(&mut loaded.config);
                    self.lua_generation = loaded.lua_generation;
                    self.config_source = loaded.source;
                    self.config = Rc::new(loaded.config);

                    // Restart config monitor if imports changed.
                    if let Some(monitor) = self.config_monitor.take() {
                        let paths = &self.config.config_paths;
                        self.config_monitor = if monitor.needs_restart(paths) {
                            monitor.shutdown();
                            ConfigMonitor::new(paths.clone(), self.proxy.clone())
                        } else {
                            Some(monitor)
                        };
                    }

                    for window_context in self.windows.values_mut() {
                        window_context.update_config(self.config.clone());
                    }
                }
            },
            (EventType::TerminalProfilesChanged, _) => {
                // The imported profile store is deliberately separate from
                // the user's config file. Rebuild the shared config snapshot
                // in-place so every existing window and future tab sees the
                // new profile without a restart.
                let mut config = (*self.config).clone();
                config::merge_terminal_profiles(&mut config);
                self.config = Rc::new(config);
                for window_context in self.windows.values_mut() {
                    window_context.update_config(self.config.clone());
                }
            },
            // Create a new terminal window.
            (EventType::CreateWindow(options), _) => {
                // XXX Ensure that no context is current when creating a new window,
                // otherwise it may lock the backing buffer of the
                // surface of current context when asking
                // e.g. EGL on Wayland to create a new context.
                for window_context in self.windows.values_mut() {
                    window_context.display.make_not_current();
                }

                if self.gl_config.is_none() {
                    // Handle initial window creation in daemon mode.
                    if let Err(err) = self.create_initial_window(event_loop, options) {
                        self.initial_window_error = Some(err);
                        event_loop.exit();
                    }
                } else if let Err(err) = self.create_window(event_loop, options) {
                    error!("Could not open window: {err:?}");
                }
            },
            // Shutdown all windows.
            #[cfg(unix)]
            (EventType::Shutdown, _) => event_loop.exit(),
            // A second launch handed over to this resident instance.
            (EventType::NebulaAttach, _) => self.handle_attach_request(event_loop),
            // Process events affecting all windows.
            (payload, None) => {
                let event = WinitEvent::UserEvent(Event::new(payload, None));
                for window_context in self.windows.values_mut() {
                    window_context.handle_event(
                        event_loop,
                        &self.proxy,
                        &mut self.clipboard,
                        &mut self.scheduler,
                        event.clone(),
                    );
                }
            },
            (EventType::Terminal(TerminalEvent::Wakeup), Some(window_id)) => {
                self.handle_terminal_wakeup(window_id, tab_id);
            },
            (EventType::Terminal(TerminalEvent::Exit), Some(window_id)) => {
                if let Some(pane_id) = tab_id {
                    self.runtime_hub.record_pane_exited(u64::from(*window_id), pane_id);
                }
                // Close the tab whose shell exited; only close the window when
                // it was the last tab (respecting the hold option).
                let close_window = match self.windows.get_mut(window_id) {
                    Some(window_context) if !window_context.display.window.hold => {
                        let close = window_context.close_tab_by_id(tab_id);
                        if !close {
                            window_context.dirty = true;
                            window_context.display.window.request_redraw();
                        }
                        close
                    },
                    Some(_) => return,
                    None => {
                        // A shell exited in a DETACHED pane (its window is
                        // gone): reap it from the residency pool. Once nothing
                        // is left to re-attach, the resident process has no
                        // reason to live.
                        self.reap_detached_pane(tab_id);
                        if self.windows.is_empty()
                            && self.detached.is_empty()
                            && !self.cli_options.daemon
                        {
                            event_loop.exit();
                        }
                        return;
                    },
                };

                if !close_window {
                    return;
                }

                let window_context = match self.windows.remove(window_id) {
                    Some(window_context) => window_context,
                    None => return,
                };

                // Unschedule pending events.
                self.scheduler.unschedule_window(window_context.id());

                // The closed window's Drop writes its final session snapshot;
                // force the surviving windows to reclaim the file on their
                // next autosave tick.
                if !window_context.session_exempt {
                    for window in self.windows.values_mut() {
                        window.mark_session_dirty();
                    }
                }

                // Shutdown if no more terminals are open (and none detached).
                if self.windows.is_empty() && self.detached.is_empty() && !self.cli_options.daemon {
                    // Write ref tests of last window to disk.
                    if self.config.debug.ref_test {
                        window_context.write_ref_test_results();
                    }

                    event_loop.exit();
                }
            },
            // NOTE: This event bypasses batching to minimize input latency.
            (EventType::Frame, Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.display.window.has_frame = true;
                    if window_context.dirty {
                        window_context.display.window.request_redraw();
                    }
                }
            },
            (EventType::NebulaTick, Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    // Agent screen semantics (blocked/working/idle) are a
                    // cheap 1 Hz fallback under exact lifecycle hooks.
                    window_context.refresh_agent_screen_states();
                    // Piggyback session persistence on the 1 Hz chrome clock.
                    window_context.autosave_session();
                    // 渲染门控看门狗:被误报的遮挡/丢失的帧回调在这里解锁
                    // (issue #21"启动后点什么都没反应")。
                    window_context.unstick_render_gates_if_visible();
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
                // 托盘 agent 清单同样搭 1 Hz 时钟：跨窗口聚合，tray::update
                // 内容不变时自去抖。多窗口各自的 tick 都会走到这里，最先到
                // 的那个完成本秒的发布，其余是廉价 no-op。
                let agents = self
                    .windows
                    .values()
                    .flat_map(|window_context| window_context.tray_agents())
                    .collect();
                crate::tray::update(agents);
            },
            (EventType::NebulaResizeSettled, Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.apply_settled_pty_resize();
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            (EventType::SshDeleteUndoExpired, Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.display.expire_ssh_delete_undo();
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            (EventType::SftpUpdated, Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.dirty = true;
                    window_context.display.window.request_redraw();
                }
            },
            (EventType::NebulaTab(request), Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    let close_window = window_context.handle_tab_request(request);
                    if close_window {
                        if let Some(mut closed) = self.windows.remove(window_id) {
                            // A window-level close with live panes = detach
                            // (multiplexer-style): the PTYs keep running in this
                            // resident process, ready for re-attach. Quitting
                            // tab by tab reaches here with zero panes and
                            // falls through to a plain close. 设置→高级 lets
                            // users opt out: with keep_session off, closing
                            // the window kills its shells like a plain
                            // terminal (no resident server).
                            if closed.has_live_panes()
                                && !closed.session_exempt
                                && closed.display.nebula_keep_session
                            {
                                self.detached.push(closed.detach_panes());
                            }
                            // Same reclaim dance as the Exit path above: the
                            // closed window's Drop snapshot must not stick
                            // while other windows live on.
                            if !closed.session_exempt {
                                for window in self.windows.values_mut() {
                                    window.mark_session_dirty();
                                }
                            }
                        }
                        if self.windows.is_empty() && self.detached.is_empty() {
                            event_loop.exit();
                        }
                    } else {
                        window_context.dirty = true;
                        window_context.display.window.request_redraw();
                    }
                }
            },
            (payload, Some(window_id)) => {
                if let Some(window_context) = self.windows.get_mut(window_id) {
                    window_context.handle_event(
                        event_loop,
                        &self.proxy,
                        &mut self.clipboard,
                        &mut self.scheduler,
                        WinitEvent::UserEvent(Event {
                            window_id: Some(*window_id),
                            tab_id,
                            payload,
                        }),
                    );
                }
            },
        };
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // WM_EXITSIZEMOVE may not have a corresponding WindowEvent. Drain it
        // here so the final pending DPI is committed before the loop sleeps.
        self.drain_native_window_stages();

        if self.config.debug.print_events {
            info!(target: LOG_TARGET_WINIT, "About to wait");
        }

        // Poll the global quick-terminal toggle hotkey.
        self.poll_quick_hotkey(event_loop);

        // Advance the quick-terminal slide one frame; `true` = still animating.
        let quick_animating = self.animate_quick_terminal();

        // Dispatch event to all windows.
        for window_context in self.windows.values_mut() {
            window_context.handle_event(
                event_loop,
                &self.proxy,
                &mut self.clipboard,
                &mut self.scheduler,
                WinitEvent::AboutToWait,
            );
        }
        self.flush_quick_hotkey_requests();
        // This is the single projection boundary for GUI, PTY, hook, and CLI
        // changes. RuntimeHub deduplicates identical semantic snapshots.
        self.publish_runtime_snapshot();

        // Update the scheduler after event processing to ensure
        // the event loop deadline is as accurate as possible.
        let control_flow = match self.scheduler.update() {
            Some(instant) => ControlFlow::WaitUntil(instant),
            None => ControlFlow::Wait,
        };
        // While the quick terminal slides, keep the loop hot so the eased
        // position is re-derived every frame instead of parking on Wait.
        event_loop.set_control_flow(if quick_animating { ControlFlow::Poll } else { control_flow });
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if self.config.debug.print_events {
            info!("Exiting the event loop");
        }

        match self.gl_config.take().map(|config| config.display()) {
            #[cfg(not(target_os = "macos"))]
            Some(glutin::display::Display::Egl(display)) => {
                // Ensure that all the windows are dropped, so the destructors for
                // Renderer and contexts ran.
                self.windows.clear();

                // SAFETY: the display is being destroyed after destroying all the
                // windows, thus no attempt to access the EGL state will be made.
                unsafe {
                    display.terminate();
                }
            },
            _ => (),
        }

        // SAFETY: The clipboard must be dropped before the event loop, so use the nop clipboard
        // as a safe placeholder.
        self.clipboard = Clipboard::new_nop();
    }
}
