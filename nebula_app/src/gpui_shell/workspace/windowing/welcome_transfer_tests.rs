use super::*;
use crate::gpui_shell::workspace::welcome::{WelcomeEvent, WelcomeView};
use gpui::TestAppContext;

fn add_welcome(
    workspace: &Entity<NebulaWorkspace>,
    directory: &std::path::Path,
    cx: &mut App,
) -> Entity<WelcomeView> {
    let entry = entry_by_id(workspace.read(cx).runtime_window_id, cx).unwrap();
    entry
        .handle
        .update(cx, |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.add_welcome(Some(directory.into()), window, cx);
                workspace.active_welcome().unwrap()
            })
        })
        .unwrap()
}

#[gpui::test]
fn welcome_tear_out_keeps_the_view_and_tab_identity_without_starting_a_pane(
    cx: &mut TestAppContext,
) {
    let _spawn = crate::gpui_shell::terminal::session::spawn_fixture::fail();
    let directory = tempfile::tempdir().unwrap();
    transfer_tests::initialize_test(cx);
    let (source_id, source, view, identity, next_pane) = cx.update(|cx| {
        let (id, source) = transfer_tests::open_test_window(cx, 0);
        let view = add_welcome(&source, directory.path(), cx);
        let (identity, next_pane) = source.update(cx, |source, cx| {
            source.tab_meta[0].custom_name = Some("Chosen session".into());
            let identity = source.meta(0).runtime_id;
            let next_pane = source.next_pane_id;
            source.schedule_move_tab_to_new_window(0, cx);
            (identity, next_pane)
        });
        (id, source, view, identity, next_pane)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_none());
        assert!(source.read(cx).tabs.is_empty());
        assert_eq!(source.read(cx).next_pane_id, next_pane);
        let entries = &cx.global::<WindowRegistry>().entries;
        assert_eq!(entries.len(), 1);
        let entry = entries[0].clone();
        let target = entry.workspace.upgrade().unwrap();
        assert_eq!(target.read(cx).active_welcome().as_ref(), Some(&view));
        assert_eq!(target.read(cx).meta(0).runtime_id, identity);
        assert_eq!(target.read(cx).meta(0).custom_name.as_deref(), Some("Chosen session"));
        let WorkspaceTab::Welcome { cwd, .. } = &target.read(cx).tabs[0] else {
            panic!("welcome was replaced")
        };
        assert_eq!(cwd.as_deref(), Some(directory.path()));
        let snapshot = entry
            .handle
            .update(cx, |_, window, cx| target.read(cx).runtime_window_snapshot(window, cx).1)
            .unwrap();
        assert_eq!(snapshot.tabs.len(), 1);
        assert_eq!(snapshot.tabs[0].kind, "welcome");
        assert!(snapshot.tabs[0].panes.is_empty());
        assert!(combined_session(None, cx).is_none_or(|session| session.tabs.is_empty()));
    });
}

#[gpui::test]
fn transferred_welcome_owns_its_actions_and_never_docks_as_a_terminal(cx: &mut TestAppContext) {
    let _spawn = crate::gpui_shell::terminal::session::spawn_fixture::fail();
    let directory = tempfile::tempdir().unwrap();
    transfer_tests::initialize_test(cx);
    let (source, target, moved_view, identity, source_next, target_next) = cx.update(|cx| {
        let (source_id, source) = transfer_tests::open_test_window(cx, 0);
        add_welcome(&source, directory.path(), cx);
        let moved_view = add_welcome(&source, directory.path(), cx);
        let (target_id, target) = transfer_tests::open_test_window(cx, 1);
        let target_pane = target.read(cx).active_terminal_pane_id().unwrap();
        let target_next = target.read(cx).next_pane_id;
        let source_next = source.read(cx).next_pane_id;
        let identity = source.read(cx).meta(1).runtime_id;
        let payload = entry_by_id(source_id, cx)
            .unwrap()
            .handle
            .update(cx, |_, window, cx| {
                source.update(cx, |source, cx| {
                    let payload = source.cross_window_drag_payload(1, cx).unwrap();
                    assert!(!payload.can_dock());
                    source.tab_meta[1].custom_name = Some("Move this session".into());
                    source.move_tab(1, 0, window, cx);
                    payload
                })
            })
            .unwrap();
        let destination = CrossWindowDropDestination {
            window_id: target_id,
            dock: Some(DockTarget { pane: Some(target_pane), nav: nebula_split::SplitNav::Right }),
        };
        assert!(drop_tab_to_existing_window(payload.clone(), destination, cx));
        assert!(
            !drop_tab_to_existing_window(payload, destination, cx),
            "a retired payload must not move its replacement"
        );
        assert_eq!(source.read(cx).tabs.len(), 1);
        assert_eq!(target.read(cx).tabs.len(), 2);
        assert_eq!(source.read(cx).next_pane_id, source_next);
        assert_eq!(target.read(cx).next_pane_id, target_next);
        let WorkspaceTab::Terminal { panes, .. } = &target.read(cx).tabs[0] else {
            panic!("target terminal was replaced")
        };
        assert_eq!(panes.len(), 1);
        assert_eq!(target.read(cx).meta(1).runtime_id, identity);
        assert_eq!(target.read(cx).active_welcome().as_ref(), Some(&moved_view));
        moved_view.update(cx, |_, cx| cx.emit(WelcomeEvent::Ssh));
        (source, target, moved_view, identity, source_next, target_next)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!source.read(cx).settings_open);
        assert!(target.read(cx).settings_open, "the moved view must dispatch to its new workspace");
        let entry = entry_by_id(target.read(cx).runtime_window_id, cx).unwrap();
        entry
            .handle
            .update(cx, |_, window, cx| {
                target.update(cx, |target, cx| target.leave_settings(window, cx));
            })
            .unwrap();
        moved_view.update(cx, |_, cx| cx.emit(WelcomeEvent::Local));
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(source.read(cx).next_pane_id, source_next);
        assert_eq!(target.read(cx).next_pane_id, target_next + 1);
        assert_eq!(target.read(cx).tabs.len(), 2);
        assert_eq!(target.read(cx).meta(1).runtime_id, identity);
        assert_eq!(target.read(cx).meta(1).custom_name.as_deref(), Some("Move this session"));
        assert_eq!(
            target.read(cx).tabs[1].focused_view().unwrap().read(cx).local_cwd().as_deref(),
            Some(directory.path())
        );
        moved_view.update(cx, |_, cx| cx.emit(WelcomeEvent::Local));
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(source.read(cx).next_pane_id, source_next);
        assert_eq!(
            target.read(cx).next_pane_id,
            target_next + 1,
            "retired welcome events must not launch twice"
        );
    });
}
