//! Exercise exit routing, retained diagnostics and recovery through the real workspace.

use super::*;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    super::draw(cx);
}

fn draw_controls(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        // GPUI replays cached paint/listeners, but not debug selector bounds.
        // Refresh only for control geometry; output assertions use normal draws.
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn exit_status(code: u32) -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        std::process::ExitStatus::from_raw((code as i32) << 8)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt as _;
        std::process::ExitStatus::from_raw(code)
    }
}

fn exit(view: &Entity<TerminalView>, code: u32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.remote_files_test_event(Event::ChildExit(exit_status(code)), cx);
        view.remote_files_test_event(Event::Exit, cx);
    });
    draw(cx);
}

#[gpui::test]
fn failed_startup_keeps_last_tab_diagnostics_and_shell_choice(cx: &mut TestAppContext) {
    let (workspace, terminals, _, mut window) = open_painted_workspace(&[1], cx);
    let terminal = &terminals[0];
    terminal.update(&mut window, |view, _| {
        feed(view, b"\r\nfixture launcher: distribution does not exist\r\n");
    });
    let launch = terminal.read_with(&window, |view, _| view.session_launch.clone());
    exit(terminal, 1, &mut window);
    assert!(terminal.read_with(&window, |view, _| view.local_session_failed()));
    let output = painted(terminal, &mut window);
    // PaintedSnapshot concatenates painted cells, excluding blank cells.
    assert!(output.text.contains("distributiondoesnotexist"), "painted: {:?}", output.text);
    workspace.update(&mut window, |workspace, cx| {
        assert_eq!(workspace.tabs.len(), 1);
        assert!(workspace.locate_pane(terminal.entity_id()).is_some());
        assert_eq!(terminal.read(cx).session_launch, launch);
        // Retaining a view must still terminate runtime waiters for this process.
        let snapshot =
            workspace.runtime_hub.publish(crate::runtime_api::RuntimeSnapshot::new(0, Vec::new()));
        assert_eq!(snapshot.pane_lifecycles.len(), 1);
        assert_eq!(snapshot.pane_lifecycles[0].pane_id, terminal.read(cx).pane_id);
        assert_eq!(
            snapshot.pane_lifecycles[0].event,
            crate::runtime_api::RuntimePaneLifecycleKind::Exited,
        );
    });

    for width in [420.0, 1000.0] {
        window.simulate_resize(size(px(width), px(700.0)));
        draw_controls(&mut window);
        let banner = window.debug_bounds("failed-session").unwrap();
        for selector in ["failed-session-choose-shell", "failed-session-close"] {
            let bounds = window.debug_bounds(selector).unwrap();
            assert!(bounds.size.height >= px(32.0));
            assert!(banner.contains(&bounds.origin) && banner.contains(&bounds.bottom_right()));
        }
    }
    let choose = window.debug_bounds("failed-session-choose-shell").unwrap().center();
    window.simulate_mouse_move(choose, None, Modifiers::default());
    draw(&mut window);
    window.simulate_click(choose, Modifiers::default());
    draw(&mut window);
    workspace.read_with(&window, |workspace, _| {
        assert!(workspace.shell_picker_open && workspace.command_palette_open);
        assert_eq!(workspace.tabs.len(), 1, "choosing a shell is explicit, not an automatic retry");
    });
    window.simulate_keystrokes("escape");
    workspace.read_with(&window, |workspace, _| assert!(!workspace.command_palette_open));
    window.simulate_keystrokes("ctrl-k");
    workspace.read_with(&window, |workspace, _| {
        assert!(workspace.shell_picker_open && workspace.command_palette_open);
    });
}

#[gpui::test]
fn successful_exit_still_closes_the_tab(cx: &mut TestAppContext) {
    let (workspace, terminals, _, mut window) = open_painted_workspace(&[1, 1], cx);
    exit(&terminals[0], 0, &mut window);
    workspace.read_with(&window, |workspace, _| {
        assert_eq!(workspace.tabs.len(), 1);
        assert!(workspace.locate_pane(terminals[0].entity_id()).is_none());
        assert!(workspace.locate_pane(terminals[1].entity_id()).is_some());
    });
}

#[gpui::test]
fn nonzero_exit_after_user_input_still_closes_the_tab(cx: &mut TestAppContext) {
    let (workspace, terminals, _, mut window) = open_painted_workspace(&[1, 1], cx);
    window.simulate_keystrokes("x");
    exit(&terminals[0], 1, &mut window);
    workspace.read_with(&window, |workspace, _| {
        assert_eq!(workspace.tabs.len(), 1);
        assert!(workspace.locate_pane(terminals[0].entity_id()).is_none());
    });
}

#[gpui::test]
fn nonzero_exit_at_a_reported_prompt_is_not_a_startup_failure(cx: &mut TestAppContext) {
    let (workspace, terminals, _, mut window) = open_painted_workspace(&[1, 1], cx);
    terminals[0].update(&mut window, |view, _| {
        // The stream processor applies OSC 133;A through this core boundary;
        // the bare VT parser used by the painting fixture does not process OSC 133.
        view.session.as_ref().unwrap().term.lock().nebula_add_prompt_mark();
        feed(view, b"$ ");
    });
    exit(&terminals[0], 1, &mut window);
    workspace.read_with(&window, |workspace, _| {
        assert_eq!(workspace.tabs.len(), 1);
        assert!(workspace.locate_pane(terminals[0].entity_id()).is_none());
    });
}

#[gpui::test]
fn pty_failure_keeps_output_even_after_input_and_can_be_closed(cx: &mut TestAppContext) {
    let (workspace, terminals, _, mut window) = open_painted_workspace(&[1, 1], cx);
    window.simulate_keystrokes("x");
    terminals[0].update(&mut window, |view, cx| {
        view.remote_files_test_event(Event::PtyFailure("fixture read failure".into()), cx);
        view.remote_files_test_event(Event::Exit, cx);
    });
    draw(&mut window);
    assert!(terminals[0].read_with(&window, |view, _| view.local_session_failed()));
    assert!(painted(&terminals[0], &mut window).text.contains("cache-output"));
    workspace.read_with(&window, |workspace, _| assert_eq!(workspace.tabs.len(), 2));
    draw_controls(&mut window);
    let close = window.debug_bounds("failed-session-close").unwrap().center();
    window.simulate_click(close, Modifiers::default());
    draw(&mut window);
    workspace.read_with(&window, |workspace, _| {
        assert_eq!(workspace.tabs.len(), 1);
        assert!(workspace.locate_pane(terminals[0].entity_id()).is_none());
    });
}

#[gpui::test]
fn missing_restored_executable_keeps_its_identity_and_recovery_controls(cx: &mut TestAppContext) {
    let (workspace, _, _, mut window) = open_painted_workspace(&[1], cx);
    let saved: crate::session::TabSession = serde_json::from_value(serde_json::json!({
        "cwd": "",
        "launch": {
            "kind": "shell",
            "name": "Removed shell",
            "program": "pebrel-test-missing-shell-executable",
            "args": []
        }
    }))
    .unwrap();
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.restore_tab(&saved, false, window, cx));
            workspace.activate_tab(1, window, cx);
        });
    });
    draw(&mut window);
    workspace.read_with(&window, |workspace, cx| {
        assert_eq!(workspace.tabs.len(), 2);
        let terminal = workspace.tabs[1].focused_view().unwrap().read(cx);
        assert!(terminal.local_session_failed());
        assert_eq!(Some(&terminal.session_launch), saved.launch.as_ref());
    });
    draw_controls(&mut window);
    assert!(window.debug_bounds("failed-session-choose-shell").is_some());
}
