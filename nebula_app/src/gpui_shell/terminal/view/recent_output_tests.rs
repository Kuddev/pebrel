use super::*;
use gpui::TestAppContext;
use startup_tests::{feed, open};

fn prompt(view: &mut TerminalView, input: &str) {
    view.suggest.suggest_env = crate::display::SuggestEnv::Local;
    feed(view, format!("\x1b]133;A\x07C:\\work>\x1b]133;B\x07\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07{input}").as_bytes());
}

fn restored_text(view: &TerminalView) -> String {
    let recent =
        crate::recent_output::RecentOutput::try_from_records(view.recent_output_snapshot())
            .unwrap();
    let (session, _) = session::test_session();
    let mut term = session.term.lock();
    recent.restore(term.grid_mut()).unwrap();
    use nebula_terminal::grid::Dimensions;
    term.bounds_to_string(
        TermPoint::new(Line(-(term.grid().history_size() as i32)), Column(0)),
        TermPoint::new(Line(-1), Column(term.columns() - 1)),
    )
}

#[gpui::test]
fn confirmed_shell_submission_captures_output_but_not_later_idle_input(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        prompt(view, "echo saved");
        view.commit_line(cx);
        view.write_input(b"\r".to_vec(), cx);
        feed(view, b"\r\nsaved result\r\n");
        prompt(view, "");
        view.finish_foreground_command(Some(0), cx);
        feed(view, b"not-submitted");
        let text = restored_text(view);
        assert!(text.contains("echo saved"), "{text:?}");
        assert!(text.contains("saved result"));
        assert!(!text.contains("not-submitted"));
        assert_eq!(view.recent_output_snapshot().len(), 1);
    });
}

#[gpui::test]
fn empty_enter_and_foreground_password_do_not_create_records(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        prompt(view, "");
        view.commit_line(cx);
        assert!(view.recent_output_snapshot().is_empty());
        feed(view, b"tool");
        view.commit_line(cx);
        view.write_input(b"\r".to_vec(), cx);
        feed(view, b"\r\nPassword: ");
        view.suggest.line_buf = "invisible-secret".into();
        view.commit_line(cx);
        assert_eq!(view.recent_output_snapshot().len(), 1);
        assert!(!restored_text(view).contains("invisible-secret"));
    });
}

#[gpui::test]
fn alternate_screen_keeps_main_output_and_never_saves_tui_input(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        prompt(view, "tool");
        view.commit_line(cx);
        view.write_input(b"\r".to_vec(), cx);
        feed(view, b"\r\nmain output\x1b[?1049hTUI private input");
        let text = restored_text(view);
        assert!(text.contains("main output"), "{text:?}");
        assert!(!text.contains("TUI private input"), "{text:?}");
        feed(view, b"\x1b[?1049l\r\n");
        prompt(view, "");
        view.finish_foreground_command(Some(0), cx);
        assert!(restored_text(view).contains("main output"));
    });
}

#[gpui::test]
fn cold_restore_inserts_once_without_executing_or_replacing_prompt(cx: &mut TestAppContext) {
    let (source, window, _) = open(cx);
    let records = source.update(window, |view, cx| {
        prompt(view, "echo old-marker");
        view.commit_line(cx);
        view.write_input(b"\r".to_vec(), cx);
        feed(view, b"\r\nold-result\r\n");
        prompt(view, "");
        view.finish_foreground_command(Some(0), cx);
        view.recent_output_snapshot()
    });
    let (target, target_window, _) = open(cx);
    target.update(target_window, |view, cx| {
        prompt(view, "");
        let before = view.session.as_ref().unwrap().term.lock().grid().cursor.point;
        assert!(view.restore_recent_output(records.clone(), cx).unwrap());
        assert!(!view.restore_recent_output(records, cx).unwrap());
        assert!(!view.command_running);
        assert!(view.active_run.is_none());
        assert_eq!(view.recent_output_snapshot().len(), 1);
        let term = view.session.as_ref().unwrap().term.lock();
        use nebula_terminal::grid::Dimensions;
        assert_eq!(term.grid().cursor.point, before);
        let text = term.bounds_to_string(
            TermPoint::new(Line(-(term.grid().history_size() as i32)), Column(0)),
            TermPoint::new(Line(-1), Column(term.columns() - 1)),
        );
        assert_eq!(text.matches("echo old-marker").count(), 1, "{text:?}");
        assert_eq!(text.matches("old-result").count(), 1, "{text:?}");
        let origin = term.viewport_origin_for(term.screen_lines());
        let visible = term.bounds_to_string(
            TermPoint::new(origin, Column(0)),
            TermPoint::new(origin + term.screen_lines() - 1, Column(term.columns() - 1)),
        );
        assert!(visible.contains("old-result"), "restored output must be visible: {visible:?}");
        assert!(visible.contains("C:\\work>"), "live prompt must remain visible: {visible:?}");
    });
}

#[gpui::test]
fn restored_output_follows_input_until_live_screen_fills(cx: &mut TestAppContext) {
    let (source, window, _) = open(cx);
    let records = source.update(window, |view, cx| {
        prompt(view, "echo old-marker");
        view.commit_line(cx);
        feed(view, b"\r\nold-result\r\n");
        prompt(view, "");
        view.finish_foreground_command(Some(0), cx);
        view.recent_output_snapshot()
    });
    let (target, window, _) = open(cx);
    target.update(window, |view, cx| {
        prompt(view, "");
        view.restore_recent_output(records, cx).unwrap();
        let offset = view.session.as_ref().unwrap().term.lock().grid().display_offset();
        assert!(offset > 0);
        view.write_input(b"e".to_vec(), cx);
        feed(view, b"e");
        view.process_event(TermEvent::Wakeup, cx);
        assert_eq!(view.session.as_ref().unwrap().term.lock().grid().display_offset(), offset);
        feed(view, b"\r\nnext-output\r\n");
        view.process_event(TermEvent::Wakeup, cx);
        assert_eq!(view.session.as_ref().unwrap().term.lock().grid().display_offset(), offset);
        feed(view, "\r\nnew-line".repeat(40).as_bytes());
        view.process_event(TermEvent::Wakeup, cx);
        assert_eq!(view.session.as_ref().unwrap().term.lock().grid().display_offset(), 0);
    });
}

#[gpui::test]
fn restored_long_command_stays_visible_when_startup_layout_narrows(cx: &mut TestAppContext) {
    let (source, window, _) = open(cx);
    let records = source.update(window, |view, cx| {
        prompt(view, "echo long-command-marker-1234567890");
        view.commit_line(cx);
        feed(view, b"\r\nold-result\r\n");
        prompt(view, "");
        view.finish_foreground_command(Some(0), cx);
        view.recent_output_snapshot()
    });
    let (target, window, _) = open(cx);
    target.update(window, |view, cx| {
        prompt(view, "");
        view.restore_recent_output(records, cx).unwrap();
        view.session
            .as_ref()
            .unwrap()
            .term
            .lock()
            .resize(session::GridSize { columns: 24, screen_lines: 24 });
        view.process_event(TermEvent::Wakeup, cx);
        let term = view.session.as_ref().unwrap().term.lock();
        use nebula_terminal::grid::Dimensions;
        let origin = term.viewport_origin_for(term.screen_lines());
        let visible = term.bounds_to_string(
            TermPoint::new(origin, Column(0)),
            TermPoint::new(origin + term.screen_lines() - 1, Column(term.columns() - 1)),
        );
        assert!(visible.contains("C:\\work>echo long-command-marker-1234567890"), "{visible:?}");
        assert!(visible.contains("old-result"), "{visible:?}");
    });
}

#[gpui::test]
fn manual_scroll_after_restore_keeps_normal_input_return_to_bottom(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        feed(view, "old-line\r\n".repeat(40).as_bytes());
        view.session.as_ref().unwrap().term.lock().scroll_display(Scroll::Delta(3));
        view.restored_viewport_offset = Some(3);
        view.scroll_to_offset(5, 3);
        view.process_event(TermEvent::Wakeup, cx);
        assert_eq!(view.session.as_ref().unwrap().term.lock().grid().display_offset(), 5);
        view.write_input(b"e".to_vec(), cx);
        assert_eq!(view.session.as_ref().unwrap().term.lock().grid().display_offset(), 0);
        assert_eq!(view.restored_viewport_offset, None);
    });
}

#[gpui::test]
fn alternate_screen_ends_restore_follow_without_scrolling_the_application(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        feed(view, "old-line\r\n".repeat(40).as_bytes());
        view.session.as_ref().unwrap().term.lock().scroll_display(Scroll::Delta(3));
        view.restored_viewport_offset = Some(3);
        feed(view, b"\x1b[?1049hfull-screen");
        view.process_event(TermEvent::Wakeup, cx);
        assert_eq!(view.restored_viewport_offset, None);
        assert_eq!(view.session.as_ref().unwrap().term.lock().grid().display_offset(), 0);
        feed(view, b"\x1b[?1049l");
        view.write_input(b"e".to_vec(), cx);
        assert_eq!(view.session.as_ref().unwrap().term.lock().grid().display_offset(), 0);
    });
}

#[gpui::test]
fn late_restore_does_not_replace_commands_submitted_since_startup(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        prompt(view, "echo new-command");
        view.commit_line(cx);
        assert!(!view.restore_recent_output(Vec::new(), cx).unwrap());
        assert!(restored_text(view).contains("echo new-command"));
    });
}

#[gpui::test]
fn disabled_capture_does_not_collect_commands_or_accept_restore(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.set_recent_output_enabled(false);
        prompt(view, "echo do-not-save");
        view.commit_line(cx);
        feed(view, b"\r\nprivate output\r\n");
        view.finish_foreground_command(Some(0), cx);
        assert!(view.recent_output_snapshot().is_empty());
        assert!(!view.restore_recent_output(Vec::new(), cx).unwrap());
        view.set_recent_output_enabled(true);
        prompt(view, "echo allowed");
        view.commit_line(cx);
        assert_eq!(view.recent_output_snapshot().len(), 1);
    });
}

#[gpui::test]
fn disabling_capture_clears_memory_and_stops_active_output(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        prompt(view, "tool");
        view.commit_line(cx);
        feed(view, b"\r\nbefore\r\n");
        assert_eq!(view.recent_output_snapshot().len(), 1);
        view.set_recent_output_enabled(false);
        view.set_recent_output_enabled(true);
        feed(view, b"after-enable");
        view.finish_foreground_command(Some(0), cx);
        assert!(view.recent_output_snapshot().is_empty());
    });
}

#[gpui::test]
fn loaded_settings_disable_existing_and_new_terminal_capture(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    for raw in ["save_command_output=0", "restore_session=0\nsave_command_output=1"] {
        view.update(window, |view, cx| {
            let settings = Settings::load_with_runtime(
                nebula_settings::ThemeName::Nord,
                nebula_settings::RuntimeSettings::from_raw(
                    &nebula_settings::RawSettings::from_text(raw),
                ),
            );
            cx.set_global(settings);
            view.apply_settings(cx);
            prompt(view, "echo private");
            view.commit_line(cx);
            assert!(view.recent_output_snapshot().is_empty(), "{raw}");
        });
        window.update(|window, cx| {
            let new_view = cx.new(|cx| {
                TerminalView::new(
                    43,
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
            assert!(!new_view.read(cx).recent_output_enabled, "{raw}");
        });
    }
}

#[gpui::test]
fn batch_restore_reads_exact_references_and_skips_changed_panes(cx: &mut TestAppContext) {
    let (source, window, _) = open(cx);
    let records = source.update(window, |view, cx| {
        prompt(view, "echo batch-marker");
        view.commit_line(cx);
        feed(view, b"\r\nbatch-result\r\n");
        view.finish_foreground_command(Some(0), cx);
        view.recent_output_snapshot()
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outputs.json");
    crate::recent_output::storage::Archive::new(std::collections::BTreeMap::from([
        ("first".into(), records.clone()),
        ("changed".into(), records.clone()),
        ("disabled".into(), records),
    ]))
    .write_to(&path)
    .unwrap();
    let (changed, changed_window, _) = open(cx);
    changed.update(changed_window, |view, cx| {
        prompt(view, "echo new-command");
        view.commit_line(cx);
    });
    let (disabled, disabled_window, _) = open(cx);
    disabled.update(disabled_window, |view, _| view.set_recent_output_enabled(false));
    let (missing, _, _) = open(cx);
    let (first, first_window, _) = open(cx);
    first_window.update(|_, cx| {
        TerminalView::restore_output_batch(
            path,
            vec![
                ("first".into(), first.downgrade()),
                ("changed".into(), changed.downgrade()),
                ("disabled".into(), disabled.downgrade()),
                ("missing".into(), missing.downgrade()),
            ],
            cx,
        );
    });
    first_window.run_until_parked();
    first.update(first_window, |view, _| {
        assert!(restored_text(view).contains("batch-marker"));
        assert!(!view.command_running);
    });
    changed.update(first_window, |view, _| {
        assert!(restored_text(view).contains("new-command"));
        assert!(!restored_text(view).contains("batch-marker"));
    });
    disabled.update(first_window, |view, _| assert!(view.recent_output_snapshot().is_empty()));
    missing.update(first_window, |view, _| assert!(view.recent_output_snapshot().is_empty()));
}

#[gpui::test]
fn runtime_submission_records_echo_only_after_barrier(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        prompt(view, "");
        view.runtime_prompt("echo runtime".into(), true, cx).unwrap();
        assert!(view.recent_output_snapshot().is_empty());
        feed(view, b"echo runtime");
        view.flush_pending_runtime_submit(cx);
        assert_eq!(view.recent_output_snapshot().len(), 1);
        assert!(restored_text(view).contains("echo runtime"));
    });
}
