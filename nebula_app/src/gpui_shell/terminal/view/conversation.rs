//! The chat view is a projection of this live CLI, not another Agent session.

use gpui::Context;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{InputOrigin, TerminalView};
use crate::{
    assistant_answer::conversation::{Location, Source},
    runtime_api::{
        ApiError, RuntimeTaskState,
        conversation::{Identity, Request},
    },
};

#[derive(Serialize)]
struct Prompt {
    id: String,
    text: String,
    options: Vec<String>,
    selected: usize,
    binary: bool,
}

fn project_confirmation(question: super::super::confirmation::Confirmation) -> Prompt {
    let binary = question.choices.is_empty();
    let id = Sha256::digest(question.id.to_le_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Prompt { id, text: question.question, options: question.choices, selected: 0, binary }
}

impl TerminalView {
    pub(crate) fn runtime_conversation_identity(&self) -> Result<Identity, ApiError> {
        self.ensure_runtime_readable()?;
        let agent = self.runtime_chat_agent().ok_or_else(|| {
            ApiError::new("conversation_ended", "this pane no longer contains a live Agent")
        })?;
        let session_id = agent.session_id.filter(|id| !id.is_empty()).ok_or_else(|| {
            ApiError::new(
                "conversation_unavailable",
                "the Agent has not reported its native conversation identity",
            )
        })?;
        Ok(Identity { kind: agent.kind, session_id, epoch: Some(self.ai_session_probe_epoch) })
    }

    pub(crate) fn runtime_conversation_check(
        &self,
        expected: &Identity,
    ) -> Result<Identity, ApiError> {
        let current = self.runtime_conversation_identity()?;
        if !expected.matches(&current) {
            return Err(ApiError::new(
                "conversation_identity_changed",
                "the Agent conversation was replaced",
            ));
        }
        Ok(current)
    }

    pub(crate) fn runtime_conversation_source(
        &self,
        expected: &Identity,
    ) -> Result<Source, ApiError> {
        let identity = self.runtime_conversation_check(expected)?;
        let target = self
            .session_agent()
            .filter(|target| {
                target.source == identity.kind
                    && target.session_id.as_deref() == Some(identity.session_id.as_str())
            })
            .ok_or_else(|| {
                ApiError::new("conversation_unavailable", "native conversation metadata is pending")
            })?;
        let path = target
            .session_file
            .filter(|path| crate::session::valid_native_session_file(path))
            .ok_or_else(|| {
                ApiError::new(
                    "conversation_unavailable",
                    "the Agent has not reported a transcript path",
                )
            })?;
        let location = if self.ssh_destination.is_some() {
            let reader = self
                .session
                .as_ref()
                .and_then(|session| session.remote_reader.lock().ok()?.clone())
                .ok_or_else(|| {
                    ApiError::new(
                        "conversation_unavailable",
                        "the SSH transcript connection is unavailable",
                    )
                })?;
            Location::Ssh(reader)
        } else {
            Location::Local(self.exec_context.clone().ok_or_else(|| {
                ApiError::new("conversation_unavailable", "the pane environment is unavailable")
            })?)
        };
        Ok(Source { identity, path, location, cwd: self.cwd.clone() })
    }

    fn conversation_prompt(&mut self) -> Option<(u64, Prompt)> {
        let question = self.capture_confirmation()?;
        Some((question.id, project_confirmation(question)))
    }

    pub(crate) fn runtime_conversation_status(
        &mut self,
        expected: &Identity,
    ) -> Result<Value, ApiError> {
        let identity = self.runtime_conversation_check(expected)?;
        let prompt = self.conversation_prompt().map(|(_, prompt)| prompt);
        Ok(json!({"identity": identity, "state": self.runtime_task_state(), "prompt": prompt,
            "can_send": self.pending_runtime_submit.is_none() && matches!(self.runtime_task_state(), RuntimeTaskState::Idle | RuntimeTaskState::Finished)}))
    }

    pub(crate) fn runtime_conversation_apply(
        &mut self,
        request: &Request,
        cx: &mut Context<Self>,
    ) -> Result<Value, ApiError> {
        let identity = self.runtime_conversation_check(request.identity())?;
        if self.pending_runtime_submit.is_some() {
            return Err(ApiError::new(
                "input_in_progress",
                "the previous message is still being committed",
            ));
        }
        match request {
            Request::Send { text, .. } => {
                if !matches!(
                    self.runtime_task_state(),
                    RuntimeTaskState::Idle | RuntimeTaskState::Finished
                ) {
                    return Err(ApiError::new(
                        "agent_busy",
                        "the Agent is working or awaiting a terminal interaction",
                    ));
                }
                self.runtime_chat_message(text.clone(), InputOrigin::User, cx)?;
            },
            Request::Choose { prompt_id, option, .. } => {
                let (request_id, _) = self
                    .conversation_prompt()
                    .filter(|(_, prompt)| &prompt.id == prompt_id)
                    .ok_or_else(|| {
                        ApiError::new(
                            "prompt_changed",
                            "the visible question changed; read its current options",
                        )
                    })?;
                // 和桌面确认通知共用问题身份、一次性消费及按键编码；不另建屏幕解析器。
                if !self.answer_choice(request_id, *option, cx) {
                    return Err(ApiError::new(
                        "prompt_changed",
                        "the visible question changed; read its current options",
                    ));
                }
            },
            Request::Key { key, modifiers, .. } => {
                self.runtime_send_key(*key, *modifiers, 1, cx)?;
            },
            Request::Read { .. } => {
                return Err(ApiError::new(
                    "runtime_unavailable",
                    "conversation reads require the background dispatcher",
                ));
            },
        }
        Ok(json!({"identity": identity, "accepted": true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_terminal::term::TermMode;

    #[test]
    fn conversation_projection_keeps_native_question_identity_and_single_key_semantics() {
        let mut state = super::super::super::confirmation::ConfirmationState::default();
        let screen = "Question 1/1 (1 unanswered)\nChoose a scope.\n› 1. Current\n  2. All\ntab to add notes | enter to submit answer | esc to interrupt";
        state.observe_waiting(true);
        let native =
            state.capture(screen, "codex", Some("session"), TermMode::empty(), true).unwrap();
        let first = project_confirmation(native.clone());
        assert_eq!(first.options, ["Current", "All"]);
        assert_eq!(
            state.answer_choice(
                native.id,
                1,
                screen,
                "codex",
                Some("session"),
                TermMode::empty(),
                true
            ),
            Some(b"2".to_vec())
        );
        assert!(state.capture(screen, "codex", Some("session"), TermMode::empty(), true).is_none());
        state.observe_waiting(false);
        state.observe_waiting(true);
        let repeated = project_confirmation(
            state.capture(screen, "codex", Some("session"), TermMode::empty(), true).unwrap(),
        );
        assert_ne!(
            first.id, repeated.id,
            "a new round of the same question must invalidate the old phone button"
        );
        assert!(
            state
                .answer_choice(
                    native.id,
                    0,
                    screen,
                    "codex",
                    Some("session"),
                    TermMode::empty(),
                    true
                )
                .is_none()
        );
        assert!(
            state
                .capture(
                    "Read this list:\n1. Yes\n2. No",
                    "codex",
                    Some("session"),
                    TermMode::empty(),
                    true
                )
                .is_none()
        );
    }
}
