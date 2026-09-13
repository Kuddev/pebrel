use super::*;
use crate::gpui_shell::file_editor::TextFileView;
use crate::i18n::Message;
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

fn initialize_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        initialize(cx, crate::runtime_api::RuntimeHub::new());
    });
}

fn open_editor(
    path: PathBuf,
    cx: &mut TestAppContext,
) -> (Entity<NebulaWorkspace>, Entity<TextFileView>, VisualTestContext) {
    std::fs::write(&path, "original").unwrap();
    let mut entities = None;
    let (_, mut visual) = cx.add_window_view(|window, cx| {
        let (id, hub) = allocate_window(cx);
        let workspace = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                id,
                hub,
                WorkspaceStartup::Empty,
                WindowRole::Regular,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| workspace.open_document_path(path, window, cx));
        let file = workspace.read(cx).tabs[0].file_editor(cx).unwrap();
        cx.global_mut::<WindowRegistry>().entries.push(WindowEntry {
            runtime_window_id: id,
            handle: window.window_handle(),
            workspace: workspace.downgrade(),
            last_activated: id,
            native_hwnd: 0,
            role: WindowRole::Regular,
        });
        entities = Some((workspace, file.clone()));
        Root::new(file, window, cx)
    });
    visual.run_until_parked();
    let (workspace, file) = entities.unwrap();
    visual.update(|window, cx| file.read(cx).focus_handle(cx).focus(window, cx));
    visual.simulate_input("draft ");
    visual.run_until_parked();
    assert!(file.read_with(visual, |file, _| file.is_dirty()));
    (workspace, file, visual.clone())
}

fn answer(cx: &mut TestAppContext, message: Message) {
    let label = cx.read(|cx| crate::gpui_shell::config::ui_language(cx).text(message));
    cx.simulate_prompt_answer(label);
    cx.run_until_parked();
}

#[gpui::test]
fn quit_cancel_preserves_unsaved_document(cx: &mut TestAppContext) {
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("draft.txt");
    let (workspace, file, _visual) = open_editor(path.clone(), cx);
    cx.update(quit_all);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "quitting must ask about the unsaved document");
    assert!(cx.read(|cx| cx.global::<WindowRegistry>().quit_pending));
    answer(cx, Message::EditorCancel);
    assert!(!cx.read(|cx| cx.global::<WindowRegistry>().quit_pending));
    assert!(!workspace.read_with(cx, |workspace, _| workspace.window_close_confirm_open));
    assert!(file.read_with(cx, |file, _| file.is_dirty()));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
}

#[gpui::test]
fn quit_reveals_a_hidden_window_with_unsaved_work(cx: &mut TestAppContext) {
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _file, _visual) = open_editor(directory.path().join("hidden.txt"), cx);
    workspace.update(cx, |workspace, _| workspace.window_hidden = true);
    cx.update(quit_all);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    assert!(!workspace.read_with(cx, |workspace, _| workspace.window_hidden));
    answer(cx, Message::EditorCancel);
}

#[gpui::test]
fn quit_cancel_in_second_window_preserves_both_drafts(cx: &mut TestAppContext) {
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let (first, first_file, _first_window) = open_editor(directory.path().join("first.txt"), cx);
    let (second, second_file, _second_window) =
        open_editor(directory.path().join("second.txt"), cx);
    cx.update(quit_all);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    answer(cx, Message::EditorDiscard);
    assert!(cx.has_pending_prompt(), "every window participates in quit confirmation");
    answer(cx, Message::EditorCancel);
    assert!(!cx.read(|cx| cx.global::<WindowRegistry>().quit_pending));
    for (workspace, file) in [(first, first_file), (second, second_file)] {
        assert!(!workspace.read_with(cx, |workspace, _| workspace.window_close_confirm_open));
        assert!(file.read_with(cx, |file, _| file.is_dirty()));
    }
    assert_eq!(cx.read(|cx| cx.global::<WindowRegistry>().entries.len()), 2);
}

#[gpui::test]
fn quit_save_failure_preserves_external_file_and_draft(cx: &mut TestAppContext) {
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conflict.txt");
    let (_workspace, file, _visual) = open_editor(path.clone(), cx);
    std::fs::write(&path, "external change").unwrap();
    cx.update(quit_all);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    answer(cx, Message::EditorSave);
    assert!(!cx.read(|cx| cx.global::<WindowRegistry>().quit_pending));
    assert!(file.read_with(cx, |file, _| file.is_dirty()));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "external change");
}

#[gpui::test]
fn quit_save_writes_the_approved_document(cx: &mut TestAppContext) {
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("save.txt");
    let (_workspace, file, _visual) = open_editor(path.clone(), cx);
    let draft = file.read_with(cx, |file, cx| file.draft(cx));
    cx.update(quit_all);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    answer(cx, Message::EditorSave);
    assert!(!cx.has_pending_prompt());
    assert!(cx.read(|cx| cx.global::<WindowRegistry>().quit_pending));
    assert!(!file.read_with(cx, |file, _| file.is_dirty()));
    assert_eq!(std::fs::read_to_string(path).unwrap(), draft.as_ref());
}

#[gpui::test]
fn quit_does_not_interrupt_a_save_already_in_progress(cx: &mut TestAppContext) {
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let (_workspace, file, _visual) = open_editor(directory.path().join("saving.txt"), cx);
    cx.update(|cx| {
        file.update(cx, |file, cx| file.save(cx)).detach();
        assert!(file.read(cx).is_saving());
        quit_all(cx);
        assert!(!cx.global::<WindowRegistry>().quit_pending);
    });
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(!file.read_with(cx, |file, _| file.is_saving()));
}

#[gpui::test]
fn quit_rechecks_discarded_draft_after_another_window_prompt(cx: &mut TestAppContext) {
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let (_first, file, mut first_window) = open_editor(directory.path().join("first.txt"), cx);
    let (_second, _second_file, _second_window) =
        open_editor(directory.path().join("second.txt"), cx);
    cx.update(quit_all);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    answer(cx, Message::EditorDiscard);
    assert!(cx.has_pending_prompt());
    first_window.simulate_input("new edit ");
    first_window.run_until_parked();
    answer(cx, Message::EditorDiscard);
    assert!(!cx.read(|cx| cx.global::<WindowRegistry>().quit_pending));
    assert!(file.read_with(cx, |file, _| file.is_dirty()));
    assert!(file.read_with(cx, |file, cx| file.draft(cx).contains("new edit")));
}
