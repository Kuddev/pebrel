use super::startup_tests::{feed, open};
use super::*;
use gpui::TestAppContext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[gpui::test]
fn runtime_submission_codex_prompt_does_not_depend_on_a_repaint(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        view.running_program = Some("codex".into());
        view.runtime_prompt("继续".into(), true, cx).unwrap();
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes)
            if bytes.as_ref() == "继续\x1b[C\r".as_bytes()));
        assert!(view.pending_runtime_submit.is_none());
        assert_eq!(view.runtime_task_state(), crate::runtime_api::RuntimeTaskState::Running);
        view.process_event(TermEvent::Wakeup, cx);
        assert!(receiver.try_recv().is_err(), "a later repaint must not send Enter twice");
    });
}

#[gpui::test]
fn runtime_submission_codex_paste_and_no_submit_preserve_their_contracts(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        view.running_program = Some("codex".into());
        view.runtime_prompt("draft".into(), false, cx).unwrap();
        assert!(
            matches!(receiver.try_recv().unwrap(), Msg::Input(bytes) if bytes.as_ref() == b"draft")
        );
        feed(view, b"\x1b[?2004h");
        view.runtime_paste("first\nsecond".into(), true, InputOrigin::Program, cx).unwrap();
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes)
            if bytes.as_ref() == b"\x1b[200~first\rsecond\x1b[201~\x1b[C\r"));
        assert!(view.pending_runtime_submit.is_none());
    });
}

#[gpui::test]
fn working_directory_copy_preserves_prompt_selection_and_utf8(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    let prompt = "    ~    15:30:27  ";
    view.update(window, |view, cx| {
        view.process_event(TermEvent::CwdReport("/home/用户/目录 with spaces".into()), cx);
        feed(view, prompt.as_bytes());
        let mut term = view.session.as_ref().unwrap().term.lock();
        term.selection = Some(Selection::new(
            SelectionType::Lines,
            TermPoint::new(Line(0), Column(0)),
            Side::Left,
        ));
    });
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            let selected = view.session.as_ref().unwrap().term.lock().selection_to_string();
            assert!(selected.as_deref().unwrap().contains(prompt));
            assert!(view.copy_working_directory(window, cx));
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some("/home/用户/目录 with spaces".into())
            );
            assert_eq!(view.session.as_ref().unwrap().term.lock().selection_to_string(), selected);
            assert!(view.copy_selection(false, window, cx));
            assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), selected);
            for unavailable in ["", "~", "relative/path", "/bad\npath"] {
                view.cwd = unavailable.into();
                assert!(!view.copy_working_directory(window, cx));
                assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), selected);
            }
        });
    });
}

#[gpui::test]
fn hidden_output_keeps_the_latest_grid_without_notifying_observers(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.run_until_parked();
    let notifications = Rc::new(Cell::new(0));
    let _subscription = window.update(|_, cx| {
        let notifications = notifications.clone();
        cx.observe(&view, move |_, _| notifications.set(notifications.get() + 1))
    });
    view.update(window, |view, cx| {
        // A visible but unfocused pane must still show output.
        view.cursor_window_active = false;
        view.cursor_pane_focused = false;
        view.process_event(TermEvent::Wakeup, cx);
    });
    assert_eq!(notifications.get(), 1);
    view.update(window, |view, cx| view.set_output_visible(false, cx));
    notifications.set(0);
    for index in 0..256 {
        view.update(window, |view, cx| {
            feed(view, format!("\r\x1b[2Kbackground-output-{index}").as_bytes());
            view.process_event(TermEvent::Wakeup, cx);
        });
    }
    assert_eq!(notifications.get(), 0, "hidden output must not invalidate chrome readers");
    view.update(window, |view, cx| {
        let snapshot = nebula_terminal::render::RenderSnapshot::capture(
            &view.session.as_ref().unwrap().term.lock(),
            &nebula_terminal::render::SnapshotConfig { rows: 24, cols: 80 },
        );
        let text: String = snapshot
            .segments
            .iter()
            .flat_map(|segment| &segment.cells)
            .map(|cell| cell.text.as_str())
            .collect();
        assert!(text.contains("background-output-255"));
        view.set_output_visible(true, cx);
    });
    assert_eq!(notifications.get(), 1, "reveal invalidates even after output has stopped");
    view.update(window, |view, cx| view.process_event(TermEvent::Wakeup, cx));
    assert_eq!(notifications.get(), 2, "visible output resumes invalidation");
}

#[gpui::test]
fn declaring_the_same_output_visibility_does_not_start_a_redraw_loop(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.run_until_parked();
    let notifications = Rc::new(Cell::new(0));
    let _subscription = window.update(|_, cx| {
        let notifications = notifications.clone();
        cx.observe(&view, move |_, _| notifications.set(notifications.get() + 1))
    });
    for visible in [true, true, false, false, true, true] {
        view.update(window, |view, cx| view.set_output_visible(visible, cx));
    }
    assert_eq!(notifications.get(), 1, "only hidden-to-visible needs invalidation");
}

#[gpui::test]
fn hidden_terminals_keep_title_progress_bell_and_exit_events(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    let received = Rc::new(RefCell::new(Vec::new()));
    let _subscription = window.update(|_, cx| {
        let received = received.clone();
        cx.subscribe(&view, move |_, event, _| {
            let label = match event {
                TerminalViewEvent::TitleChanged => "title",
                TerminalViewEvent::ProgressChanged(_) => "progress",
                TerminalViewEvent::Bell => "bell",
                TerminalViewEvent::Exited => "exit",
                _ => return,
            };
            received.borrow_mut().push(label);
        })
    });
    view.update(window, |view, cx| {
        view.set_output_visible(false, cx);
        view.process_event(TermEvent::Title("background task".into()), cx);
        view.process_event(TermEvent::Progress { state: 1, value: Some(25) }, cx);
        view.process_event(TermEvent::Bell, cx);
        view.process_event(TermEvent::Exit, cx);
    });
    assert_eq!(*received.borrow(), ["title", "progress", "bell", "exit"]);
}

#[gpui::test]
fn hidden_wakeup_still_flushes_shell_commands_and_pending_enter(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    view.update(window, |view, cx| {
        view.set_output_visible(false, cx);
        view.run_command("echo background-check".into(), cx);
        assert!(receiver.try_recv().is_err(), "wait for the real prompt");
        feed(view, b"\x1b]133;A\x07user@host:/tmp$ ");
        view.process_event(TermEvent::Wakeup, cx);
        assert!(view.pending_shell_command.is_none());
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes)
            if bytes.as_ref() == b"echo background-check"));
        assert!(receiver.try_recv().is_err(), "Enter waits for echo");
        feed(view, b"echo background-check");
        view.process_event(TermEvent::Wakeup, cx);
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes)
            if bytes.as_ref() == b"\r"));
        assert!(view.pending_runtime_submit.is_none());
    });
}
