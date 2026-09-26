//! Pebrel's notification center.
//!
//! One funnel for everything that may deserve the user's attention —
//! terminal bells (Claude Code / Codex ring one when a turn finishes),
//! OSC 9 text notifications, long commands finishing (OSC 133;C/D) — with
//! one policy gate and pluggable delivery. Deliberately small and additive:
//! new sources should become a [`Notification`] variant, new outputs a line
//! in [`deliver`], so AI-CLI-specific hooks can land without rewiring.
//!
//! Delivery on Windows is a real WinRT toast (system tray / notification
//! center), on top of the taskbar flash. Unlike launchers that ask the user to
//! hand-edit hook scripts into each CLI's config — Pebrel needs ZERO user
//! setup: AI CLIs already ring BEL when a turn ends, so the toast fires off
//! that signal out of the box. Toast identity comes from a "Pebrel" AUMID
//! registered under `HKCU\Software\Classes\AppUserModelId` (the documented
//! registry route for unpackaged apps — no COM, no Start-menu shortcut, no
//! installer), so banners read "Pebrel" instead of "Windows PowerShell".
//!
//! Delivery discipline: the toast RPC runs on a throwaway thread so a slow or
//! faulty notification stack can never stall the winit event loop — and a
//! panic there kills that thread, not the terminal. Notifications are
//! best-effort by contract: every failure degrades to a log line, never to a
//! crash. A small global throttle keeps a bell-happy background job from
//! flooding the Action Center.

#[cfg(any(feature = "gpui-shell", test))]
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(feature = "legacy-shell")]
use winit::event_loop::EventLoopProxy;

#[cfg(feature = "legacy-shell")]
use crate::display::window::Window;
#[cfg(feature = "legacy-shell")]
use crate::event::{Event, EventType};

/// Event-loop proxy for toast click handlers. A click lands on a WinRT
/// threadpool thread, which can only talk to the app through user events.
/// Set once at boot, before the first toast can exist.
#[cfg(feature = "legacy-shell")]
static PROXY: OnceLock<EventLoopProxy<Event>> = OnceLock::new();

/// Install the proxy used by toast activation (click-to-focus).
#[cfg(feature = "legacy-shell")]
pub fn init_proxy(proxy: EventLoopProxy<Event>) {
    let _ = PROXY.set(proxy);
}

use crate::platform::notifications::ToastActivation;
pub use crate::platform::notifications::notify_test;

#[cfg(feature = "gpui-shell")]
static GPUI_ACTIVATION: OnceLock<std::sync::mpsc::Sender<crate::gpui_shell::GpuiShellEvent>> =
    OnceLock::new();

#[cfg(feature = "gpui-shell")]
pub(crate) fn init_gpui_activation(
    sender: std::sync::mpsc::Sender<crate::gpui_shell::GpuiShellEvent>,
) {
    let _ = GPUI_ACTIVATION.set(sender);
}

#[cfg(feature = "gpui-shell")]
fn gpui_activation(
    sender: std::sync::mpsc::Sender<crate::gpui_shell::GpuiShellEvent>,
    pane_id: Option<u64>,
) -> ToastActivation {
    Arc::new(move || {
        let _ = sender.send(crate::gpui_shell::GpuiShellEvent::NotificationFocus(pane_id));
    })
}

fn application_activation(pane_id: Option<u64>) -> Option<ToastActivation> {
    #[cfg(feature = "gpui-shell")]
    {
        GPUI_ACTIVATION.get().map(|sender| gpui_activation(sender.clone(), pane_id))
    }
    #[cfg(not(feature = "gpui-shell"))]
    {
        let _ = pane_id;
        None
    }
}

/// Something that happened in a pane which may deserve attention.
#[derive(Debug, Clone)]
pub enum Notification {
    /// BEL from the shell/TUI. AI CLIs ring this when a turn completes, so
    /// it is the primary "claude/codex finished" signal. Carries the tracked
    /// program name (e.g. "claude", "codex") when one is running, so the toast
    /// can say who finished.
    Bell { program: Option<String> },
    /// A tracked command finished (OSC 133;C started it, 133;D ended it).
    CommandDone { duration: Duration, program: Option<String> },
    /// Shell-reported nonzero exit status, including short failed commands.
    CommandFailed { duration: Duration, program: Option<String>, exit_code: i32 },
    /// Free-text notification from a program (OSC 9, iTerm style). Claude
    /// Code emits these (with the turn's actual message) when its notif
    /// channel is `iterm2`/`iterm2_with_bell`. Carries the tracked program
    /// name so the toast is titled "claude" instead of "Pebrel".
    Text { body: String, program: Option<String> },
    /// Typed AI-CLI turn event delivered through the `pebrel-hook` pipe
    /// (claude hooks / codex notify — see `ai_hook`). `attention` means the
    /// CLI needs the user NOW (permission prompt / idle reminder) rather
    /// than "turn finished".
    AiTurn { program: String, message: Option<String>, attention: bool },
    /// The provider stopped without a confirmed successful answer.
    AiTurnIssue { program: String, message: Option<String>, outcome: crate::ai_hook::AiTurnOutcome },
}

/// Longest notification body any shell may render.
///
/// A toast/banner is a glance layer, not a reader. Codex's turn-complete
/// notification carries the model's `last-assistant-message` (bounded only at
/// 4 000 chars in `ai_hook`), and an OSC 9 body is unbounded; rendered at a
/// fixed width that becomes a card taller than the window. Anchored to the
/// bottom-right it then overflows off the top, taking its close button out of
/// reach, and sits there for the whole banner TTL while covering whatever is
/// underneath (including a split pane). Only the preview is bounded here — the
/// full text stays in the log and in the terminal's answer reader.
pub(crate) const TOAST_BODY_MAX_CHARS: usize = 160;

/// Make a single-line preview of at most [`TOAST_BODY_MAX_CHARS`] characters.
/// Line breaks and other whitespace become spaces; a short multi-line body
/// must not create an arbitrarily tall card. Read only one character beyond
/// the limit, and reserve the last character for `…` when text was omitted.
/// Raw text remains available separately for logging and recovery.
pub(crate) fn clamp_toast_body(body: &str) -> String {
    let mut chars = body.chars().map(|ch| if ch.is_whitespace() { ' ' } else { ch });
    let mut preview = String::with_capacity(body.len().min(TOAST_BODY_MAX_CHARS * 4));
    preview.extend(chars.by_ref().take(TOAST_BODY_MAX_CHARS));
    if chars.next().is_some() {
        preview.pop();
        preview.push('…');
    }
    preview
}

impl Notification {
    pub(crate) fn command_finished(
        program: Option<String>,
        duration: Duration,
        exit_code: Option<i32>,
        hook_seen: bool,
    ) -> Option<Self> {
        if let Some(exit_code) = exit_code.filter(|code| *code != 0) {
            return Some(Self::CommandFailed { program, duration, exit_code });
        }
        (!hook_seen && duration >= COMMAND_NOTIFY_MIN)
            .then_some(Self::CommandDone { program, duration })
    }

    pub(crate) fn from_ai_hook(
        event: &crate::ai_hook::AiHookEvent,
        message: Option<String>,
        attention: bool,
    ) -> Option<Self> {
        use crate::ai_hook::AiTurnOutcome;
        if event.kind == crate::ai_hook::AiHookKind::TurnDone {
            if event.active_background_tasks() > 0 {
                return None;
            }
            match event.turn_outcome {
                AiTurnOutcome::Cancelled => return None,
                AiTurnOutcome::Failed | AiTurnOutcome::Incomplete => {
                    return Some(Self::AiTurnIssue {
                        program: event.source.clone(),
                        message,
                        outcome: event.turn_outcome,
                    });
                },
                AiTurnOutcome::Unknown | AiTurnOutcome::Unspecified if event.source == "pi" => {
                    return Some(Self::AiTurnIssue {
                        program: event.source.clone(),
                        message: None,
                        outcome: AiTurnOutcome::Unknown,
                    });
                },
                _ => {},
            }
        }
        Some(Self::AiTurn { program: event.source.clone(), message, attention })
    }

    pub(crate) fn is_failure(&self) -> bool {
        matches!(
            self,
            Self::CommandFailed { .. }
                | Self::AiTurnIssue {
                    outcome: crate::ai_hook::AiTurnOutcome::Failed
                        | crate::ai_hook::AiTurnOutcome::Incomplete,
                    ..
                }
        )
    }

    /// Source classification only. In-app preferences must not silence native
    /// notifications or infer an AI source from arbitrary message text.
    pub(crate) fn is_ai(&self) -> bool {
        let program = match self {
            Self::AiTurn { .. } | Self::AiTurnIssue { .. } => return true,
            Self::Bell { program }
            | Self::CommandDone { program, .. }
            | Self::CommandFailed { program, .. }
            | Self::Text { program, .. } => program.as_deref(),
        };
        program.is_some_and(|program| crate::ai_agents::AgentKind::parse(program).is_some())
    }

    /// Toast title + body. Title names the source ("Pebrel" or the program);
    /// body carries the human detail, bounded to a glanceable length.
    pub(crate) fn toast_text(&self) -> (String, String) {
        let (title, body) = self.raw_toast_text();
        (title, clamp_toast_body(&body))
    }

    pub(crate) fn raw_toast_text(&self) -> (String, String) {
        match self {
            Self::Bell { program } => match program {
                Some(p) => (
                    p.clone(),
                    notification_language().text(crate::i18n::Message::NotificationBell).to_owned(),
                ),
                None => (crate::brand::NAME.to_owned(), "终端响铃".to_owned()),
            },
            Self::CommandDone { duration, program } => (
                program.clone().unwrap_or_else(|| crate::brand::NAME.to_owned()),
                notification_language().format(
                    crate::i18n::Message::NotificationCommandFinished,
                    &[("seconds", &duration.as_secs().to_string())],
                ),
            ),
            Self::CommandFailed { duration, program, exit_code } => (
                program.clone().unwrap_or_else(|| crate::brand::NAME.to_owned()),
                notification_language().format(
                    crate::i18n::Message::NotificationCommandFailed,
                    &[
                        ("seconds", &duration.as_secs().to_string()),
                        ("code", &exit_code.to_string()),
                    ],
                ),
            ),
            Self::Text { body, program } => match program {
                Some(p) => (p.clone(), body.clone()),
                None => (crate::brand::NAME.to_owned(), body.clone()),
            },
            Self::AiTurn { program, message, attention } => {
                let body = message.clone().unwrap_or_else(|| {
                    if *attention {
                        "需要你的确认或输入".to_owned()
                    } else {
                        "回合完成，等待下一条指令".to_owned()
                    }
                });
                (program.clone(), body)
            },
            Self::AiTurnIssue { program, message, outcome } => (
                program.clone(),
                turn_issue_text(*outcome, message.as_deref(), notification_language()),
            ),
        }
    }

    pub(crate) fn is_attention(&self) -> bool {
        matches!(self, Self::AiTurn { attention: true, .. })
    }
}

fn notification_language() -> crate::i18n::UiLanguage {
    crate::i18n::LanguagePreference::from(nebula_settings::RuntimeSettings::load().language)
        .resolved()
}

fn turn_issue_text(
    outcome: crate::ai_hook::AiTurnOutcome,
    message: Option<&str>,
    language: crate::i18n::UiLanguage,
) -> String {
    use crate::ai_hook::AiTurnOutcome;
    use crate::i18n::Message;
    match outcome {
        AiTurnOutcome::Failed => match message.filter(|message| !message.trim().is_empty()) {
            Some(error) => {
                language.format(Message::NotificationTurnFailedReason, &[("error", error)])
            },
            None => language.text(Message::NotificationTurnFailed).to_owned(),
        },
        AiTurnOutcome::Incomplete => language.text(Message::NotificationTurnIncomplete).to_owned(),
        _ => language.text(Message::NotificationTurnEnded).to_owned(),
    }
}

/// Commands shorter than this never notify: quick `ls`-style commands would
/// otherwise flash the taskbar all day.
pub const COMMAND_NOTIFY_MIN: Duration = Duration::from_secs(10);

/// Minimum spacing between system toasts. Anything inside the window still
/// flashes the taskbar (cheap, silent, coalesced by the shell) but skips the
/// toast, so a build script ringing BEL in a loop cannot flood Action Center.
const TOAST_THROTTLE: Duration = Duration::from_secs(3);

/// Repeated failures from an automatic retry need one reminder, not a sound
/// for every attempt. Different errors and source panes remain independent.
const FAILURE_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct PaneFailureThrottle {
    recent: Vec<RecentFailure>,
}

struct RecentFailure {
    pane: u64,
    program: String,
    message: Option<String>,
    outcome: crate::ai_hook::AiTurnOutcome,
    shown_at: Instant,
}

impl PaneFailureThrottle {
    pub(crate) const fn new() -> Self {
        Self { recent: Vec::new() }
    }

    pub(crate) fn accepts(
        &mut self,
        pane: u64,
        notification: &Notification,
        delivering: bool,
        now: Instant,
    ) -> bool {
        self.recent.retain(|last| now.saturating_duration_since(last.shown_at) < FAILURE_COOLDOWN);
        // Recovery ends the old failure episode. A subsequent failure needs
        // its own notice even if it reports the same provider error.
        if matches!(notification, Notification::AiTurn { attention: false, .. }) {
            self.recent.retain(|last| last.pane != pane);
        }
        if !delivering {
            return false;
        }
        if !notification.is_failure() {
            return true;
        }
        let Notification::AiTurnIssue { program, message, outcome } = notification else {
            return true;
        };
        if self.recent.iter().any(|last| {
            last.pane == pane
                && last.program == *program
                && last.message == *message
                && last.outcome == *outcome
        }) {
            return false;
        }
        self.recent.push(RecentFailure {
            pane,
            program: program.clone(),
            message: message.clone(),
            outcome: *outcome,
            shown_at: now,
        });
        true
    }
}

#[cfg(any(feature = "gpui-shell", test))]
#[derive(Default)]
struct PaneNotificationThrottle {
    recent: HashMap<(u64, bool, Option<u64>), Instant>,
}

#[cfg(any(feature = "gpui-shell", test))]
impl PaneNotificationThrottle {
    fn accepts(&mut self, pane_id: u64, attention: bool, now: Instant) -> bool {
        self.accepts_request(pane_id, attention, None, now)
    }

    fn accepts_request(
        &mut self,
        pane_id: u64,
        attention: bool,
        request: Option<u64>,
        now: Instant,
    ) -> bool {
        self.recent.retain(|_, last| now.saturating_duration_since(*last) < TOAST_THROTTLE);
        if self.recent.contains_key(&(pane_id, attention, request)) {
            return false;
        }
        self.recent.insert((pane_id, attention, request), now);
        true
    }
}

#[cfg(feature = "gpui-shell")]
pub(crate) fn deliver_gpui(notification: &Notification, pane_id: u64) {
    deliver_gpui_with_choices(notification, pane_id, None);
}

#[cfg(feature = "gpui-shell")]
pub(crate) fn deliver_gpui_with_choices(
    notification: &Notification,
    pane_id: u64,
    confirmation: Option<(u64, Vec<String>)>,
) {
    static THROTTLE: OnceLock<Mutex<PaneNotificationThrottle>> = OnceLock::new();
    let accepted = THROTTLE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .accepts_request(
            pane_id,
            notification.is_attention(),
            confirmation.as_ref().map(|c| c.0),
            Instant::now(),
        );
    if !accepted {
        log::debug!("notify: toast suppressed for pane={pane_id}: {notification:?}");
        return;
    }
    let (title, body) = notification.toast_text();
    log::debug!("notify: system toast source for pane={pane_id}: {notification:?}");
    let actions = confirmation
        .and_then(|(request_id, labels)| {
            let sender = GPUI_ACTIVATION.get()?.clone();
            Some(
                labels
                    .into_iter()
                    .enumerate()
                    .map(|(choice, label)| {
                        let sender = sender.clone();
                        crate::platform::notifications::ToastAction {
                            label: label.chars().take(80).collect(),
                            activate: Arc::new(move || {
                                let _ = sender.send(
                                    crate::gpui_shell::GpuiShellEvent::NotificationChoice {
                                        pane_id,
                                        request_id,
                                        choice,
                                    },
                                );
                            }),
                        }
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    spawn_actionable_toast(title, body, application_activation(Some(pane_id)), actions);
}

/// Deliver `notification` for a window that is currently unfocused.
///
/// Policy lives at the call sites: they only call this when the window is
/// NOT focused (a focused user already sees the pane; the visual bell covers
/// that case). Delivery is taskbar attention + a real system toast (which
/// carries its own sound), the native Windows notification-center channel.
///
/// `pane` names the pane the event came from, when known: clicking the toast
/// then focuses the window AND surfaces that pane's tab (mac-style).
#[cfg(feature = "legacy-shell")]
pub fn deliver(window: &Window, notification: &Notification, pane: Option<u64>) {
    // Taskbar flash / attention request (winit wraps FlashWindowEx). Always
    // fires: it is idempotent, silent, and the shell coalesces repeats.
    window.set_urgent(true);

    if throttled() {
        log::debug!("notify: toast suppressed by throttle: {notification:?}");
        return;
    }

    let (title, body) = notification.toast_text();
    let window_id = window.id();
    let activation = PROXY.get().map(|proxy| {
        let proxy = proxy.clone();
        Arc::new(move || {
            let _ = proxy.send_event(Event::new(EventType::FocusWindow { pane }, window_id));
        }) as ToastActivation
    });
    log::debug!("notify: toast source: {notification:?}");
    // Fire-and-forget worker: the WinRT show() is a cross-process RPC (can
    // take tens of ms — an eternity for the event loop), and notifications
    // must never be able to take the terminal down with them.
    spawn_toast(title, body, activation);
}

/// Global toast rate limit. Returns true when this one should be dropped.
#[cfg(feature = "legacy-shell")]
fn throttled() -> bool {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    // A poisoned lock only means some thread panicked mid-check; the state is
    // a plain Option, safe to keep using.
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    match *last {
        Some(at) if at.elapsed() < TOAST_THROTTLE => true,
        _ => {
            *last = Some(Instant::now());
            false
        },
    }
}

/// Raise a native system toast. Best-effort: any failure is logged and
/// swallowed (the taskbar flash already fired, so the user is not left with
/// nothing). Native delivery runs on a worker thread, never on the event loop.
pub(crate) fn toast(title: &str, body: &str) {
    spawn_toast(title.to_owned(), body.to_owned(), application_activation(None));
}

fn spawn_toast(title: String, body: String, activation: Option<ToastActivation>) {
    spawn_actionable_toast(title, body, activation, Vec::new());
}

fn spawn_actionable_toast(
    title: String,
    body: String,
    activation: Option<ToastActivation>,
    actions: Vec<crate::platform::notifications::ToastAction>,
) {
    crate::platform::notifications::dispatch(title, body, activation, actions);
}

#[cfg(test)]
mod delivery_tests {
    use super::*;

    fn pi_result(reason: Option<&str>, message: Option<&str>) -> crate::ai_hook::AiHookEvent {
        let payload = serde_json::json!({
            "kind": "done", "stop_reason": reason, "message": message,
        });
        let envelope = format!("nebula-hook/1 source=pi pane=12\n{payload}");
        crate::ai_hook::parse_remote_envelope(envelope.as_bytes(), Some(12)).unwrap()
    }

    #[test]
    fn short_failures_notify_while_success_and_hook_completion_remain_quiet() {
        for hook in [false, true] {
            let failed = Notification::command_finished(
                Some("cargo".into()),
                Duration::from_millis(50),
                Some(1),
                hook,
            )
            .unwrap();
            assert!(failed.is_failure());
            assert!(!failed.is_attention());
            assert!(
                Notification::command_finished(
                    Some("cargo".into()),
                    Duration::from_millis(50),
                    Some(0),
                    hook
                )
                .is_none()
            );
        }
        assert!(
            Notification::command_finished(
                Some("codex".into()),
                Duration::from_secs(60),
                Some(0),
                true
            )
            .is_none()
        );
        assert!(
            Notification::command_finished(
                Some("cargo".into()),
                Duration::from_secs(60),
                Some(0),
                false
            )
            .is_some()
        );
    }

    #[test]
    fn successive_questions_have_independent_native_notification_identity() {
        let mut throttle = PaneNotificationThrottle::default();
        let now = Instant::now();
        assert!(throttle.accepts_request(42, true, Some(100), now));
        assert!(!throttle.accepts_request(42, true, Some(100), now));
        assert!(throttle.accepts_request(42, true, Some(101), now));
    }

    #[test]
    fn unfinished_background_tasks_suppress_every_terminal_turn_result() {
        for reason in ["stop", "error", "length", "aborted"] {
            let payload = serde_json::json!({
                "kind": "done", "stop_reason": reason,
                "background_tasks": [{"type": "local_bash", "status": "running"}],
            });
            let wire = format!("nebula-hook/1 source=pi pane=12\n{payload}");
            let event = crate::ai_hook::parse_remote_envelope(wire.as_bytes(), Some(12)).unwrap();
            assert_eq!(event.active_background_tasks(), 1);
            assert!(Notification::from_ai_hook(&event, None, false).is_none(), "{reason}");
        }
    }

    #[test]
    fn pi_error_and_incomplete_results_are_not_completion_or_permission_notices() {
        use crate::ai_hook::AiTurnOutcome;
        for (reason, expected) in
            [("error", AiTurnOutcome::Failed), ("length", AiTurnOutcome::Incomplete)]
        {
            let event = pi_result(Some(reason), Some("Our servers are currently overloaded."));
            assert_eq!(event.turn_outcome, expected);
            assert_eq!(event.kind, crate::ai_hook::AiHookKind::TurnDone);
            // A stale screen permission prompt must not override a typed error.
            let note = Notification::from_ai_hook(&event, event.message.clone(), true).unwrap();
            assert!(note.is_failure());
            assert!(note.is_ai());
            assert!(!note.is_attention());
            assert!(
                matches!(note, Notification::AiTurnIssue { outcome, .. } if outcome == expected)
            );
        }
        assert_eq!(
            turn_issue_text(
                AiTurnOutcome::Failed,
                Some("overloaded"),
                crate::i18n::UiLanguage::ZhCn
            ),
            "请求失败：overloaded"
        );
        assert_eq!(
            turn_issue_text(AiTurnOutcome::Failed, None, crate::i18n::UiLanguage::EnUs),
            "Request failed. Check the terminal for details."
        );
    }

    #[test]
    fn cancellation_is_silent_and_legacy_pi_outcomes_remain_unconfirmed() {
        let cancelled = pi_result(Some("aborted"), None);
        assert!(Notification::from_ai_hook(&cancelled, None, false).is_none());
        let unknown = pi_result(None, None);
        let note = Notification::from_ai_hook(&unknown, None, false).unwrap();
        assert!(matches!(
            note,
            Notification::AiTurnIssue { outcome: crate::ai_hook::AiTurnOutcome::Unknown, .. }
        ));
        assert!(!note.is_failure());
        assert!(!note.is_attention());
        let success = pi_result(Some("stop"), None);
        assert!(matches!(
            Notification::from_ai_hook(&success, None, false),
            Some(Notification::AiTurn { attention: false, .. })
        ));
    }

    #[test]
    fn repeated_failures_share_a_fixed_cooldown_without_hiding_recovery_or_new_errors() {
        let event = pi_result(Some("error"), Some("overload"));
        let failure = Notification::from_ai_hook(&event, event.message.clone(), false).unwrap();
        let now = Instant::now();
        let mut gate = PaneFailureThrottle::new();
        assert!(gate.accepts(1, &failure, true, now));
        assert!(gate.accepts(2, &failure, true, now));
        assert!(!gate.accepts(1, &failure, true, now + Duration::from_secs(6)));
        let success =
            Notification::from_ai_hook(&pi_result(Some("stop"), None), None, false).unwrap();
        let attention =
            Notification::AiTurn { program: "pi".into(), message: None, attention: true };
        assert!(gate.accepts(1, &attention, true, now + Duration::from_secs(7)));
        assert!(!gate.accepts(1, &failure, true, now + Duration::from_secs(8)));
        let other = pi_result(Some("error"), Some("authentication failed"));
        let other = Notification::from_ai_hook(&other, other.message.clone(), false).unwrap();
        assert!(gate.accepts(1, &other, true, now + Duration::from_secs(9)));
        assert!(!gate.accepts(1, &failure, true, now + Duration::from_secs(29)));
        assert!(gate.accepts(1, &failure, true, now + FAILURE_COOLDOWN));
        assert!(
            !gate.accepts(1, &success, false, now + Duration::from_secs(31)),
            "visible recovery still ends the failure episode"
        );
        assert!(gate.accepts(1, &failure, true, now + Duration::from_secs(32)));
    }

    #[test]
    fn incomplete_and_failed_without_details_are_distinct_notices() {
        let mut gate = PaneFailureThrottle::new();
        let now = Instant::now();
        for reason in ["length", "error"] {
            let event = pi_result(Some(reason), None);
            let note = Notification::from_ai_hook(&event, None, false).unwrap();
            assert!(gate.accepts(1, &note, true, now));
            assert!(!gate.accepts(1, &note, true, now));
        }
    }

    #[test]
    fn pi_outcomes_do_not_turn_failure_or_cancellation_into_success() {
        use crate::ai_hook::AiTurnOutcome;
        for (reason, expected) in [
            ("error", Some(AiTurnOutcome::Failed)),
            ("aborted", None),
            ("unknown", Some(AiTurnOutcome::Unknown)),
            ("future", Some(AiTurnOutcome::Unknown)),
            ("length", Some(AiTurnOutcome::Incomplete)),
            ("toolUse", Some(AiTurnOutcome::Incomplete)),
        ] {
            let event = pi_result(Some(reason), None);
            let notification = Notification::from_ai_hook(&event, None, false);
            match (notification, expected) {
                (None, None) => {},
                (Some(Notification::AiTurnIssue { outcome, .. }), Some(expected)) => {
                    assert_eq!(outcome, expected);
                },
                result => panic!("unexpected {reason} result: {result:?}"),
            }
        }
        for reason in [None, Some("stop")] {
            let mut payload = serde_json::json!({"kind": "done"});
            if let Some(reason) = reason {
                payload["stop_reason"] = reason.into();
            }
            let wire = format!("nebula-hook/1 source=pi\n{payload}");
            let event = crate::ai_hook::parse_remote_envelope(wire.as_bytes(), Some(1)).unwrap();
            let note = Notification::from_ai_hook(&event, None, false).unwrap();
            assert_eq!(matches!(note, Notification::AiTurn { .. }), reason == Some("stop"));
        }
    }

    #[test]
    fn every_registered_ai_is_recognized_for_bell_text_and_command_events() {
        for agent in crate::ai_agents::AgentKind::ALL {
            let program = Some(agent.slug().to_owned());
            assert!(Notification::Bell { program: program.clone() }.is_ai());
            assert!(
                Notification::Text { body: "done".to_owned(), program: program.clone() }.is_ai()
            );
            assert!(
                Notification::CommandDone { duration: Duration::from_secs(12), program }.is_ai()
            );
        }
    }

    #[test]
    fn typed_ai_events_and_executable_aliases_keep_their_source_identity() {
        for attention in [false, true] {
            assert!(
                Notification::AiTurn {
                    program: "custom-agent".to_owned(),
                    message: None,
                    attention,
                }
                .is_ai()
            );
        }
        for program in [r"C:\tools\CODEX.EXE", "/usr/bin/claude-code", "gemini-cli"] {
            assert!(Notification::Bell { program: Some(program.to_owned()) }.is_ai());
        }
    }

    #[test]
    fn ordinary_notifications_are_not_classified_from_their_text() {
        for program in [None, Some("cargo".to_owned()), Some("powershell".to_owned())] {
            assert!(!Notification::Bell { program: program.clone() }.is_ai());
            assert!(
                !Notification::Text {
                    body: "codex claude permission required".to_owned(),
                    program: program.clone(),
                }
                .is_ai()
            );
            assert!(
                !Notification::CommandDone { duration: Duration::from_secs(12), program }.is_ai()
            );
        }
    }

    #[test]
    fn pane_throttle_does_not_suppress_another_pane_or_attention() {
        let now = Instant::now();
        let mut throttle = PaneNotificationThrottle::default();
        assert!(throttle.accepts(1, false, now));
        assert!(throttle.accepts(2, false, now));
        assert!(!throttle.accepts(1, false, now));
        assert!(throttle.accepts(1, true, now));
        assert!(!throttle.accepts(1, true, now));
        assert!(throttle.accepts(2, true, now));
    }

    #[test]
    fn pane_throttle_expires_without_extending_on_repeats() {
        let now = Instant::now();
        let mut throttle = PaneNotificationThrottle::default();
        assert!(throttle.accepts(1, false, now));
        assert!(!throttle.accepts(1, false, now + Duration::from_secs(2)));
        assert!(throttle.accepts(1, false, now + TOAST_THROTTLE));
        assert_eq!(throttle.recent.len(), 1);
    }

    #[test]
    fn attention_importance_is_independent_of_completion_text() {
        let done =
            Notification::AiTurn { program: "codex".to_owned(), message: None, attention: false };
        let waiting = Notification::AiTurn {
            program: "claude".to_owned(),
            message: Some("Confirm command".to_owned()),
            attention: true,
        };
        assert!(!done.is_attention());
        assert!(waiting.is_attention());
        assert_eq!(waiting.toast_text(), ("claude".to_owned(), "Confirm command".to_owned()));
        assert!(!Notification::Bell { program: None }.is_attention());
        assert!(
            !Notification::Text { body: "permission".to_owned(), program: None }.is_attention()
        );
    }

    /// 回归锁：一篇很长的模型回答不能变成一张撑破窗口的通知卡。正文只保留
    /// 一眼能读的预览，其余在日志和终端「原文」里。
    #[test]
    fn oversized_notification_bodies_are_clamped_to_a_glanceable_preview() {
        let long = "回答正文。".repeat(1_000);
        let done = Notification::AiTurn {
            program: "codex".to_owned(),
            message: Some(long.clone()),
            attention: false,
        };
        let (title, body) = done.toast_text();
        assert_eq!(title, "codex");
        assert!(body.chars().count() <= TOAST_BODY_MAX_CHARS, "正文必须被收短: {}", body.len());
        assert!(body.ends_with('…'), "被截断的正文要以省略号收尾");
        assert!(long.starts_with(body.trim_end_matches('…')), "保留的是原文开头");

        // OSC 9 原文同样无界，走 Text 分支也要收短。
        let (_, osc) =
            Notification::Text { body: long, program: Some("claude".to_owned()) }.toast_text();
        assert!(osc.chars().count() <= TOAST_BODY_MAX_CHARS);

        // 幂等：已经收短过的正文再收一次不会多出第二个省略号。
        let once = clamp_toast_body(&"x".repeat(1_000));
        assert_eq!(clamp_toast_body(&once), once);
    }

    #[test]
    fn notification_preview_bounds_line_breaks_and_preserves_raw_text() {
        let source = format!("{}done", "x\n".repeat(60));
        assert!(source.chars().count() < TOAST_BODY_MAX_CHARS);
        let notification = Notification::Text { body: source.clone(), program: None };
        let (_, preview) = notification.toast_text();
        assert_eq!(preview.lines().count(), 1);
        assert_eq!(preview, source.replace('\n', " "));
        assert_eq!(notification.raw_toast_text().1, source);
        assert_eq!(clamp_toast_body("a\r\nb\tc\u{2028}d\u{2029}e"), "a  b c d e");
    }

    #[test]
    fn notification_preview_preserves_short_text_and_unicode_boundaries() {
        for count in [0, 1, TOAST_BODY_MAX_CHARS - 1, TOAST_BODY_MAX_CHARS] {
            let source = "界".repeat(count);
            assert_eq!(clamp_toast_body(&source), source);
        }
        let source = "😀".repeat(TOAST_BODY_MAX_CHARS + 1);
        let preview = clamp_toast_body(&source);
        assert_eq!(preview, format!("{}…", "😀".repeat(TOAST_BODY_MAX_CHARS - 1)));
        assert_eq!(clamp_toast_body(&preview), preview);
    }

    #[cfg(feature = "gpui-shell")]
    #[test]
    fn activation_preserves_exact_pane_and_generic_application_targets() {
        use crate::gpui_shell::GpuiShellEvent;

        let (sender, receiver) = std::sync::mpsc::channel();
        let pane_activation = gpui_activation(sender.clone(), Some(42));
        let generic_activation = gpui_activation(sender, None);
        pane_activation();
        generic_activation();
        assert!(matches!(receiver.try_recv(), Ok(GpuiShellEvent::NotificationFocus(Some(42)))));
        assert!(matches!(receiver.try_recv(), Ok(GpuiShellEvent::NotificationFocus(None))));
        drop(receiver);
        pane_activation();
    }
}
