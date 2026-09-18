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

fn open(cx: &mut TestAppContext) -> (Entity<TerminalView>, &mut VisualTestContext, Receiver<Msg>) {
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

fn feed(view: &mut TerminalView, bytes: &[u8]) {
    let mut term = view.session.as_ref().unwrap().term.lock();
    let mut parser = nebula_terminal::vte::ansi::Processor::<
        nebula_terminal::vte::ansi::StdSyncHandler,
    >::default();
    parser.advance(&mut *term, bytes);
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
