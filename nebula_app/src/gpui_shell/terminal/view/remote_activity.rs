//! Project authenticated active-pane observations into the shared Agent lifecycle.
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui::{Context, EventEmitter as _};

use super::{TerminalView, TerminalViewEvent};
use crate::ai_agents::{AgentKind, AgentStatusSource};
use crate::ssh_session::TmuxActivity;

/// How long an Agent identified through an unobserved multiplexer may be
/// absent from the screen before its wrapper is restored.
pub(super) const PASSTHROUGH_RELEASE: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(super) struct RemoteActivity {
    pub context: Option<String>,
    pub sample_at: Option<Instant>,
    title_sample_at: Option<Instant>,
    /// The foreground program replaced by a screen identity while a
    /// multiplexer client owned the PTY without pane-scoped observations.
    pub passthrough: Option<Option<String>>,
    pub passthrough_missing: Option<Instant>,
}

impl TerminalView {
    pub(super) fn sync_remote_activity(&mut self, cx: &mut Context<Self>) -> Option<TmuxActivity> {
        if self.ssh_destination.is_none() {
            return None;
        }
        let observation = self.session.as_ref().and_then(|session| {
            session.tmux_activity.lock().ok()?.clone().filter(TmuxActivity::current)
        });
        let context = observation.as_ref().map(|value| value.context.clone());
        if self.remote_activity.context != context {
            // A different pane, foreground job or SSH generation cannot inherit
            // the previous task, confirmation, answer or ordered Hook owner.
            self.clear_foreground_agent_state(cx);
            self.title = "shell".into();
            self.last_command_failed = false;
            self.remote_activity.context = context;
            self.remote_activity.sample_at = None;
            self.remote_activity.title_sample_at = None;
            if let Some(session) = &self.session {
                let mut term = session.term.lock();
                term.end_program_status_activity();
                term.acknowledge_program_status_results();
            }
            if let Some(program) = observation.as_ref().and_then(TmuxActivity::program) {
                if AgentKind::parse(&program).is_some() {
                    self.agent_activity.reset();
                    self.agent_activity.identify(AgentStatusSource::Process);
                }
                self.running_program = Some(program);
            }
            self.refresh_program_status(cx);
            cx.emit(TerminalViewEvent::SessionIdentityChanged);
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
        if let Some(observation) = &observation
            && self.running_program.as_deref().and_then(AgentKind::parse).is_some()
        {
            let title: String = observation.title.chars().filter(|ch| !ch.is_control()).collect();
            let title = title.trim();
            let title =
                if title.is_empty() || title.starts_with("NEBULA|") { "shell" } else { title };
            if self.title != title {
                self.title = title.into();
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            }
            if self.remote_activity.title_sample_at != Some(observation.observed_at) {
                self.remote_activity.title_sample_at = Some(observation.observed_at);
                self.observe_agent_title(observation.observed_at, cx);
            }
        }
        observation
    }

    /// Pane-scoped observations replace the composite outer screen and title
    /// only while one is current; without them the outer evidence remains.
    pub(super) fn observing_tmux(&self) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session.resume_target.lock().ok().is_some_and(|target| target.is_some())
                && session
                    .tmux_activity
                    .lock()
                    .ok()
                    .is_some_and(|activity| activity.as_ref().is_some_and(TmuxActivity::current))
        })
    }

    /// A multiplexer client is transparent to screen identity while it owns
    /// the SSH PTY and no pane-scoped observation describes its active pane.
    pub(super) fn screen_passthrough(&self) -> bool {
        self.ssh_destination.is_some()
            && self
                .session
                .as_ref()
                .is_some_and(|session| session.multiplexer_client.load(Ordering::Relaxed))
            && !self.observing_tmux()
    }

    /// Restore the wrapper once a screen-identified Agent has stayed off the
    /// screen; no shell boundary is visible through the multiplexer.
    pub(super) fn release_passthrough_agent(
        &mut self,
        detected: Option<AgentKind>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.remote_activity.passthrough.is_none() {
            return false;
        }
        let current = self.running_program.as_deref().and_then(AgentKind::parse);
        if detected.is_some() && detected == current {
            self.remote_activity.passthrough_missing = None;
            return false;
        }
        let since = *self.remote_activity.passthrough_missing.get_or_insert_with(Instant::now);
        if since.elapsed() < PASSTHROUGH_RELEASE {
            return false;
        }
        self.running_program = self.remote_activity.passthrough.take().flatten();
        self.remote_activity.passthrough_missing = None;
        self.ai_session = None;
        self.title = "shell".into();
        self.confirmation.observe_waiting(false);
        self.answers.close();
        self.agent_activity.reset();
        if let Some(session) = &self.session {
            session.term.lock().set_redraw_anchor_enabled(false);
        }
        cx.emit(TerminalViewEvent::SessionIdentityChanged);
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
        true
    }
}

/// A multiplexer can reserve its first or last row for a status line, which
/// would otherwise hide an Agent form anchored to the end of the screen.
pub(super) fn passthrough_identity(screen: &str) -> (&str, Option<AgentKind>) {
    let trimmed = screen.trim_end();
    let candidates = [
        Some(trimmed),
        trimmed.rsplit_once('\n').map(|(rest, _)| rest),
        trimmed.split_once('\n').map(|(_, rest)| rest),
    ];
    for candidate in candidates.into_iter().flatten() {
        if let Some(agent) = crate::ai_agents::identify(candidate) {
            return (candidate, Some(agent));
        }
    }
    (screen, None)
}
