use super::super::startup_tests::open;
use super::*;
use gpui::{BorrowAppContext as _, TestAppContext};
use std::time::Duration;

fn hook(owner: &str, name: &str, sequence: u64) -> AiHookEvent {
    crate::ai_hook::parse_remote_envelope(
        format!("nebula-hook/1 source=claude process={owner}\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"{owner}\",\"error\":\"unknown\",\"bridge_sequence\":{sequence}}}").as_bytes(), Some(42),
    ).unwrap()
}

#[gpui::test]
fn auto_continue_sends_once_to_owning_pane_without_hiding_failure(cx: &mut TestAppContext) {
    let owner = "auto-sends";
    let (view, window, receiver) = open(cx);
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
    view.read_with(window, |view, _| {
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
    });
    window.executor().advance_clock(Duration::from_secs(10));
    window.run_until_parked();
    assert!(!receiver.try_iter().any(|msg| matches!(msg, Msg::Input(_))));
}

#[gpui::test]
fn auto_continue_cancels_on_input_setting_changes_new_events_and_exit(cx: &mut TestAppContext) {
    for action in ["input", "toggle", "prompt", "stop", "end", "command", "identity", "closed"] {
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
                    view.handle_ai_hook(&hook(owner, "UserPromptSubmit", 3), cx);
                },
                "stop" => {
                    view.handle_ai_hook(&hook(owner, "Stop", 3), cx);
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
