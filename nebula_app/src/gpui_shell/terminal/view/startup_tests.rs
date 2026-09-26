use super::*;
use gpui::{Entity, TestAppContext, VisualTestContext, size};
use gpui_component::Root;
use nebula_terminal::event::Event;
use std::sync::mpsc::Receiver;

// Keep the terminal entity off the layout tree so tests control each viewport
// and PTY event explicitly, while using real GPUI tasks and terminal parsing.
struct Surface;
impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

pub(super) fn open(
    cx: &mut TestAppContext,
) -> (Entity<TerminalView>, &mut VisualTestContext, Receiver<Msg>) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut result = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            TerminalView::new(
                42,
                (80, 24),
                TerminalLaunch::Local {
                    cwd: None,
                    shell: Some(nebula_terminal::tty::Shell::new(
                        "pebrel-test-missing-shell-executable".into(),
                        vec![],
                    )),
                    shell_name: None,
                },
                window,
                cx,
            )
        });
        let receiver = view.update(cx, |view, _| {
            let (session, receiver) = session::test_session();
            view.session = Some(session);
            view.error = None;
            view.exited = None;
            view.exec_context = None;
            view.suggest.suggest_env = crate::display::SuggestEnv::Wsl { distro: "Debian".into() };
            receiver
        });
        result = Some((view, receiver));
        Root::new(cx.new(|_| Surface), window, cx)
    });
    let (view, receiver) = result.unwrap();
    (view, window, receiver)
}

pub(super) fn feed(view: &mut TerminalView, bytes: &[u8]) {
    let mut term = view.session.as_ref().unwrap().term.lock();
    let mut parser = nebula_terminal::vte::ansi::Processor::<
        nebula_terminal::vte::ansi::StdSyncHandler,
    >::default();
    parser.advance(&mut *term, bytes);
}

#[gpui::test]
fn ended_command_does_not_leave_osc_progress_running(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        for exit_code in [Some(0), Some(1), None] {
            view.process_event(Event::CommandStart, cx);
            view.process_event(Event::Progress { state: 3, value: None }, cx);
            assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
            view.process_event(Event::CommandDone { exit_code }, cx);
            let expected = if exit_code.is_some_and(|code| code != 0) {
                SidebarActivity::CommandFailed
            } else {
                SidebarActivity::Idle
            };
            assert_eq!(view.sidebar_activity(), expected);
            assert_eq!(view.progress, crate::taskbar::TaskProgress::None);
        }
    });
}

#[gpui::test]
fn ligature_changes_update_all_faces_without_replacing_the_open_session(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        feed(view, b"a->b != c");
        let term = view.session.as_ref().unwrap().term.clone();
        for enabled in [false, true] {
            cx.global_mut::<Settings>().ligatures = enabled;
            view.apply_settings(cx);
            assert_eq!(view.ligatures, enabled);
            assert!(std::sync::Arc::ptr_eq(&term, &view.session.as_ref().unwrap().term));
            for font in [&view.font, &view.font_bold, &view.font_italic, &view.font_bold_italic] {
                for tag in ["calt", "liga", "clig"] {
                    assert!(
                        font.features.tag_value_list().contains(&(tag.into(), u32::from(enabled)))
                    );
                }
            }
            assert_eq!(
                term.lock().grid()[nebula_terminal::index::Line(0)]
                    [nebula_terminal::index::Column(1)]
                .c,
                '-'
            );
        }
    });
}

#[gpui::test]
fn ssh_tab_name_and_hover_preserve_host_identity_across_remote_titles(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.ssh_destination = Some("root@192.0.2.10:2222".into());
        view.ssh_label = Some("SG-1 新加坡".into());
        view.process_event(Event::CwdReport("/srv/project".into()), cx);
        view.process_event(Event::Title("NEBULA|/srv/project|main|htop".into()), cx);
        assert_eq!(view.tab_label(), "SG-1 新加坡");
        assert_eq!(
            view.tab_tooltip("SG-1 新加坡"),
            "SG-1 新加坡\nroot@192.0.2.10:2222\n/srv/project\nhtop"
        );
        assert!(
            view.tab_tooltip("手动命名").starts_with("手动命名\nSG-1 新加坡\nroot@192.0.2.10:2222")
        );
        view.ssh_label = None;
        assert_eq!(view.tab_label(), "root@192.0.2.10:2222");
    });
}

#[gpui::test]
fn ai_tab_hover_shows_full_directory_and_reported_task_but_not_stale_task(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.process_event(Event::CwdReport("/home/test/很长的项目目录".into()), cx);
        view.running_program = Some("codex".into());
        view.process_event(Event::Title("修复 SSH 标签名称".into()), cx);
        assert_eq!(view.tab_label(), "很长的项目目录");
        let hover = view.tab_tooltip(&view.tab_label());
        assert!(hover.contains("/home/test/很长的项目目录"));
        assert!(hover.contains("修复 SSH 标签名称"));
        view.process_event(Event::CommandDone { exit_code: Some(0) }, cx);
        assert!(!view.tab_tooltip(&view.tab_label()).contains("修复 SSH 标签名称"));
    });
}

#[gpui::test]
fn review_regression_cold_resume_survives_initial_prompt_and_clears_on_exit(
    cx: &mut TestAppContext,
) {
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| view.update(cx, |view, cx| {
        view.run_command("codex resume saved-42".into(), cx);
        view.seed_ai_session("codex".into(), "saved-42".into(), cx);
        assert!(receiver.try_recv().is_err(), "do not submit into shell initialization");
        assert_eq!(view.session_agent().unwrap().session_id.as_deref(), Some("saved-42"));
        view.process_event(Event::CommandDone { exit_code: Some(0) }, cx);
        assert!(view.pending_shell_command.is_some());
        feed(view, b"\x1b]133;A\x07hello@host:/home/hello$ ");
        view.process_event(Event::Wakeup, cx);
        assert!(view.pending_shell_command.is_none());
        assert_eq!(view.running_program.as_deref(), Some("codex"));
        assert!(view.ai_session.is_none(), "submission is not confirmation");
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes) if bytes.as_ref() == b"codex resume saved-42"));
        // The initial shell edge cannot consume the pending Enter or identity.
        view.process_event(Event::CommandDone { exit_code: Some(0) }, cx);
        assert!(view.recovery.awaiting_confirmation);
        feed(view, b"codex resume saved-42");
        view.flush_pending_runtime_submit(cx);
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes) if bytes.as_ref() == b"\r"));
        view.process_event(Event::CommandStart, cx);
        assert_eq!(view.runtime_agent().unwrap().kind, "codex");
        let mut event = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=codex\n{\"type\":\"agent-turn-complete\",\"thread-id\":\"saved-42\"}", Some(view.pane_id)
        ).expect("native hook");
        event.pane = Some(view.pane_id);
        assert!(view.handle_ai_hook(&event, cx));
        assert_eq!(view.ai_session.as_ref().unwrap().session_id, "saved-42");
        view.process_event(Event::CommandDone { exit_code: Some(0) }, cx);
        assert!(view.running_program.is_none());
        assert!(view.ai_session.is_none(), "both foreground fields must clear together");
        assert!(view.runtime_agent().is_none());
    }));
}

#[gpui::test]
fn pi_cancelled_turn_becomes_idle_instead_of_completed(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        for payload in [
            r#"{"kind":"prompt","session_id":"pi-cancelled-test"}"#,
            r#"{"kind":"done","stop_reason":"aborted","session_id":"pi-cancelled-test"}"#,
        ] {
            let wire = format!("nebula-hook/1 source=pi\n{payload}");
            let event = crate::ai_hook::parse_remote_envelope(wire.as_bytes(), Some(view.pane_id))
                .expect("Pi hook");
            assert!(view.handle_ai_hook(&event, cx));
        }
        assert_eq!(view.agent_activity.status(), crate::ai_agents::AgentStatus::Idle);
    });
}

#[gpui::test]
fn pi_outcomes_emit_only_the_matching_notification(cx: &mut TestAppContext) {
    use std::cell::RefCell;
    use std::rc::Rc;

    let (view, window, _) = open(cx);
    let notifications = Rc::new(RefCell::new(Vec::new()));
    let observed = notifications.clone();
    let _subscription = view.update(window, |_, cx| {
        cx.subscribe(&view, move |_, _, event, _| {
            if let TerminalViewEvent::Notification(notification) = event {
                observed.borrow_mut().push(notification.clone());
            }
        })
    });
    for (reason, status, expected_count) in [
        ("aborted", crate::ai_agents::AgentStatus::Idle, 0),
        ("unknown", crate::ai_agents::AgentStatus::Idle, 1),
        ("error", crate::ai_agents::AgentStatus::Idle, 2),
        ("stop", crate::ai_agents::AgentStatus::Done, 3),
    ] {
        view.update(window, |view, cx| {
            for payload in [
                serde_json::json!({"kind": "prompt", "session_id": "pi-test"}),
                serde_json::json!({"kind": "done", "stop_reason": reason, "session_id": "pi-test"}),
            ] {
                let wire = format!("nebula-hook/1 source=pi\n{payload}");
                let event =
                    crate::ai_hook::parse_remote_envelope(wire.as_bytes(), Some(view.pane_id))
                        .expect("Pi hook");
                assert!(view.handle_ai_hook(&event, cx));
            }
            assert_eq!(view.agent_activity.status(), status);
        });
        assert_eq!(notifications.borrow().len(), expected_count, "{reason}");
    }
    let notifications = notifications.borrow();
    assert!(matches!(
        &notifications[1],
        crate::notify::Notification::AiTurnIssue {
            outcome: crate::ai_hook::AiTurnOutcome::Failed,
            ..
        }
    ));
    assert!(!notifications[1].is_attention());
}

#[gpui::test]
fn failed_cold_resume_keeps_target_for_retry(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            let saved = crate::session::AgentSession {
                source: "codex".into(),
                session_id: Some("saved-42".into()),
                session_file: None,
            };
            view.restore_agent(saved.clone(), cx);
            feed(view, b"\x1b]133;A\x07hello@host:/home/hello$ ");
            view.process_event(Event::Wakeup, cx);
            assert!(receiver.try_recv().is_ok());
            feed(view, b"codex resume saved-42");
            view.flush_pending_runtime_submit(cx);
            view.process_event(Event::CommandStart, cx);
            feed(view, b"\r\nNo session found matching 'saved-42'\r\n");
            view.process_event(Event::CommandDone { exit_code: Some(1) }, cx);
            assert!(view.ai_session.is_none());
            assert_eq!(view.session_agent(), Some(saved));
            assert!(view.recovery.awaiting_confirmation);
            assert!(view.recovery_pending(), "failure must not acknowledge the update ticket");
            assert!(!view.recovery_ready());
            assert!(view.can_retry_recovery());
            assert!(view.ai_fork_command().is_none());
        })
    });
}

#[gpui::test]
fn review_regression_saved_identity_without_a_resume_command_is_not_live(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.seed_ai_session("codex".into(), "saved-42".into(), cx);
            assert!(view.ai_session.is_none());
            view.run_command("codex resume saved-42".into(), cx);
            feed(view, b"Password: ");
            view.process_event(Event::Wakeup, cx);
            assert!(
                view.pending_shell_command.is_some(),
                "never send a command into authentication"
            );
            assert!(view.running_program.is_none());
        })
    });
}

#[gpui::test]
fn review_regression_quiet_startup_and_maximize_deliver_the_latest_pty_size(
    cx: &mut TestAppContext,
) {
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_layout(
                point(px(0.0), px(0.0)),
                px(10.0),
                px(20.0),
                size(px(1200.0), px(700.0)),
                1.0,
                cx,
            );
            view.set_layout(
                point(px(0.0), px(0.0)),
                px(10.0),
                px(20.0),
                size(px(1600.0), px(900.0)),
                1.0,
                cx,
            );
            assert!(receiver.try_recv().is_err(), "startup waits for final layout");
        })
    });
    window.run_until_parked();
    window.executor().advance_clock(TerminalView::STARTUP_GRID_GRACE);
    window.run_until_parked();
    let sizes: Vec<_> = receiver
        .try_iter()
        .filter_map(|message| match message {
            Msg::Resize(size) => Some((size.num_cols, size.num_lines)),
            _ => None,
        })
        .collect();
    assert_eq!(sizes, [(160, 45)], "one final resize even without a new terminal frame");
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.mark_structural_resize();
            view.set_layout(
                point(px(0.0), px(0.0)),
                px(10.0),
                px(20.0),
                size(px(800.0), px(500.0)),
                1.0,
                cx,
            );
        })
    });
    assert!(receiver.try_iter().any(|message| matches!(message, Msg::Resize(size) if size.num_cols == 80 && size.num_lines == 25)));
}

#[gpui::test]
fn shutdown_waits_for_missing_native_identity_but_keeps_a_known_target(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.running_program = Some("pi".into());
            assert!(view.ai_session_save_pending());
            view.seed_ai_session("pi".into(), "native-id".into(), cx);
            assert!(!view.ai_session_save_pending(), "the saved target survives a failed refresh");
            assert!(view.recovery_pending(), "durable target is not a live acknowledgement");
        })
    });
}

#[test]
fn different_native_session_cannot_confirm_or_erase_a_pending_resume() {
    use super::startup_command::SessionRecovery;
    let saved = crate::session::AgentSession {
        source: "pi".into(),
        session_id: Some("saved".into()),
        session_file: None,
    };
    let mut recovery = SessionRecovery::default();
    recovery.target = Some(saved.clone());
    recovery.awaiting_confirmation = true;
    assert!(!recovery.confirm(crate::session::AgentSession {
        session_id: Some("unrelated".into()),
        ..saved.clone()
    }));
    assert_eq!(recovery.target, Some(saved.clone()));
    assert!(recovery.awaiting_confirmation);
    assert!(recovery.confirm(saved));
    recovery.command_ended();
    assert!(recovery.target.is_none(), "intentional exit must not resurrect the conversation");
}

#[test]
fn native_acknowledgements_keep_known_files_when_a_bridge_omits_the_path() {
    use super::startup_command::SessionRecovery;
    for source in ["claude", "codex", "pi", "omp", "gemini", "opencode", "kimi"] {
        let saved = crate::session::AgentSession {
            source: source.into(),
            session_id: Some("saved-conversation".into()),
            session_file: Some("/sessions/conversation.jsonl".into()),
        };
        let mut recovery = SessionRecovery::default();
        recovery.target = Some(saved.clone());
        recovery.awaiting_confirmation = true;
        let mut acknowledgement = saved.clone();
        acknowledgement.session_file = Some("/sessions/unrelated.jsonl".into());
        assert!(!recovery.confirm(acknowledgement.clone()), "{source}: conflicting file");
        acknowledgement.session_file = None;
        acknowledgement.session_id = Some("unrelated".into());
        assert!(!recovery.confirm(acknowledgement.clone()), "{source}: conflicting identity");
        acknowledgement.session_id = saved.session_id.clone();
        assert!(recovery.confirm(acknowledgement), "{source}: native identity matches");
        assert_eq!(recovery.target, Some(saved));
        assert!(!recovery.awaiting_confirmation);
    }
}

#[gpui::test]
fn cold_resume_normalizes_codex_identity_before_waiting_for_native_confirmation(
    cx: &mut TestAppContext,
) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let thread = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c";
        let path = format!(r"C:\Users\user\.codex\sessions\rollout-date-{thread}.jsonl");
        view.restore_agent(
            crate::session::AgentSession {
                source: "codex".into(),
                session_id: Some("obsolete-hook-group".into()),
                session_file: Some(path.clone()),
            },
            cx,
        );
        let target = view.session_agent().unwrap();
        assert_eq!(target.session_id.as_deref(), Some(thread));
        assert_eq!(target.resume_command(), Some(format!("codex resume {thread}")));
        let event = crate::ai_hook::parse_remote_envelope(
            format!(
                "nebula-hook/1 source=codex codex_hooks=full\n{}",
                serde_json::json!({
                    "hook_event_name": "SessionStart", "session_id": "new-runtime-group",
                    "transcript_path": path,
                })
            )
            .as_bytes(),
            Some(view.pane_id),
        )
        .unwrap();
        assert!(view.handle_ai_hook(&event, cx));
        assert!(!view.recovery_pending());
        assert_eq!(view.session_agent().unwrap(), target);
    });
}

#[gpui::test]
fn cold_resume_of_supported_clis_waits_for_the_shell_and_preserves_each_target(
    cx: &mut TestAppContext,
) {
    for (source, expected) in [
        ("claude", "claude --resume saved-conversation"),
        ("codex", "codex resume saved-conversation"),
        ("gemini", "gemini --resume saved-conversation"),
        ("opencode", "opencode --session saved-conversation"),
        ("amp", "amp threads continue saved-conversation"),
        ("cursor", "agent --resume=saved-conversation"),
        ("copilot", "copilot --resume saved-conversation"),
        ("grok", "grok --resume saved-conversation"),
        ("omp", "omp --resume saved-conversation"),
        ("kimi", "kimi --session saved-conversation"),
    ] {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            let saved = crate::session::AgentSession {
                source: source.into(),
                session_id: Some("saved-conversation".into()),
                session_file: None,
            };
            view.restore_agent(saved.clone(), cx);
            assert_eq!(view.session_agent(), Some(saved.clone()), "{source}");
            assert!(view.ai_session.is_none(), "queuing is not a provider acknowledgement");
            assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_))));
            feed(view, b"\x1b]133;A\x07user@host:~$ ");
            view.flush_pending_shell_command(cx);
            assert!(
                receiver.try_iter().any(|message| {
                    matches!(message, Msg::Input(bytes) if bytes.as_ref() == expected.as_bytes())
                }),
                "{source}: exact saved conversation submitted when the prompt is ready"
            );
            assert_eq!(view.session_agent(), Some(saved));
            assert!(view.recovery_pending());
        });
    }
}

#[test]
fn file_only_recovery_requires_the_provider_to_confirm_that_file() {
    use super::startup_command::SessionRecovery;
    let saved = crate::session::AgentSession {
        source: "pi".into(),
        session_id: None,
        session_file: Some("/sessions/selected.jsonl".into()),
    };
    let mut recovery = SessionRecovery::default();
    recovery.target = Some(saved.clone());
    recovery.awaiting_confirmation = true;
    assert!(!recovery.confirm(crate::session::AgentSession {
        source: "pi".into(),
        session_id: Some("unrelated".into()),
        session_file: None,
    }));
    assert!(recovery.awaiting_confirmation);
    assert_eq!(recovery.target, Some(saved.clone()));
    assert!(recovery.confirm(crate::session::AgentSession {
        session_id: Some("selected-native-id".into()),
        ..saved
    }));
}

#[gpui::test]
fn cold_resume_preserves_codex_id_with_a_legacy_native_file(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        let event = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=codex codex_hooks=full\n{\"hook_event_name\":\"SessionStart\",\"session_id\":\"saved-thread\",\"session_file\":\"/sessions/conversation.jsonl\"}",
            Some(view.pane_id),
        ).unwrap();
        let reported = crate::session::AgentSession {
            source: event.source,
            session_id: event.session_id,
            session_file: event.session_file,
        };
        let saved = serde_json::from_str::<crate::session::AgentSession>(
            &serde_json::to_string(&reported).unwrap(),
        ).unwrap();
        view.restore_agent(saved.clone(), cx);
        assert_eq!(view.session_agent(), Some(saved.clone()));
        assert!(view.pending_shell_command.is_some());
        assert!(!view.can_retry_recovery(), "a known ID must not become a failed restore");
        assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_))));
        feed(view, b"\x1b]133;A\x07user@host:~$ ");
        view.flush_pending_shell_command(cx);
        assert!(receiver.try_iter().any(|message| {
            matches!(message, Msg::Input(bytes) if bytes.as_ref() == b"codex resume saved-thread")
        }));
        assert_eq!(view.session_agent(), Some(saved));
        assert!(view.recovery_pending(), "submission still waits for provider confirmation");
    });
}

fn submit_codex_restore(
    view: &mut TerminalView,
    receiver: &Receiver<Msg>,
    cx: &mut Context<TerminalView>,
) -> crate::session::AgentSession {
    let saved = crate::session::AgentSession {
        source: "codex".into(),
        session_id: Some("saved-missing".into()),
        session_file: None,
    };
    view.restore_agent(saved.clone(), cx);
    feed(view, b"\x1b]133;A\x07user@host:~$ ");
    view.flush_pending_shell_command(cx);
    feed(view, b"codex resume saved-missing");
    view.flush_pending_runtime_submit(cx);
    view.process_event(Event::CommandStart, cx);
    receiver.try_iter().for_each(drop);
    saved
}

#[gpui::test]
fn missing_codex_target_opens_the_native_chooser_and_confirms_the_users_choice(
    cx: &mut TestAppContext,
) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        let saved = submit_codex_restore(view, &receiver, cx);
        feed(view, b"\r\nERROR: No saved session found with ID saved-missing. Run codex resume without an ID.\r\n");
        view.process_event(Event::CommandDone { exit_code: Some(1) }, cx);
        assert_eq!(view.session_agent(), Some(saved));
        assert!(view.pending_shell_command.is_some());
        assert!(receiver.try_iter().all(|message| !matches!(message, Msg::Input(_))));
        feed(view, b"\x1b]133;A\x07user@host:~$ ");
        view.process_event(Event::Wakeup, cx);
        assert!(receiver.try_iter().any(|message| {
            matches!(message, Msg::Input(bytes) if bytes.as_ref() == b"codex resume")
        }));
        assert!(view.recovery_pending(), "opening a chooser is not a recovered conversation");
        let selected = crate::session::AgentSession {
            source: "codex".into(),
            session_id: Some("0199a213-c2a4-7cf5-8f6b-d746fbb6e86c".into()),
            session_file: Some("/sessions/rollout-date-0199a213-c2a4-7cf5-8f6b-d746fbb6e86c.jsonl".into()),
        };
        assert!(!view.recovery.accepts(&crate::session::AgentSession {
            source: "claude".into(),
            ..selected.clone()
        }));
        let event = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=codex codex_hooks=full\n{\"hook_event_name\":\"SessionStart\",\"session_id\":\"hook-group\",\"transcript_path\":\"/sessions/rollout-date-0199a213-c2a4-7cf5-8f6b-d746fbb6e86c.jsonl\"}",
            Some(view.pane_id),
        ).unwrap();
        let mut old_end = event.clone();
        old_end.kind = crate::ai_hook::AiHookKind::SessionEnd;
        assert!(!view.handle_ai_hook(&old_end, cx), "a delayed session end is not the user's choice");
        let mut foreign_start = event.clone();
        foreign_start.source = "claude".into();
        foreign_start.session_id = None;
        foreign_start.session_file = None;
        assert!(!view.handle_ai_hook(&foreign_start, cx));
        assert!(view.recovery_pending());
        assert!(view.handle_ai_hook(&event, cx));
        assert_eq!(view.session_agent(), Some(selected));
        assert!(!view.recovery_pending());
    });
}

#[gpui::test]
fn unrelated_codex_failures_do_not_replace_the_saved_target(cx: &mut TestAppContext) {
    for (message, exit_code) in [
        ("ERROR: No saved session found with ID somebody-else.", Some(1)),
        ("ERROR: network unavailable", Some(1)),
        ("ERROR: No saved session found with ID saved-missing.", Some(0)),
        ("ERROR: network unavailable", None),
    ] {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            let saved = submit_codex_restore(view, &receiver, cx);
            feed(view, format!("\r\n{message}\r\n").as_bytes());
            view.process_event(Event::CommandDone { exit_code }, cx);
            assert!(view.pending_shell_command.is_none(), "{message}: {exit_code:?}");
            assert_eq!(view.session_agent(), Some(saved));
        });
    }
}

#[gpui::test]
fn cmd_prompt_completion_without_exit_code_still_offers_the_missing_conversation(
    cx: &mut TestAppContext,
) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        let saved = submit_codex_restore(view, &receiver, cx);
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        feed(view, b"\r\nERROR: No saved session found with ID saved-missing.\r\n");
        view.session.as_ref().unwrap().native_prompt.observe_prompt();
        view.apply_prompt_process_probe(
            view.command_started,
            view.prompt_input_epoch,
            Ok(vec![crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                executable: "cmd.exe".into(),
                depth: 0,
            }]),
            cx,
        );
        assert_eq!(view.session_agent(), Some(saved));
        assert!(
            view.pending_shell_command.is_some(),
            "the real CMD completion route queues the chooser"
        );
        assert!(view.recovery_pending());
    });
}

#[gpui::test]
fn a_failed_restore_can_choose_a_conversation_without_recognizing_an_error_string(
    cx: &mut TestAppContext,
) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        let saved = submit_codex_restore(view, &receiver, cx);
        assert!(!view.can_choose_recovery_session(), "cannot inject a chooser into a running CLI");
        feed(view, "\r\n未找到指定会话\r\n".as_bytes());
        view.finish_foreground_command(None, cx);
        assert!(view.can_choose_recovery_session());
        view.choose_recovery_session(cx);
        assert!(view.pending_shell_command.is_some());
        assert_eq!(view.session_agent(), Some(saved));
        assert!(view.recovery_pending());
    });
}

#[gpui::test]
fn lifecycle_only_hook_cannot_replace_a_verified_rollout_id(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let id = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c";
        let target = crate::session::AgentSession { source: "codex".into(), session_id: Some(id.into()),
            session_file: Some(format!("C:/sessions/rollout-date-{id}.jsonl")) };
        assert!(view.recovery.confirm(target.clone()));
        view.ai_session = Some(crate::display::AiSessionIdentity { source: "codex".into(), session_id: id.into() });
        view.ai_session_from_probe = true;
        view.running_program = Some("codex".into());
        view.agent_activity.begin_command(true);
        let event = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=codex codex_hooks=full\n{\"hook_event_name\":\"SessionStart\",\"session_id\":\"runtime-group\"}", Some(view.pane_id),
        ).unwrap();
        assert!(view.handle_ai_hook(&event, cx));
        assert_eq!(view.session_agent(), Some(target));
        assert_eq!(view.ai_session.as_ref().unwrap().session_id, id);
    });
}

#[gpui::test]
fn a_failed_codex_chooser_does_not_start_an_automatic_retry_loop(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        let saved = submit_codex_restore(view, &receiver, cx);
        let error = b"\r\nERROR: No saved session found with ID saved-missing.\r\n";
        feed(view, error);
        view.process_event(Event::CommandDone { exit_code: Some(1) }, cx);
        feed(view, b"\x1b]133;A\x07user@host:~$ ");
        view.flush_pending_shell_command(cx);
        feed(view, b"codex resume");
        view.flush_pending_runtime_submit(cx);
        view.process_event(Event::CommandStart, cx);
        receiver.try_iter().for_each(drop);
        feed(view, error);
        view.process_event(Event::CommandDone { exit_code: Some(1) }, cx);
        assert!(view.pending_shell_command.is_none());
        assert_eq!(view.session_agent(), Some(saved));
        assert!(view.can_retry_recovery());
        assert!(receiver.try_iter().all(|message| !matches!(message, Msg::Input(_))));
    });
}
