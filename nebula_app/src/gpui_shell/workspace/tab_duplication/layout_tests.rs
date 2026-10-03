use super::*;
use crate::gpui_shell::workspace::windowing;
use crate::session::{AgentSession, LayoutSession, SplitAxis};
use gpui::{AppContext as _, TestAppContext};
use gpui_component::Root;
use std::sync::Arc;

#[gpui::test]
fn duplicate_rebuilds_nested_mixed_layout_with_fresh_sessions(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::workspace::init(cx);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let startup = tempfile::tempdir().unwrap();
    let cwd = directory.path().to_string_lossy().into_owned();
    let program = directory.path().join("pebrel-test-missing-shell").to_string_lossy().into_owned();
    let launches = [
        LaunchSession::Shell { name: "Shell".into(), program: program.clone(), args: vec![] },
        LaunchSession::Profile {
            name: "Profile".into(),
            command: program,
            args: vec!["--login".into()],
            cwd: Some(startup.path().to_string_lossy().into_owned()),
            shell_id: Some("profile-shell".into()),
        },
        LaunchSession::Ssh { host: "pebrel-test@127.0.0.1:1".into() },
    ];
    let pane = |index: usize| LayoutSession::Pane {
        cwd: if index == 2 { "/remote/project with spaces".into() } else { cwd.clone() },
        agent: None,
        launch: Some(launches[index].clone()),
        custom_name: Some(format!("Pane {index}")),
    };
    let original = TabSession {
        cwd: cwd.clone(),
        custom_name: Some("Mixed workspace".into()),
        color: Some(crate::display::color::Rgb::new(12, 34, 56)),
        launch: Some(launches[0].clone()),
        layout: Some(LayoutSession::Split {
            axis: SplitAxis::LeftRight,
            ratio_permille: 370,
            first: Box::new(pane(0)),
            second: Box::new(LayoutSession::Split {
                axis: SplitAxis::TopBottom,
                ratio_permille: 630,
                first: Box::new(pane(1)),
                second: Box::new(pane(2)),
            }),
        }),
        active_pane: 1,
    };
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            let mut workspace = NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                crate::runtime_api::RuntimeHub::new(),
                windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular,
                cx,
            );
            assert!(workspace.restore_tab(&original, false, window, cx));
            if let WorkspaceTab::Terminal { panes, zoomed, broadcast, .. } = &mut workspace.tabs[0]
            {
                // Storage order is independent of layout order; the focused pane isn't the first leaf.
                panes.reverse();
                *zoomed = true;
                *broadcast = true;
                for pane in panes {
                    pane.view.update(cx, |view, cx| {
                        if view.ssh_destination.is_none() {
                            view.cwd = cwd.clone();
                            view.restore_agent(
                                AgentSession {
                                    source: "codex".into(),
                                    session_id: Some("source-only".into()),
                                    session_file: None,
                                },
                                cx,
                            );
                        }
                    });
                }
            }
            // Duplicate an inactive tab, as the context menu can do.
            workspace.add_terminal_with(
                launches[0].clone(),
                Some(directory.path().to_path_buf()),
                None,
                window,
                cx,
            );
            workspace.duplicate_tab(0, window, cx);
            let target = workspace.active;
            assert_eq!(workspace.tabs.len(), 3);
            assert_ne!(target, 0);
            let WorkspaceTab::Terminal { panes: source, tree: source_tree, .. } =
                &workspace.tabs[0]
            else {
                panic!("source tab lost");
            };
            let WorkspaceTab::Terminal { panes, tree, focused, zoomed, broadcast } =
                &workspace.tabs[target]
            else {
                panic!("duplicate must be a terminal tab");
            };
            assert_eq!(panes.len(), 3);
            assert!(!zoomed && !broadcast);
            assert_eq!(*focused, tree.leaves()[1]);
            for (old, new) in source_tree.leaves().iter().zip(tree.leaves()) {
                let old = source.iter().find(|pane| pane.id == *old).unwrap();
                let new = panes.iter().find(|pane| pane.id == new).unwrap();
                assert_ne!(old.id, new.id);
                assert_ne!(old.view.entity_id(), new.view.entity_id());
                assert_eq!(old.custom_name, new.custom_name);
                assert_eq!(old.view.read(cx).cwd, new.view.read(cx).cwd);
                assert!(!new.view.read(cx).recovery_pending());
                if let Some(old_session) = &old.view.read(cx).session {
                    let new_view = new.view.read(cx);
                    let new_session = new_view.session.as_ref().unwrap();
                    assert!(!Arc::ptr_eq(&old_session.term, &new_session.term));
                }
            }
            let snapshot = workspace.snapshot_session(cx);
            let mut expected = original.clone();
            let LayoutSession::Split { second, .. } = expected.layout.as_mut().unwrap() else {
                unreachable!()
            };
            let LayoutSession::Split { first, .. } = second.as_mut() else { unreachable!() };
            let LayoutSession::Pane { launch: Some(LaunchSession::Profile { cwd, .. }), .. } =
                first.as_mut()
            else {
                unreachable!()
            };
            *cwd = None;
            assert_eq!(snapshot.tabs[target], expected);
            workspace.close_tab(target, window, cx);
            assert_eq!(workspace.tabs.len(), 2);
            assert_eq!(
                workspace.snapshot_session(cx).tabs[0].layout.as_ref().unwrap().pane_count(),
                3
            );
            workspace.close_tab(0, window, cx);
            workspace
        });
        Root::new(workspace, window, cx)
    });
    window.update(|window, cx| window.draw(cx).clear(cx));
}
