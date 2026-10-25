use super::super::startup_tests::{feed, open};
use super::*;
use gpui::{BorrowAppContext as _, TestAppContext};
use std::time::Duration;

fn hook(owner: &str, name: &str, sequence: u64) -> AiHookEvent {
    hook_with_prompt(owner, name, sequence, Some(owner))
}

fn hook_with_prompt(
    owner: &str,
    name: &str,
    sequence: u64,
    prompt_id: Option<&str>,
) -> AiHookEvent {
    let prompt = prompt_id.map_or_else(String::new, |id| format!(",\"prompt_id\":\"{id}\""));
    crate::ai_hook::parse_remote_envelope(
        format!(
            "nebula-hook/1 source=claude \
             process={owner}\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"{owner}\"{prompt},\
             \"error\":\"unknown\",\"bridge_sequence\":{sequence}}}"
        )
        .as_bytes(),
        Some(42),
    )
    .unwrap()
}

#[gpui::test]
fn auto_continue_sends_once_to_owning_pane_without_hiding_failure(cx: &mut TestAppContext) {
    let owner = "auto-sends";
    let (view, window, receiver) = open(cx);
    let mut queued_old_events = None;
    window.update(|_, cx| {
        cx.update_global::<Settings, _>(|settings, _| settings.ai_auto_continue = true)
    });
    view.update(window, |view, cx| {
        let mut prompt = hook(owner, "UserPromptSubmit", 1);
        prompt.turn_id = Some("current-prompt".into());
        view.handle_ai_hook(&prompt, cx);
        let mut failure = hook(owner, "StopFailure", 2);
        failure.turn_id = prompt.turn_id.clone();
        view.handle_ai_hook(&failure, cx);
        assert!(view.last_command_failed);
        view.handle_ai_hook(&failure, cx);
        failure.bridge_sequence = Some(3);
        view.handle_ai_hook(&failure, cx);
        queued_old_events =
            Some((hook(owner, "UserPromptSubmit", 4), hook(owner, "StopFailure", 5)));
    });
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_millis(1499));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
    window.executor().advance_clock(Duration::from_millis(1));
    window.run_until_parked();
    let inputs: Vec<_> = receiver
        .try_iter()
        .filter_map(|msg| match msg {
            Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(inputs, vec![b"continue\r".to_vec()]);
    view.update(window, |view, cx| {
        let (old_prompt, old_failure) = queued_old_events.take().unwrap();
        assert!(!view.handle_ai_hook(&old_prompt, cx));
        assert!(!view.handle_ai_hook(&old_failure, cx));
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
    });
    window.executor().advance_clock(Duration::from_secs(10));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
}

#[gpui::test]
fn queued_old_claude_hooks_cannot_retry_after_keyboard_submit(cx: &mut TestAppContext) {
    let owner = "keyboard-submit-fence";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| {
        cx.update_global::<Settings, _>(|settings, _| settings.ai_auto_continue = true)
    });
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx));
            assert!(view.handle_ai_hook(&hook(owner, "Stop", 2), cx));
            let old_prompt = hook(owner, "UserPromptSubmit", 3);
            let old_failure = hook(owner, "StopFailure", 4);
            view.on_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("enter").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
            assert!(!view.handle_ai_hook(&old_prompt, cx));
            assert!(!view.handle_ai_hook(&old_failure, cx));
        });
    });
    receiver.try_iter().for_each(drop);
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_secs(2));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_))));

    view.update(window, |view, cx| {
        let next = hook_with_prompt(owner, "UserPromptSubmit", 5, Some("next-prompt"));
        assert!(view.handle_ai_hook(&next, cx));
        assert!(view.agent_activity.is_current_turn("next-prompt"));
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
    });
}

#[gpui::test]
fn raw_remote_hook_queued_before_input_is_rejected_after_pump(cx: &mut TestAppContext) {
    let owner = "raw-hook-submit-fence";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| {
        cx.update_global::<Settings, _>(|settings, _| settings.ai_auto_continue = true)
    });
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx));
            assert!(view.handle_ai_hook(&hook(owner, "Stop", 2), cx));
            let (hook_sender, mut hook_receiver) = super::super::super::event_mailbox::channel();
            let old_prompt = nebula_terminal::event::Event::AiHookEnvelope(
                format!(
                    "nebula-hook/1 source=claude \
                     process={owner}\n{{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"\
                     {owner}\",\"prompt_id\":\"{owner}\"}}"
                )
                .into_bytes(),
            );
            let old_failure = nebula_terminal::event::Event::AiHookEnvelope(
                format!(
                    "nebula-hook/1 source=claude \
                     process={owner}\n{{\"hook_event_name\":\"StopFailure\",\"session_id\":\"\
                     {owner}\",\"prompt_id\":\"{owner}\",\"error\":\"unknown\"}}"
                )
                .into_bytes(),
            );
            hook_sender.send(old_prompt);
            hook_sender.send(old_failure);
            view.on_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("enter").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
            for queued in
                [hook_receiver.try_recv_queued().unwrap(), hook_receiver.try_recv_queued().unwrap()]
            {
                view.process_event_with_sequence(queued.event, queued.received_sequence, cx);
            }
            assert_eq!(view.agent_activity.status(), AgentStatus::Working);
            assert!(!view.last_command_failed);
        });
    });
    receiver.try_iter().for_each(drop);
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_secs(2));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_))));
}

#[gpui::test]
fn runtime_claude_submit_fences_queued_old_hooks(cx: &mut TestAppContext) {
    let owner = "runtime-submit-fence";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| {
        cx.update_global::<Settings, _>(|settings, _| settings.ai_auto_continue = true)
    });
    view.update(window, |view, cx| {
        assert!(view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx));
        let old_prompt = hook(owner, "UserPromptSubmit", 2);
        let old_failure = hook(owner, "StopFailure", 3);
        view.runtime_prompt("next prompt".into(), true, cx).unwrap();
        feed(view, b"next prompt");
        view.flush_pending_runtime_submit(cx);
        assert!(view.pending_runtime_submit.is_none());
        assert!(!view.handle_ai_hook(&old_prompt, cx));
        assert!(!view.handle_ai_hook(&old_failure, cx));
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
    });
    receiver.try_iter().for_each(drop);
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_secs(2));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_))));
}

#[gpui::test]
fn shift_enter_does_not_arm_a_new_claude_turn(cx: &mut TestAppContext) {
    let owner = "shift-enter-submit-fence";
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx));
            feed(view, b"\x1b[?9001h");
            view.on_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("shift-enter").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
            let next = hook_with_prompt(owner, "UserPromptSubmit", 2, Some("next-prompt"));
            assert!(!view.handle_ai_hook(&next, cx));
            assert!(view.agent_activity.is_current_turn(owner));
        });
    });
}

#[gpui::test]
fn enter_answering_claude_attention_does_not_arm_a_new_prompt(cx: &mut TestAppContext) {
    let owner = "permission-answer-submit-fence";
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx));
            assert!(view.handle_ai_hook(&hook(owner, "PermissionRequest", 2), cx));
            assert_eq!(view.agent_activity.status(), AgentStatus::Blocked);
            view.on_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("enter").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
            assert!(view.handle_ai_hook(&hook(owner, "StopFailure", 3), cx));
            assert_eq!(view.agent_activity.status(), AgentStatus::Idle);
            assert!(view.last_command_failed);
        });
    });
}

#[gpui::test]
fn auto_continue_cancels_on_input_setting_changes_new_events_and_exit(cx: &mut TestAppContext) {
    for action in ["input", "toggle", "prompt", "end", "command", "identity", "closed"] {
        let owner = action;
        let (view, window, receiver) = open(cx);
        window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
        view.update(window, |view, cx| {
            view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx);
            view.handle_ai_hook(&hook(owner, "StopFailure", 2), cx);
        });
        window.run_until_parked();
        if action == "toggle" {
            window
                .update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = false));
            window.run_until_parked();
            window
                .update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
        } else {
            view.update(window, |view, cx| match action {
                "input" => view.write_input(b"manual".to_vec(), cx),
                "prompt" => {
                    view.agent_activity.submitted_text("claude", "next prompt");
                    view.handle_ai_hook(
                        &hook_with_prompt(owner, "UserPromptSubmit", 3, Some("next-prompt")),
                        cx,
                    );
                },
                "end" => {
                    view.handle_ai_hook(&hook(owner, "SessionEnd", 3), cx);
                },
                "command" => {
                    view.clear_foreground_agent_state(cx);
                },
                "identity" => {
                    view.ai_session = None;
                },
                "closed" => {
                    view.session = None;
                },
                _ => unreachable!(),
            });
        }
        receiver.try_iter().for_each(drop);
        window.executor().advance_clock(Duration::from_secs(10));
        window.run_until_parked();
        assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))), "{action}");
    }
}

#[gpui::test]
fn auto_continue_disabled_and_foreign_failure_never_submit(cx: &mut TestAppContext) {
    for enabled in [false, true] {
        let owner = if enabled { "auto-foreign" } else { "auto-disabled" };
        let (view, window, receiver) = open(cx);
        window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = enabled));
        view.update(window, |view, cx| {
            view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx);
            let mut event = hook(owner, "StopFailure", 2);
            if enabled {
                event.session_id = Some("foreign".into());
            }
            view.handle_ai_hook(&event, cx);
        });
        window.run_until_parked();
        window.executor().advance_clock(Duration::from_secs(10));
        window.run_until_parked();
        assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
    }
}

#[gpui::test]
fn auto_continue_ignores_late_prompt_submit_and_failure_from_previous_turn(
    cx: &mut TestAppContext,
) {
    let owner = "late-prompt-identity";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
    view.update(window, |view, cx| {
        let old = hook_with_prompt(owner, "UserPromptSubmit", 1, Some("old-prompt"));
        assert!(view.handle_ai_hook(&old, cx));

        view.agent_activity.submitted_text("claude", "current prompt");
        let current = hook_with_prompt(owner, "UserPromptSubmit", 2, Some("current-prompt"));
        assert!(view.handle_ai_hook(&current, cx));

        let late_prompt = hook_with_prompt(owner, "UserPromptSubmit", 3, Some("old-prompt"));
        assert!(!view.handle_ai_hook(&late_prompt, cx));
        let late_failure = hook_with_prompt(owner, "StopFailure", 4, Some("old-prompt"));
        assert!(!view.handle_ai_hook(&late_failure, cx));
    });
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_secs(2));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
    view.read_with(window, |view, _| {
        assert!(view.agent_activity.is_current_turn("current-prompt"));
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
        assert!(!view.last_command_failed);
    });
}

#[gpui::test]
fn working_claude_enter_allows_the_queued_prompt_after_the_current_turn(cx: &mut TestAppContext) {
    let owner = "working-enter-queued-prompt";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.handle_ai_hook(
                &hook_with_prompt(owner, "UserPromptSubmit", 1, Some("current-prompt")),
                cx,
            ));
            assert_eq!(view.agent_activity.status(), AgentStatus::Working);

            view.on_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("enter").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );

            let previous_failure =
                hook_with_prompt(owner, "StopFailure", 2, Some("current-prompt"));
            assert!(!view.handle_ai_hook(&previous_failure, cx));

            let next = hook_with_prompt(owner, "UserPromptSubmit", 3, Some("queued-prompt"));
            assert!(view.handle_ai_hook(&next, cx));
            assert!(view.agent_activity.is_current_turn("queued-prompt"));
            assert_eq!(view.agent_activity.status(), AgentStatus::Working);

            let queued_failure = hook_with_prompt(owner, "StopFailure", 4, Some("queued-prompt"));
            assert!(view.handle_ai_hook(&queued_failure, cx));
            assert!(view.last_command_failed);
            assert!(view.auto_continue_task.is_some());
            assert!(!view.handle_ai_hook(
                &hook_with_prompt(owner, "StopFailure", 5, Some("queued-prompt")),
                cx,
            ));
        });
    });
    receiver.try_iter().for_each(drop);
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_millis(1499));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
    window.executor().advance_clock(Duration::from_millis(1));
    window.run_until_parked();
    let inputs: Vec<_> = receiver
        .try_iter()
        .filter_map(|msg| match msg {
            Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(inputs, vec![b"continue\r".to_vec()]);
    window.executor().advance_clock(Duration::from_secs(10));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
    receiver.try_iter().for_each(drop);
}

#[gpui::test]
fn auto_continue_rejects_a_prompt_seen_before_many_newer_turns(cx: &mut TestAppContext) {
    let owner = "seen-older-prompt";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
    view.update(window, |view, cx| {
        assert!(view.handle_ai_hook(
            &hook_with_prompt(owner, "UserPromptSubmit", 1, Some("older-prompt")),
            cx,
        ));
        let identityless_start = crate::ai_hook::parse_remote_envelope(
            format!(
                "nebula-hook/1 source=claude \
                 process={owner}\n{{\"hook_event_name\":\"SessionStart\"}}"
            )
            .as_bytes(),
            Some(42),
        )
        .unwrap();
        assert!(view.handle_ai_hook(&identityless_start, cx));
        view.agent_activity.submitted_text("claude", "current prompt");
        assert!(view.handle_ai_hook(
            &hook_with_prompt(owner, "UserPromptSubmit", 2, Some("current-prompt")),
            cx,
        ));

        view.agent_activity.submitted_text("claude", "next prompt");
        let mut sequence = 2;
        let mut latest_prompt = None;
        for index in 0..65 {
            sequence += 1;
            let prompt_id = format!("prompt-{index}");
            view.agent_activity.submitted_text("claude", "next prompt");
            assert!(view.handle_ai_hook(
                &hook_with_prompt(owner, "UserPromptSubmit", sequence, Some(&prompt_id)),
                cx,
            ));
            latest_prompt = Some(prompt_id);
        }

        sequence += 1;
        view.agent_activity.submitted_text("claude", "replay old prompt");
        assert!(!view.handle_ai_hook(
            &hook_with_prompt(owner, "UserPromptSubmit", sequence, Some("older-prompt")),
            cx,
        ));
        sequence += 1;
        assert!(!view.handle_ai_hook(
            &hook_with_prompt(owner, "StopFailure", sequence, Some("older-prompt")),
            cx,
        ));
        assert!(view.agent_activity.is_current_turn(latest_prompt.as_deref().unwrap()));
        assert!(!view.last_command_failed);
    });
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_secs(2));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
}

#[gpui::test]
fn auto_continue_rejects_late_failure_without_prompt_identity(cx: &mut TestAppContext) {
    let owner = "late-without-prompt";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
    view.update(window, |view, cx| {
        view.handle_ai_hook(
            &hook_with_prompt(owner, "UserPromptSubmit", 1, Some("current-prompt")),
            cx,
        );
        view.handle_ai_hook(&hook_with_prompt(owner, "StopFailure", 2, None), cx);
        assert!(view.auto_continue_task.is_none());
    });
    window.executor().advance_clock(Duration::from_secs(10));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
}

#[gpui::test]
fn auto_continue_does_not_rearm_after_identityless_session_start(cx: &mut TestAppContext) {
    let owner = "late-after-session-start";
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
    view.update(window, |view, cx| {
        view.handle_ai_hook(
            &hook_with_prompt(owner, "UserPromptSubmit", 1, Some("current-prompt")),
            cx,
        );
        view.handle_ai_hook(&hook_with_prompt(owner, "SessionStart", 2, None), cx);
        view.handle_ai_hook(&hook_with_prompt(owner, "StopFailure", 3, Some("current-prompt")), cx);
        assert!(view.auto_continue_task.is_none());
    });
    window.executor().advance_clock(Duration::from_secs(10));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
}

#[gpui::test]
fn auto_continue_cancels_when_ime_preedit_is_abandoned(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler as _;

    for clear_with_unmark in [true, false] {
        let owner = if clear_with_unmark { "ime-unmark" } else { "ime-empty" };
        let (view, window, receiver) = open(cx);
        window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
        view.update(window, |view, cx| {
            view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 1), cx);
            view.handle_ai_hook(&hook(owner, "StopFailure", 2), cx);
        });
        window.run_until_parked();
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
                if clear_with_unmark {
                    view.unmark_text(window, cx);
                } else {
                    view.replace_and_mark_text_in_range(None, "", None, window, cx);
                }
                assert!(view.marked_text.is_none());
            });
        });
        window.executor().advance_clock(Duration::from_secs(2));
        window.run_until_parked();
        assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))), "{owner}");
    }
}

#[gpui::test]
fn auto_continue_cancels_before_clipboard_image_staging(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
    view.update(window, |view, cx| {
        view.handle_ai_hook(&hook("paste-intent", "UserPromptSubmit", 1), cx);
        view.handle_ai_hook(&hook("paste-intent", "StopFailure", 2), cx);
    });
    window.run_until_parked();
    window.update(|window, cx| {
        // Invalid bytes keep this regression independent of image files and host staging.
        // Accepting the paste must cancel retry before asynchronous decoding can finish.
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]);
        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image));
        view.update(cx, |view, cx| {
            assert!(view.auto_continue_task.is_some());
            view.paste(window, cx);
            assert!(view.auto_continue_task.is_none(), "paste intent must cancel the timer");
        });
    });
    window.run_until_parked();
    window.executor().advance_clock(Duration::from_secs(2));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
}

#[gpui::test]
fn auto_continue_cancels_before_external_path_translation(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| cx.update_global::<Settings, _>(|s, _| s.ai_auto_continue = true));
    view.update(window, |view, cx| {
        view.handle_ai_hook(&hook("drop-intent", "UserPromptSubmit", 1), cx);
        view.handle_ai_hook(&hook("drop-intent", "StopFailure", 2), cx);
        view.exec_context = Some(crate::runtime_exec::PaneExecContext::from_pty_options(
            &nebula_terminal::tty::Options {
                shell: Some(nebula_terminal::tty::Shell::new("wsl.exe".into(), Vec::new())),
                ..Default::default()
            },
        ));
    });
    window.run_until_parked();
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.drop_external_paths(
                &[std::path::PathBuf::from("C:/fixture/image.png")],
                window,
                cx,
            );
            // Do not run the executor: cancellation precedes the WSL conversion job.
            assert!(view.auto_continue_task.is_none(), "accepted drop must cancel the timer");
        });
    });
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
}
