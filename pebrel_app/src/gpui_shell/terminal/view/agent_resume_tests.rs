use super::startup_tests::open;
use super::*;
use gpui::{TestAppContext, size};
use gpui_component::Root;
use pebrel_terminal::event::Event;

#[gpui::test]
fn agent_resume_and_fork_rules_append_quoted_arguments_and_respect_enabled_toggle(
    cx: &mut TestAppContext,
) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.exec_context = Some(crate::runtime_exec::PaneExecContext::from_pty_options(
            &pebrel_terminal::tty::Options {
                shell: Some(pebrel_terminal::tty::Shell::new("/bin/zsh".into(), vec![])),
                ..Default::default()
            },
        ));
        view.session_launch = crate::session::LaunchSession::Shell {
            name: "zsh".into(),
            program: "/bin/zsh".into(),
            args: Vec::new(),
        };
        cx.global_mut::<Settings>().agent_resume_args =
            pebrel_settings::AgentResumeArgs::from_raw(&pebrel_settings::RawSettings::from_text(
                "agent_resume_args_codex=--yolo\n\
                 agent_resume_enabled_codex=1\n\
                 agent_resume_args_claude=--dangerously-skip-permissions\n\
                 agent_resume_enabled_claude=1\n\
                 agent_resume_args_antigravity=--dangerously-skip-permissions\n\
                 agent_resume_enabled_antigravity=0\n",
            ));

        view.restore_agent(
            crate::session::AgentSession {
                source: "codex".into(),
                session_id: Some("session-1".into()),
                session_file: None,
            },
            cx,
        );
        assert_eq!(
            view.pending_shell_command.as_ref().map(|pending| pending.text.as_str()),
            Some("codex resume session-1 --yolo")
        );

        view.pending_shell_command = None;
        view.run_agent_session_command("claude --resume 'session-2' --fork-session".to_owned(), cx);
        assert_eq!(
            view.pending_shell_command.as_ref().map(|pending| pending.text.as_str()),
            Some("claude --resume 'session-2' --fork-session --dangerously-skip-permissions")
        );

        view.pending_shell_command = None;
        view.restore_agent(
            crate::session::AgentSession {
                source: "antigravity".into(),
                session_id: Some("conv-3".into()),
                session_file: None,
            },
            cx,
        );
        assert_eq!(
            view.pending_shell_command.as_ref().map(|pending| pending.text.as_str()),
            Some("agy --conversation conv-3")
        );

        // Ordinary non-resume/fork commands must keep exact arguments without rule decoration.
        view.pending_shell_command = None;
        view.run_command("codex --version".into(), cx);
        assert_eq!(
            view.pending_shell_command.as_ref().map(|pending| pending.text.as_str()),
            Some("codex --version")
        );

        view.pending_shell_command = None;
        cx.global_mut::<Settings>().agent_resume_args = pebrel_settings::AgentResumeArgs::from_raw(
            &pebrel_settings::RawSettings::from_text("agent_resume_args_claude=--yolo; echo bad\n"),
        );
        view.restore_agent(
            crate::session::AgentSession {
                source: "claude".into(),
                session_id: Some("session-4".into()),
                session_file: None,
            },
            cx,
        );
        assert!(view.pending_shell_command.is_none());
        assert!(view.recovery.failed);

        view.recovery.failed = false;
        view.run_agent_session_command("claude --resume 'session-5' --fork-session".into(), cx);
        assert!(view.pending_shell_command.is_none());
        assert!(view.recovery.failed);
        assert!(view.can_retry_without_rule());
        view.retry_without_rule(cx);
        assert_eq!(
            view.pending_shell_command.as_ref().map(|pending| pending.text.as_str()),
            Some("claude --resume 'session-5' --fork-session")
        );
    });

    // Blocked notifications and banners report agent, shell, and failure reason without
    // echoing the raw rule text (which may hold credentials or private paths).
    let notices = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let notices_sink = notices.clone();
    cx.update(crate::gpui_shell::scientific_render::init);
    let (_, painted_window) = cx.add_window_view(|window, cx| {
        cx.subscribe(&view, move |_, _, event, _| {
            if let TerminalViewEvent::Notification(crate::notify::Notification::Text {
                body, ..
            }) = event
            {
                notices_sink.borrow_mut().push(body.clone());
            }
        })
        .detach();
        Root::new(view.clone(), window, cx)
    });
    let secret_marker = "secret-marker-965";
    let invalid_rule = format!("--token={secret_marker}; echo bad");
    let raw_rule = format!(r#"["--config","api_token=\"{secret_marker}\""]"#);
    view.update(painted_window, |view, cx| {
        view.pending_shell_command = None;
        cx.global_mut::<Settings>().agent_resume_args = pebrel_settings::AgentResumeArgs::from_raw(
            &pebrel_settings::RawSettings::from_text(&format!(
                "agent_resume_args_claude={invalid_rule}\nagent_resume_enabled_claude=1\n"
            )),
        );
        view.restore_agent(
            crate::session::AgentSession {
                source: "claude".into(),
                session_id: Some("session-invalid-secret".into()),
                session_file: None,
            },
            cx,
        );
        let blocked = view.recovery.blocked_rule.as_ref().expect("blocked invalid rule");
        for lang in [crate::display::UiLanguage::EnUs, crate::display::UiLanguage::ZhCn] {
            let text = blocked.message(lang);
            assert!(text.contains("Claude Code"), "missing agent name: {text}");
            assert!(!text.contains(secret_marker), "leaked marker in invalid message: {text}");
            assert!(!text.contains('→'), "unshipped settings subpage referenced: {text}");
        }

        view.exec_context = Some(crate::runtime_exec::PaneExecContext::from_pty_options(
            &pebrel_terminal::tty::Options {
                shell: Some(pebrel_terminal::tty::Shell::new("cmd.exe".into(), vec![])),
                ..Default::default()
            },
        ));
        view.session_launch = crate::session::LaunchSession::Shell {
            name: "cmd".into(),
            program: "cmd.exe".into(),
            args: Vec::new(),
        };
        cx.global_mut::<Settings>().agent_resume_args =
            pebrel_settings::AgentResumeArgs::from_raw(&pebrel_settings::RawSettings::from_text(
                &format!("agent_resume_args_codex={raw_rule}\nagent_resume_enabled_codex=1\n"),
            ));
        view.restore_agent(
            crate::session::AgentSession {
                source: "codex".into(),
                session_id: Some("session-cmd".into()),
                session_file: None,
            },
            cx,
        );
        assert!(view.pending_shell_command.is_none());
        assert!(view.recovery.failed);
        assert!(view.can_retry_without_rule());
        let blocked = view.recovery.blocked_rule.as_ref().expect("blocked cmd rule");
        for lang in [crate::display::UiLanguage::EnUs, crate::display::UiLanguage::ZhCn] {
            let text = blocked.message(lang);
            assert!(
                text.contains("Codex") && text.contains("cmd.exe"),
                "missing agent or shell in message: {text}"
            );
            assert!(!text.contains(secret_marker), "leaked marker in cmd message: {text}");
            assert!(!text.contains('→'), "unshipped settings subpage referenced: {text}");
        }
    });
    assert_eq!(notices.borrow().len(), 2);
    for notice in notices.borrow().iter() {
        assert!(!notice.contains(secret_marker), "leaked marker in notification: {notice}");
        assert!(!notice.contains("api_token"), "leaked rule body in notification: {notice}");
    }
    let last_notice = notices.borrow().last().cloned().unwrap_or_default();
    assert!(
        last_notice.contains("Codex") && last_notice.contains("cmd.exe"),
        "unexpected notice: {last_notice}"
    );
    painted_window.simulate_resize(size(px(800.0), px(600.0)));
    painted_window.run_until_parked();
    painted_window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(painted_window.debug_bounds("blocked-resume-rule").is_some());
    let retry_btn = painted_window.debug_bounds("session-retry-without-rule").unwrap().center();
    painted_window.simulate_click(retry_btn, gpui::Modifiers::default());
    painted_window.run_until_parked();
    view.read_with(painted_window, |view, cx| {
        assert!(!view.recovery.failed);
        assert!(!view.can_retry_without_rule());
        assert_eq!(
            view.pending_shell_command.as_ref().map(|pending| pending.text.as_str()),
            Some("codex resume session-cmd")
        );
        assert_eq!(cx.global::<Settings>().agent_resume_args.get("codex"), raw_rule);
    });
}

#[gpui::test]
fn agent_resume_blocked_recovery_yields_to_manual_sessions_and_running_commands(
    cx: &mut TestAppContext,
) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.exec_context = Some(crate::runtime_exec::PaneExecContext::from_pty_options(
            &pebrel_terminal::tty::Options {
                shell: Some(pebrel_terminal::tty::Shell::new("/bin/zsh".into(), vec![])),
                ..Default::default()
            },
        ));
        view.session_launch = crate::session::LaunchSession::Shell {
            name: "zsh".into(),
            program: "/bin/zsh".into(),
            args: Vec::new(),
        };
        cx.global_mut::<Settings>().agent_resume_args = pebrel_settings::AgentResumeArgs::from_raw(
            &pebrel_settings::RawSettings::from_text(
                "agent_resume_args_codex=--yolo; echo bad\nagent_resume_enabled_codex=1\n",
            ),
        );

        view.restore_agent(
            crate::session::AgentSession {
                source: "codex".into(),
                session_id: Some("session-old".into()),
                session_file: None,
            },
            cx,
        );
        assert!(view.can_retry_without_rule());
        assert_eq!(
            view.session_agent().and_then(|agent| agent.session_id),
            Some("session-old".into())
        );

        // Retry is suppressed while a foreground command is running, even before
        // its program name is identified.
        view.command_running = true;
        assert!(view.running_program.is_none());
        assert!(!view.can_retry_without_rule());
        assert!(!view.can_retry_recovery());
        view.retry_without_rule(cx);
        assert!(view.pending_shell_command.is_none());
        view.command_running = false;
        assert!(view.can_retry_without_rule());

        // User manually opens another session in the same pane without a prior
        // shell CommandStart: the new session registers and replaces the old recovery.
        let manual_start = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=claude\n{\"hook_event_name\":\"SessionStart\",\"session_id\":\"session-manual\"}",
            Some(view.pane_id),
        )
        .unwrap();
        assert!(view.handle_ai_hook(&manual_start, cx));
        assert_eq!(
            view.ai_session
                .as_ref()
                .map(|session| (session.source.as_str(), session.session_id.as_str())),
            Some(("claude", "session-manual"))
        );
        assert_eq!(
            view.session_agent(),
            Some(crate::session::AgentSession {
                source: "claude".into(),
                session_id: Some("session-manual".into()),
                session_file: None,
            })
        );
        assert!(!view.can_retry_without_rule());
        assert!(view.recovery.blocked_rule.is_none());

        let manual_end = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=claude\n{\"hook_event_name\":\"SessionEnd\",\"session_id\":\"session-manual\"}",
            Some(view.pane_id),
        )
        .unwrap();
        assert!(view.handle_ai_hook(&manual_end, cx));
        assert!(view.ai_session.is_none());
        assert!(view.session_agent().is_none());
        assert!(!view.can_retry_without_rule());
        assert!(!view.can_retry_recovery());
        view.retry_without_rule(cx);
        assert!(view.pending_shell_command.is_none());

        // Explicit shell command execution also supersedes a blocked recovery immediately.
        view.restore_agent(
            crate::session::AgentSession {
                source: "codex".into(),
                session_id: Some("session-old-2".into()),
                session_file: None,
            },
            cx,
        );
        assert!(view.can_retry_without_rule());
        view.suggest.last_committed = "echo manual-command".into();
        view.process_event(Event::CommandStart, cx);
        assert!(!view.can_retry_without_rule());
        assert!(view.recovery.blocked_rule.is_none());
        assert!(view.recovery.target.is_none());
        view.process_event(
            Event::CommandDone { exit_code: Some(0), empty_prompt_zone: false },
            cx,
        );
        assert!(!view.can_retry_without_rule());
        assert!(!view.can_retry_recovery());
    });
}
