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

fn native_prompt(view: &mut TerminalView, cx: &mut Context<'_, TerminalView>) {
    view.session.as_ref().unwrap().native_prompt.observe_prompt();
    view.process_event(Event::UserVar { name: "pebrel_cmd_prompt".into(), value: "1".into() }, cx);
}

pub(super) fn feed(view: &mut TerminalView, bytes: &[u8]) {
    let mut term = view.session.as_ref().unwrap().term.lock();
    let mut parser = nebula_terminal::vte::ansi::Processor::<
        nebula_terminal::vte::ansi::StdSyncHandler,
    >::default();
    parser.advance(&mut *term, bytes);
}

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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
