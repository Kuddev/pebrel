use gpui::{Context, EventEmitter as _};

use super::{TerminalView, TerminalViewEvent};

impl TerminalView {
    pub(crate) fn capture_confirmation(
        &mut self,
    ) -> Option<super::super::confirmation::Confirmation> {
        let (screen, mode) = self.runtime_screen_state()?;
        let program = self.running_program.as_deref().unwrap_or_default();
        let waiting = self.confirmation_waiting();
        self.confirmation.capture(
            &screen,
            program,
            self.ai_session.as_ref().map(|identity| identity.session_id.as_str()),
            mode,
            waiting,
        )
    }

    pub(crate) fn confirmation_waiting(&self) -> bool {
        self.marked_text.is_none()
            && self
                .running_program
                .as_deref()
                .and_then(crate::ai_agents::AgentKind::parse)
                .is_some()
            && matches!(
                self.runtime_task_state(),
                crate::runtime_api::RuntimeTaskState::Attention
                    | crate::runtime_api::RuntimeTaskState::WaitingInput
            )
    }

    pub(crate) fn answer_confirmation(
        &mut self,
        request_id: u64,
        allow: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.answer_choice(request_id, usize::from(!allow), cx)
    }

    pub(crate) fn confirmation_generation(&self) -> u64 {
        self.confirmation.generation()
    }

    pub(crate) fn answer_choice(
        &mut self,
        request_id: u64,
        choice: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.session.is_none() || self.pending_runtime_submit.is_some() {
            return false;
        }
        let Some((screen, mode)) = self.runtime_screen_state() else { return false };
        let waiting = self.confirmation_waiting();
        let reply = self.confirmation.answer_choice(
            request_id,
            choice,
            &screen,
            self.running_program.as_deref().unwrap_or_default(),
            self.ai_session.as_ref().map(|identity| identity.session_id.as_str()),
            mode,
            waiting,
        );
        let Some(reply) = reply else { return false };
        let submit = reply.last() == Some(&b'\r');
        let text =
            std::str::from_utf8(if submit { &reply[..reply.len() - 1] } else { &reply }).unwrap();
        let bytes = super::super::keymap::encode_choice_text(text, &mode);
        self.write_input(bytes, cx);
        let epoch = self.prompt_input_epoch;
        let generation = self.confirmation_generation();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            // Separate line submission from text so TUIs cannot treat the pair
            // as one pasted chunk. Manual input or a new lifecycle cancels it.
            executor.timer(std::time::Duration::from_millis(75)).await;
            if submit {
                let _ = this.update(cx, |view, cx| {
                    if view.prompt_input_epoch != epoch
                        || view.confirmation_generation() != generation
                        || !view.confirmation_waiting()
                    {
                        return;
                    }
                    let bytes = crate::input::terminal_input::build_runtime_sequence_for_program(
                        crate::runtime_api::RuntimeKey::Enter,
                        Default::default(),
                        1,
                        view.term_mode(),
                        view.running_program.as_deref(),
                    );
                    view.write_input(bytes, cx);
                });
                return;
            }
            // A numbered answer may advance a multi-question form without a
            // new hook. Advertise the next live question once its frame arrives.
            for _ in 0..8 {
                let keep_waiting = this
                    .update(cx, |view, cx| {
                        if view.prompt_input_epoch != epoch || !view.confirmation_waiting() {
                            return false;
                        }
                        if let Some(next) = view.capture_confirmation() {
                            cx.emit(TerminalViewEvent::Notification(
                                crate::notify::Notification::AiTurn {
                                    program: view.running_program.clone().unwrap_or_default(),
                                    message: Some(next.question),
                                    attention: true,
                                },
                            ));
                            false
                        } else {
                            true
                        }
                    })
                    .unwrap_or(false);
                if !keep_waiting {
                    return;
                }
                executor.timer(std::time::Duration::from_millis(75)).await;
            }
        })
        .detach();
        cx.emit(TerminalViewEvent::TitleChanged);
        true
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use crate::gpui_shell::terminal::view::startup_tests::{feed, open};
    use gpui::TestAppContext;
    use nebula_terminal::event_loop::Msg;
    use std::time::Duration;

    fn waiting(
        view: &mut TerminalView,
        program: &str,
        prompt: &str,
        session: &str,
        cx: &mut Context<TerminalView>,
    ) {
        view.session.as_ref().unwrap().term.lock().set_options(nebula_terminal::term::Config {
            kitty_keyboard: true,
            ..Default::default()
        });
        let header = if program == "codex" {
            "nebula-hook/1 source=codex codex_hooks=full"
        } else {
            "nebula-hook/1 source=claude"
        };
        let wire = format!(
            "{header}\n{}",
            serde_json::json!({"hook_event_name":"PermissionRequest", "session_id":session})
        );
        let event = crate::ai_hook::parse_remote_envelope(wire.as_bytes(), Some(42)).unwrap();
        view.handle_ai_hook(&event, cx);
        feed(view, prompt.replace('\n', "\r\n").as_bytes());
    }

    fn input(receiver: &std::sync::mpsc::Receiver<Msg>) -> Vec<Vec<u8>> {
        receiver
            .try_iter()
            .filter_map(|msg| match msg {
                Msg::Input(bytes) => Some(bytes.to_vec()),
                _ => None,
            })
            .collect()
    }

    #[gpui::test]
    fn binary_answers_separate_legacy_enter_and_cancel_after_manual_input(cx: &mut TestAppContext) {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            waiting(view, "claude", "Allow operation? [Y/n]", "confirmation-test-legacy", cx);
            let request = view.capture_confirmation().unwrap();
            assert!(view.answer_choice(request.id, 0, cx));
        });
        assert_eq!(input(&receiver), [b"Y".to_vec()]);
        window.run_until_parked();
        window.background_executor.advance_clock(Duration::from_millis(80));
        window.run_until_parked();
        assert_eq!(input(&receiver), [b"\r".to_vec()]);
        view.update(window, |view, cx| {
            view.confirmation.observe_waiting(false);
            view.confirmation.observe_waiting(true);
            let request = view.capture_confirmation().unwrap();
            assert!(view.answer_choice(request.id, 1, cx));
            view.write_input(b"x".to_vec(), cx);
        });
        assert_eq!(input(&receiver), [b"n".to_vec(), b"x".to_vec()]);
        window.run_until_parked();
        window.background_executor.advance_clock(Duration::from_millis(80));
        window.run_until_parked();
        assert!(
            input(&receiver).is_empty(),
            "a delayed Enter cannot follow intervening keyboard input"
        );
    }

    #[gpui::test]
    fn binary_confirmation_releases_text_and_enter_when_key_events_are_requested(
        cx: &mut TestAppContext,
    ) {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            waiting(view, "claude", "Allow operation? [Y/n]", "confirmation-test-key-events", cx);
            feed(view, b"\x1b[>10u");
            let request = view.capture_confirmation().unwrap();
            assert!(view.answer_choice(request.id, 0, cx));
        });
        assert_eq!(input(&receiver), [b"\x1b[89u\x1b[89;1:3u".to_vec()]);
        window.run_until_parked();
        window.background_executor.advance_clock(Duration::from_millis(80));
        window.run_until_parked();
        assert_eq!(input(&receiver), [b"\x1b[13u\x1b[13;1:3u".to_vec()]);
    }

    #[gpui::test]
    fn numbered_codex_answers_send_one_key_without_enter(cx: &mut TestAppContext) {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            waiting(view, "codex", "Question 1/1 (1 unanswered)\nChoose a scope.\n› 1. Current\n  2. All\ntab to add notes | enter to submit answer | esc to interrupt", "confirmation-test-numbered", cx);
            feed(view, b"\x1b[>10u");
            assert!(view.term_mode().contains(nebula_terminal::term::TermMode::REPORT_ALL_KEYS_AS_ESC));
            let request = view.capture_confirmation().unwrap();
            assert_eq!(request.choices, ["Current", "All"]);
            assert!(view.answer_choice(request.id, 1, cx));
            assert!(!view.answer_choice(request.id, 0, cx));
        });
        assert_eq!(input(&receiver), [b"\x1b[50u\x1b[50;1:3u".to_vec()]);
        window.run_until_parked();
        window.background_executor.advance_clock(Duration::from_millis(800));
        window.run_until_parked();
        assert!(input(&receiver).is_empty());
    }
}
