//! Native CMD input comes from the marked grid, including history recall.
use super::*;
use gpui::TestAppContext;
use nebula_terminal::event::Event;
use nebula_terminal::event_loop::StreamProcessor;
use startup_tests::{feed, open};

fn prompt_bytes(prompt: &str, input: &str) -> Vec<u8> {
    format!(
        "\x1b]133;A\x07{prompt}\x1b]133;B\x07\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07{input}"
    )
    .into_bytes()
}

fn root_process() -> Vec<crate::process_tree::ProcessEntry> {
    vec![crate::process_tree::ProcessEntry {
        pid: 1,
        parent_pid: 0,
        depth: 0,
        executable: "cmd.exe".into(),
    }]
}

#[gpui::test]
fn custom_native_prompts_capture_recalled_input_and_keep_submission_epochs(
    cx: &mut gpui::TestAppContext,
) {
    for prompt in [
        "[C:\\work] ",
        "[C:\\work]\r\n>",
        "",
        "工作目录 :: ",
        &"x".repeat(75),
        &"x".repeat(79),
        &"x".repeat(80),
        &"x".repeat(160),
    ] {
        let (view, window, _) = open(cx);
        view.update(window, |view, cx| {
            let (session, _input, mut events, proxy) = session::test_session_with_events();
            view.session = Some(session);
            view.suggest.suggest_env = crate::display::SuggestEnv::Local;
            let mut stream = StreamProcessor::default();
            stream.feed(
                &mut view.session.as_ref().unwrap().term.lock(),
                &proxy,
                &prompt_bytes(prompt, "pause"),
            );
            view.suggest.line_buf.clear();
            view.commit_line(cx);
            assert_eq!(view.suggest.last_committed, "pause", "prompt {prompt:?}");
            assert!(view.command_running);
            view.write_input(b"\r".to_vec(), cx);
            while let Ok(event) = events.try_recv() {
                view.process_event(event, cx);
            }
            view.apply_prompt_process_probe(
                view.command_started,
                view.prompt_input_epoch,
                Ok(root_process()),
                cx,
            );
            assert!(view.command_running, "queued old prompt must not finish pause");

            let mut next = b"\r\n".to_vec();
            next.extend(prompt_bytes(prompt, ""));
            stream.feed(&mut view.session.as_ref().unwrap().term.lock(), &proxy, &next);
            while let Ok(event) = events.try_recv() {
                view.process_event(event, cx);
            }
            view.apply_prompt_process_probe(
                view.command_started,
                view.prompt_input_epoch,
                Ok(root_process()),
                cx,
            );
            assert!(!view.command_running, "fresh prompt ends the builtin wait");
        });
    }
}

#[gpui::test]
fn custom_native_prompts_handle_empty_submit_paste_and_runtime_without_fake_history(
    cx: &mut gpui::TestAppContext,
) {
    let nonce =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    for (index, prompt) in ["", "[C:\\work] ", "[C:\\work]\r\n>", &"x".repeat(80), &"x".repeat(160)]
        .into_iter()
        .enumerate()
    {
        for action in ["empty", "paste", "runtime"] {
            let prefix = format!("echo pebrel_native_history_{nonce}_{index}_{action}");
            let command = format!("{prefix} echoed");
            let scope = crate::nebula_history::HistoryScope::Local;
            assert_eq!(suggest::history_hint_for_test(&scope, &prefix), None);
            let (view, window, _) = open(cx);
            view.update(window, |view, cx| {
                let (session, _input, _events, proxy) = session::test_session_with_events();
                view.session = Some(session);
                view.suggest.suggest_env = crate::display::SuggestEnv::Local;
                StreamProcessor::default().feed(
                    &mut view.session.as_ref().unwrap().term.lock(),
                    &proxy,
                    &prompt_bytes(prompt, ""),
                );
                assert!(crate::display::nebula_shell_ready_from_raw_grid(
                    &view.session.as_ref().unwrap().term.lock(),
                    &view.suggest.suggest_env,
                ));
                match action {
                    "empty" => view.commit_line(cx),
                    "paste" => view.paste_now_impl(&format!("{command}\r\n"), false, cx),
                    _ => {
                        view.runtime_prompt(command.clone(), true, cx).unwrap();
                    },
                }
                assert_eq!(view.command_running, action != "empty", "{prompt:?} {action}");
                // Runtime keeps the submitted program identity before its echo
                // barrier. Persistent history must still wait for screen input.
                let identity = if action == "runtime" { command.as_str() } else { "" };
                assert_eq!(view.suggest.last_committed, identity, "{prompt:?} {action}");
                assert_eq!(
                    suggest::history_hint_for_test(&scope, &prefix),
                    None,
                    "unconfirmed input is not history: {prompt:?} {action}"
                );
                if action != "empty" {
                    assert!(view.suggest.pending_command_prompt.is_some());
                } else {
                    // Positive control reads the same history owner after real
                    // terminal echo and Enter, including pending-wrap prompts.
                    feed(view, command.as_bytes());
                    view.commit_line(cx);
                    assert_eq!(view.suggest.last_committed, command);
                    assert_eq!(
                        suggest::history_hint_for_test(&scope, &prefix).as_deref(),
                        Some(" echoed")
                    );
                }
            });
        }
    }
}

fn native_prompt(view: &mut TerminalView, cx: &mut Context<'_, TerminalView>) {
    view.session.as_ref().unwrap().native_prompt.observe_prompt();
    view.process_event(Event::UserVar { name: "pebrel_cmd_prompt".into(), value: "1".into() }, cx);
}

#[gpui::test]
fn native_queued_prompt_cannot_finish_a_later_submission(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let (session, _input, mut events, proxy) = session::test_session_with_events();
        view.session = Some(session);
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        // Parse a real prompt, but hold its mailbox delivery until after Enter.
        nebula_terminal::event_loop::StreamProcessor::default().feed(
            &mut *view.session.as_ref().unwrap().term.lock(),
            &proxy,
            b"C:\\work>\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07pause",
        );
        view.runtime_send_key(crate::runtime_api::RuntimeKey::Enter, Default::default(), 1, cx)
            .unwrap();
        let started = view.command_started;
        while let Ok(event) = events.try_recv() {
            view.process_event(event, cx);
        }
        view.apply_prompt_process_probe(
            started,
            view.prompt_input_epoch,
            Ok(vec![crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                executable: "cmd.exe".into(),
                depth: 0,
            }]),
            cx,
        );
        assert_eq!(
            view.sidebar_activity(),
            SidebarActivity::Running,
            "queued startup prompt must not end pause"
        );
        // A later real prompt still completes this same command without another Enter.
        nebula_terminal::event_loop::StreamProcessor::default().feed(
            &mut *view.session.as_ref().unwrap().term.lock(),
            &proxy,
            b"\r\nC:\\work>\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07",
        );
        while let Ok(event) = events.try_recv() {
            view.process_event(event, cx);
        }
        view.apply_prompt_process_probe(
            started,
            view.prompt_input_epoch,
            Ok(vec![crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                executable: "cmd.exe".into(),
                depth: 0,
            }]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn native_queued_prompt_allows_fast_next_submission(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let (session, _input, _events, proxy) = session::test_session_with_events();
        view.session = Some(session);
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>echo first");
        view.runtime_send_key(crate::runtime_api::RuntimeKey::Enter, Default::default(), 1, cx)
            .unwrap();
        let first = view.command_started;
        nebula_terminal::event_loop::StreamProcessor::default().feed(
            &mut *view.session.as_ref().unwrap().term.lock(),
            &proxy,
            b"\r\nfirst\r\nC:\\work>\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07pause",
        );
        view.runtime_send_key(crate::runtime_api::RuntimeKey::Enter, Default::default(), 1, cx)
            .unwrap();
        assert_eq!(
            view.suggest.last_committed, "pause",
            "a parsed prompt owns the next input even before UI delivery"
        );
        assert_ne!(view.command_started, first, "new command invalidates first command probes");
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

#[gpui::test]
fn native_queued_prompt_allows_fast_newline_paste(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let (session, _input, mut events, proxy) = session::test_session_with_events();
        view.session = Some(session);
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>echo first");
        view.runtime_send_key(crate::runtime_api::RuntimeKey::Enter, Default::default(), 1, cx)
            .unwrap();
        let first = view.command_started;
        nebula_terminal::event_loop::StreamProcessor::default().feed(
            &mut *view.session.as_ref().unwrap().term.lock(),
            &proxy,
            b"\r\nfirst\r\nC:\\work>\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07",
        );
        view.paste_now_impl("pause\r\n", false, cx);
        assert_ne!(view.command_started, first, "newline paste starts its own command boundary");
        while let Ok(event) = events.try_recv() {
            view.process_event(event, cx);
        }
        view.apply_prompt_process_probe(
            first,
            1,
            Ok(vec![crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                executable: "cmd.exe".into(),
                depth: 0,
            }]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert_eq!(
            view.suggest.last_committed, "echo first",
            "unconfirmed paste must not enter history"
        );
    });
}

#[gpui::test]
fn native_cmd_submission_starts_activity_without_osc_or_clink(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>ping -n 8 127.0.0.1");
        view.suggest.line_buf = "ping -n 8 127.0.0.1".into();
        view.commit_line(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert_eq!(view.suggest.pending_command_prompt.as_deref(), Some("C:\\work>"));
    });
}

#[gpui::test]
fn native_cmd_prompt_return_ends_non_agent_command(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        view.mark_command_running();
        feed(view, b"C:\\work>");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        view.apply_prompt_process_probe(
            view.command_started,
            view.prompt_input_epoch,
            Ok(vec![]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        assert!(view.suggest.pending_command_prompt.is_none());
    });
}

#[gpui::test]
fn native_cmd_internal_input_keeps_the_original_command_boundary(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        view.mark_command_running();
        let started = view.command_started;
        feed(view, b"Password: reply");
        view.suggest.line_buf = "reply".into();
        view.commit_line(cx);
        assert_eq!(view.suggest.pending_command_prompt.as_deref(), Some("C:\\work>"));
        assert_eq!(view.command_started, started);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

#[gpui::test]
fn native_cmd_empty_enter_does_not_start_activity(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>");
        view.commit_line(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn native_cmd_history_submission_uses_echo_without_typed_mirror(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>ping -n 8 127.0.0.1");
        view.commit_line(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert_eq!(view.suggest.last_committed, "ping -n 8 127.0.0.1");
    });
}

#[gpui::test]
fn native_cmd_changed_directory_prompt_ends_command(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        view.mark_command_running();
        feed(view, b"D:\\other>");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        view.apply_prompt_process_probe(
            view.command_started,
            view.prompt_input_epoch,
            Ok(vec![]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn native_cmd_alternate_screen_prompt_is_not_shell_completion(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        view.mark_command_running();
        feed(view, b"\x1b[?1049hC:\\work>");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

#[gpui::test]
fn native_cmd_unknown_process_snapshot_does_not_end_builtin_wait(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        view.mark_command_running();
        view.command_started = Some(std::time::Instant::now() - std::time::Duration::from_secs(5));
        view.session.as_mut().unwrap().shell_pid = u32::MAX;
        feed(view, b"Value: ");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

#[gpui::test]
fn native_cmd_runtime_enter_uses_the_same_submission_boundary(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>pause");
        view.runtime_send_key(crate::runtime_api::RuntimeKey::Enter, Default::default(), 1, cx)
            .unwrap();
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert_eq!(view.suggest.pending_command_prompt.as_deref(), Some("C:\\work>"));
    });
}

#[gpui::test]
fn native_cmd_runtime_prompt_captures_echo_before_submitting(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>");
        view.runtime_prompt("pause".into(), true, cx).unwrap();
        feed(view, b"pause");
        view.flush_pending_runtime_submit(cx);
        assert_eq!(view.suggest.pending_command_prompt.as_deref(), Some("C:\\work>"));
        feed(view, b"\r\nPress any key to continue . . .");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        feed(view, b"\r\nC:\\work>");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        view.apply_prompt_process_probe(
            view.command_started,
            view.prompt_input_epoch,
            Ok(vec![]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn native_cmd_prompt_probe_requires_current_successful_process_evidence(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        view.mark_command_running();
        let started = view.command_started;
        feed(view, b"C:\\work>");
        let root = crate::process_tree::ProcessEntry {
            pid: 1,
            parent_pid: 0,
            executable: "cmd.exe".into(),
            depth: 0,
        };
        let child = crate::process_tree::ProcessEntry {
            pid: 2,
            parent_pid: 1,
            executable: "python.exe".into(),
            depth: 1,
        };
        view.apply_prompt_process_probe(started, 0, Err("snapshot unavailable".into()), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        view.apply_prompt_process_probe(started, 0, Ok(vec![root.clone(), child]), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        view.apply_prompt_process_probe(None, 0, Ok(vec![root.clone()]), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running, "stale command result");
        feed(view, b"still executing");
        view.apply_prompt_process_probe(started, 0, Ok(vec![root.clone()]), cx);
        assert_eq!(
            view.sidebar_activity(),
            SidebarActivity::Running,
            "prompt changed during probe"
        );
        feed(view, b"\r\nC:\\work>");
        view.apply_prompt_process_probe(started, 0, Ok(vec![root]), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn native_cmd_input_invalidates_pending_prompt_probe(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        view.mark_command_running();
        let started = view.command_started;
        feed(view, b"C:\\work>");
        // The new input has reached the PTY, but its echo has not arrived yet.
        view.write_input(b"pause\r".to_vec(), cx);
        view.apply_prompt_process_probe(
            started,
            0,
            Ok(vec![crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                executable: "cmd.exe".into(),
                depth: 0,
            }]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

#[gpui::test]
fn native_marker_restores_prompt_despite_background_process(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.mark_command_running();
        let started = view.command_started;
        native_prompt(view, cx);
        view.apply_prompt_process_probe(
            started,
            0,
            Ok(vec![
                crate::process_tree::ProcessEntry {
                    pid: 1,
                    parent_pid: 0,
                    executable: "cmd.exe".into(),
                    depth: 0,
                },
                crate::process_tree::ProcessEntry {
                    pid: 2,
                    parent_pid: 1,
                    executable: "python.exe".into(),
                    depth: 1,
                },
            ]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn native_marker_disables_visible_prompt_guessing(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        native_prompt(view, cx);
        view.mark_command_running();
        view.suggest.pending_command_prompt = Some("C:\\work>".into());
        feed(view, b"C:\\work>");
        view.refresh_agent_screen_state(cx);
        assert_eq!(
            view.sidebar_activity(),
            SidebarActivity::Running,
            "set /p or removed PROMPT marker is not completion"
        );
    });
}

#[gpui::test]
fn native_marker_rejects_nested_shell_and_unknown_snapshot(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.mark_command_running();
        let started = view.command_started;
        native_prompt(view, cx);
        view.apply_prompt_process_probe(started, 0, Err("unavailable".into()), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        let root = crate::process_tree::ProcessEntry {
            pid: 1,
            parent_pid: 0,
            executable: "cmd.exe".into(),
            depth: 0,
        };
        view.apply_prompt_process_probe(
            started,
            0,
            Ok(vec![
                root.clone(),
                crate::process_tree::ProcessEntry {
                    pid: 2,
                    parent_pid: 1,
                    executable: "cmd.exe".into(),
                    depth: 1,
                },
            ]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        assert!(view.command_running, "outer command remains live");
        // The periodic process fallback must not undo a confirmed inner prompt.
        view.session.as_mut().unwrap().shell_pid = std::process::id();
        view.suggest.last_committed = "ping -n 5 127.0.0.1".into();
        view.command_started = Some(std::time::Instant::now() - std::time::Duration::from_secs(4));
        view.reconcile_shell_activity(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        view.command_started = started;
        // A fresh outer prompt after the nested shell exits can complete the run.
        native_prompt(view, cx);
        view.apply_prompt_process_probe(started, 0, Ok(vec![root]), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn native_nested_prompt_keeps_outer_run_and_agent_identity(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.mark_command_running();
        view.active_run = Some(crate::runtime_api::begin_runtime_run());
        let run_id = view.active_run.unwrap().run_id;
        let started = view.command_started;
        let processes = vec![
            crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                executable: "cmd.exe".into(),
                depth: 0,
            },
            crate::process_tree::ProcessEntry {
                pid: 2,
                parent_pid: 1,
                executable: "cmd.exe".into(),
                depth: 1,
            },
        ];
        native_prompt(view, cx);
        view.apply_prompt_process_probe(started, 0, Ok(processes.clone()), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        assert_eq!(view.active_run.unwrap().run_id, run_id);
        assert!(view.last_run.is_none());
        feed(view, b"C:\\work>pause");
        view.commit_line(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        view.running_program = Some("pi".into());
        let hook = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=pi pane=42\n{\"kind\":\"prompt\",\"session_id\":\"nested\",\"bridge_sequence\":1}",
            Some(42),
        ).unwrap();
        view.handle_ai_hook(&hook, cx);
        let started = view.command_started;
        native_prompt(view, cx);
        view.apply_prompt_process_probe(started, 0, Ok(processes), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert_eq!(view.running_program.as_deref(), Some("pi"));
        assert!(view.agent_activity.hook_seen());
        assert_eq!(view.active_run.unwrap().run_id, run_id);
    });
}

#[gpui::test]
fn native_nested_prompt_clears_cli_progress_without_ending_outer_run(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.mark_command_running();
        view.active_run = Some(crate::runtime_api::begin_runtime_run());
        let run_id = view.active_run.unwrap().run_id;
        let started = view.command_started;
        view.process_event(Event::Progress { state: 3, value: None }, cx);
        native_prompt(view, cx);
        view.apply_prompt_process_probe(
            started,
            0,
            Ok(vec![
                crate::process_tree::ProcessEntry {
                    pid: 1,
                    parent_pid: 0,
                    executable: "cmd.exe".into(),
                    depth: 0,
                },
                crate::process_tree::ProcessEntry {
                    pid: 2,
                    parent_pid: 1,
                    executable: "cmd.exe".into(),
                    depth: 1,
                },
            ]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        assert_eq!(view.progress, crate::taskbar::TaskProgress::None);
        assert_eq!(view.active_run.unwrap().run_id, run_id);
        assert!(view.last_run.is_none());
    });
}

#[gpui::test]
fn native_marker_pending_probe_cannot_finish_new_input(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.mark_command_running();
        let started = view.command_started;
        native_prompt(view, cx);
        view.write_input(b"pause\r".to_vec(), cx);
        view.apply_prompt_process_probe(started, 0, Ok(vec![]), cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

#[gpui::test]
fn native_prompt_survives_typing_before_process_probe_returns(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.mark_command_running();
        let started = view.command_started;
        native_prompt(view, cx);
        view.write_input(b"echo next".to_vec(), cx);
        let root = crate::process_tree::ProcessEntry {
            pid: 1,
            parent_pid: 0,
            executable: "cmd.exe".into(),
            depth: 0,
        };
        view.apply_prompt_process_probe(started, 0, Ok(vec![root.clone()]), cx);
        assert_eq!(
            view.sidebar_activity(),
            SidebarActivity::Running,
            "old input snapshot is discarded"
        );
        view.apply_prompt_process_probe(started, 1, Ok(vec![root]), cx);
        assert_eq!(
            view.sidebar_activity(),
            SidebarActivity::Idle,
            "typing without submitting must not lose the only prompt marker"
        );
    });
}

#[gpui::test]
fn native_queued_prompt_cannot_finish_encoded_submission(cx: &mut TestAppContext) {
    for bytes in [&b"pause\r"[..], &b"\x1b[13;28;13;1;0;1_\x1b[13;28;13;0;0;1_"[..]] {
        let (view, window, _) = open(cx);
        view.update(window, |view, cx| {
            view.mark_command_running();
            let started = view.command_started;
            native_prompt(view, cx);
            view.write_input(bytes.to_vec(), cx);
            view.apply_prompt_process_probe(
                started,
                view.prompt_input_epoch,
                Ok(vec![crate::process_tree::ProcessEntry {
                    pid: 1,
                    parent_pid: 0,
                    executable: "cmd.exe".into(),
                    depth: 0,
                }]),
                cx,
            );
            assert_eq!(view.sidebar_activity(), SidebarActivity::Running, "{bytes:?}");
        });
    }
}

#[gpui::test]
fn native_queued_prompt_survives_editing_keys(cx: &mut TestAppContext) {
    for bytes in [
        &b"\x08"[..],
        &b"\x1b[D"[..],
        &b"\x1b[8;14;8;1;0;1_\x1b[8;14;8;0;0;1_"[..],
        &b"\x1b[65;30;97;1;0;1_\x1b[65;30;97;0;0;1_"[..],
    ] {
        let (view, window, _) = open(cx);
        view.update(window, |view, cx| {
            let (session, _input, mut events, proxy) = session::test_session_with_events();
            view.session = Some(session);
            view.suggest.suggest_env = crate::display::SuggestEnv::Local;
            view.mark_command_running();
            let started = view.command_started;
            nebula_terminal::event_loop::StreamProcessor::default().feed(
                &mut *view.session.as_ref().unwrap().term.lock(),
                &proxy,
                b"C:\\work>\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07",
            );
            view.write_input(bytes.to_vec(), cx);
            while let Ok(event) = events.try_recv() {
                view.process_event(event, cx);
            }
            view.apply_prompt_process_probe(
                started,
                view.prompt_input_epoch,
                Ok(vec![crate::process_tree::ProcessEntry {
                    pid: 1,
                    parent_pid: 0,
                    executable: "cmd.exe".into(),
                    depth: 0,
                }]),
                cx,
            );
            assert_eq!(view.sidebar_activity(), SidebarActivity::Idle, "{bytes:?}");
        });
    }
}

#[gpui::test]
fn native_newline_paste_starts_command_without_recording_unconfirmed_history(
    cx: &mut TestAppContext,
) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>");
        native_prompt(view, cx);
        view.paste_now_impl("pause\r\n", false, cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert_eq!(view.suggest.pending_command_prompt.as_deref(), Some("C:\\work>"));
        assert!(view.suggest.last_committed.is_empty(), "paste is not yet echoed shell history");
    });
}

#[gpui::test]
fn native_paste_without_shell_submission_does_not_start_command(cx: &mut TestAppContext) {
    for (screen, pasted, bracketed) in [
        ("C:\\work>", "pause", false),
        ("C:\\work>", "\r\n", false),
        ("Password: ", "secret\r\n", false),
        ("C:\\work>", "pause\r\n", true),
    ] {
        let (view, window, _) = open(cx);
        view.update(window, |view, cx| {
            view.suggest.suggest_env = crate::display::SuggestEnv::Local;
            feed(view, screen.as_bytes());
            native_prompt(view, cx);
            if bracketed {
                feed(view, b"\x1b[?2004h");
            }
            view.paste_now_impl(pasted, false, cx);
            assert_eq!(
                view.sidebar_activity(),
                SidebarActivity::Idle,
                "{screen:?} {pasted:?} bracketed={bracketed}"
            );
        });
    }
}

#[gpui::test]
fn native_prompt_completion_clears_progress_without_another_enter(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"C:\\work>python synthetic-progress.py");
        view.commit_line(cx);
        let started = view.command_started;
        view.process_event(Event::Progress { state: 3, value: None }, cx);
        native_prompt(view, cx);
        view.apply_prompt_process_probe(
            started,
            0,
            Ok(vec![crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                executable: "cmd.exe".into(),
                depth: 0,
            }]),
            cx,
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        assert_eq!(view.progress, crate::taskbar::TaskProgress::None);
    });
}
