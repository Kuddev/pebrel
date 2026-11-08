//! Project authenticated active-pane observations into the shared Agent lifecycle.
use std::time::Instant;

use gpui::{Context, EventEmitter as _};

use super::{TerminalView, TerminalViewEvent};
use crate::ai_agents::{AgentKind, AgentStatusSource};
use crate::ssh_session::TmuxActivity;

#[derive(Default)]
pub(super) struct RemoteActivity {
    pub context: Option<String>,
    pub sample_at: Option<Instant>,
    title_sample_at: Option<Instant>,
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

    pub(super) fn observing_tmux(&self) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session.resume_target.lock().ok().is_some_and(|target| target.is_some())
        })
    }
}
