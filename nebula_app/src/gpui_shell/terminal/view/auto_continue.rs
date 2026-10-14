//! Pane-owned retry timer. Never bypass hook arbitration or broadcast input.
use std::time::Instant;

use gpui::{Context, EventEmitter as _};
use nebula_terminal::event_loop::Msg;

use crate::ai_agents::AgentStatus;
use crate::ai_hook::AiHookEvent;
use crate::gpui_shell::config::Settings;

use super::{TerminalView, TerminalViewEvent};

impl TerminalView {
    pub(super) fn cancel_auto_continue(&mut self) {
        self.auto_continue_task = None;
        self.auto_continue.cancel();
    }

    pub(super) fn schedule_auto_continue(&mut self, event: &AiHookEvent, cx: &mut Context<Self>) {
        // A newer accepted event cancels the old turn's timer, even if not retryable.
        self.auto_continue_task = None;
        let delay = self.auto_continue.observe(event, Instant::now());
        if !cx.try_global::<Settings>().is_some_and(|settings| settings.ai_auto_continue) {
            return;
        }
        let Some(delay) = delay else { return };
        let session_id = event.session_id.clone().expect("policy requires identity");
        let input_epoch = self.prompt_input_epoch;
        let executor = cx.background_executor().clone();
        self.auto_continue_task = Some(cx.spawn(async move |this, cx| {
            executor.timer(delay).await;
            let _ = this.update(cx, |view, cx| {
                if !cx.try_global::<Settings>().is_some_and(|settings| settings.ai_auto_continue)
                    || view.prompt_input_epoch != input_epoch
                    || view.exited.is_some()
                    || view.error.is_some()
                    || view.marked_text.is_some()
                    || view.pending_runtime_submit.is_some()
                    || view.running_program.as_deref() != Some("claude")
                    || view.agent_activity.status() != AgentStatus::Idle
                    || !view.last_command_failed
                    || !view.ai_session.as_ref().is_some_and(|identity| {
                        identity.source == "claude" && identity.session_id == session_id
                    })
                {
                    return;
                }
                let Ok(enter) = view.runtime_key_sequence(
                    crate::runtime_api::RuntimeKey::Enter,
                    crate::runtime_api::RuntimeKeyModifiers::default(),
                    1,
                ) else {
                    return;
                };
                let mut bytes = b"continue".to_vec();
                bytes.extend(enter);
                let Some(session) = &view.session else { return };
                // Queue only to this pane, respecting its negotiated Enter protocol.
                // A closed PTY channel is not a retry.
                if session.notifier.0.send(Msg::Input(bytes.into())).is_err() {
                    return;
                }
                view.auto_continue.sent(Instant::now());
                view.agent_activity.submitted();
                view.last_command_failed = false;
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            });
        }));
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
