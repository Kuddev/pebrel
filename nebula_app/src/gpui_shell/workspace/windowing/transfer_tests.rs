use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use gpui::TestAppContext;

fn initialize_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        initialize(cx, crate::runtime_api::RuntimeHub::new());
    });
}

fn open_test_window(cx: &mut App, count: usize) -> (u64, Entity<NebulaWorkspace>) {
    let (id, workspace) =
        open_workspace_window(cx, WorkspaceStartup::Empty, None, None, false, WindowRole::Regular)
            .unwrap();
    let entry = entry_by_id(id, cx).unwrap();
    entry
        .handle
        .update(cx, |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                for index in 0..count {
                    let pane = workspace.new_pane(
                        (80, 24),
                        TerminalLaunch::Local {
                            cwd: Some(PathBuf::from(format!("test-project-{index}"))),
                            shell: Some(nebula_terminal::tty::Shell::new(
                                "pebrel-test-missing-shell-executable".into(),
                                vec![],
                            )),
                            shell_name: None,
                        },
                        None,
                        window,
                        cx,
                    );
                    workspace.insert_tab_at(
                        index,
                        WorkspaceTab::Terminal {
                            tree: SplitTree::leaf(pane.id),
                            focused: pane.id,
                            panes: vec![pane],
                            zoomed: false,
                            broadcast: false,
                        },
                        TabMeta::default(),
                    );
                }
            });
        })
        .unwrap();
    (id, workspace)
}

#[gpui::test]
fn moving_last_tab_closes_only_source_after_transfer(cx: &mut TestAppContext) {
    initialize_test(cx);
    let (source_id, source, other_id, other, moved_view) = cx.update(|cx| {
        let (source_id, source) = open_test_window(cx, 1);
        let (other_id, other) = open_test_window(cx, 1);
        let moved_view = source.read(cx).tabs[0].focused_view().unwrap().clone();
        source.update(cx, |source, cx| source.schedule_move_tab_to_new_window(0, cx));
        (source_id, source, other_id, other, moved_view)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_none());
        assert!(source.read(cx).tabs.is_empty());
        assert_eq!(other.read(cx).tabs.len(), 1);
        assert!(entry_by_id(other_id, cx).is_some());
        let entries = &cx.global::<WindowRegistry>().entries;
        assert_eq!(entries.len(), 2);
        let target = entries
            .iter()
            .find(|entry| entry.runtime_window_id != other_id)
            .unwrap()
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}

#[gpui::test]
fn moving_one_of_two_tabs_preserves_source_and_identity(cx: &mut TestAppContext) {
    initialize_test(cx);
    let (source_id, source, moved_view) = cx.update(|cx| {
        let (id, source) = open_test_window(cx, 2);
        let moved_view = source.read(cx).tabs[1].focused_view().unwrap().clone();
        source.update(cx, |source, cx| source.schedule_move_tab_to_new_window(1, cx));
        (id, source, moved_view)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_some());
        assert_eq!(source.read(cx).tabs.len(), 1);
        let target = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id != source_id)
            .unwrap()
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}
