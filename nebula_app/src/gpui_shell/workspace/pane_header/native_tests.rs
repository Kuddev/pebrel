use super::super::TabMeta;
use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use gpui::{Focusable as _, Modifiers, TestAppContext};

#[gpui::test]
fn header_drag_exchanges_existing_views_and_escape_cancels(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("missing-test-shell");
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::super::init(cx);
        super::super::windowing::initialize(cx, hub.clone());
        cx.set_reduce_motion(true);
    });
    let mut fixture = None;
    let (_, mut window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                hub,
                super::super::windowing::WorkspaceStartup::Empty,
                super::super::windowing::WindowRole::Regular,
                cx,
            )
        });
        let ids = workspace.update(cx, |workspace, cx| {
            let mut panes = (0..4)
                .map(|_| {
                    workspace.new_pane(
                        (80, 24),
                        TerminalLaunch::Local {
                            cwd: Some(directory.path().into()),
                            shell: Some(nebula_terminal::tty::Shell::new(
                                program.to_string_lossy().into_owned(),
                                vec![],
                            )),
                            shell_name: None,
                        },
                        None,
                        window,
                        cx,
                    )
                })
                .collect::<Vec<_>>();
            let other = panes.pop().unwrap();
            let other_id = other.id;
            let ids = [panes[0].id, panes[1].id, panes[2].id];
            let tree = SplitTree::leaf(ids[0]).joined(
                SplitTree::leaf(ids[1])
                    .joined(SplitTree::leaf(ids[2]), nebula_split::SplitNav::Right),
                nebula_split::SplitNav::Right,
            );
            workspace.insert_tab_at(
                0,
                WorkspaceTab::Terminal {
                    panes,
                    tree,
                    focused: ids[0],
                    zoomed: false,
                    broadcast: true,
                },
                TabMeta::default(),
            );
            workspace.insert_tab_at(
                1,
                WorkspaceTab::Terminal {
                    panes: vec![other],
                    tree: SplitTree::leaf(other_id),
                    focused: other_id,
                    zoomed: false,
                    broadcast: false,
                },
                TabMeta::default(),
            );
            workspace.focus_active(window, cx);
            cx.notify();
            ids
        });
        fixture = Some((workspace.clone(), ids));
        Root::new(workspace, window, cx)
    });
    let (workspace, ids) = fixture.unwrap();
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let source_selector = &*Box::leak(format!("pane-header-grip-{}", ids[0]).into_boxed_str());
    let target_selector = &*Box::leak(format!("pane-header-grip-{}", ids[2]).into_boxed_str());
    let source = window.debug_bounds(source_selector).unwrap().center();
    let target = window.debug_bounds(target_selector).unwrap().center();
    let views = workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { panes, .. } = &workspace.tabs[0] else { panic!() };
        panes.iter().map(|pane| pane.view.clone()).collect::<Vec<_>>()
    });
    window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        assert!(workspace.pane_drag.as_ref().is_some_and(|drag| drag.active));
    });
    window.simulate_keystrokes("ctrl-tab");
    workspace.read_with(&window, |workspace, _| {
        assert_eq!(workspace.active, 1);
        assert!(workspace.pane_drag.is_none());
    });
    window.simulate_keystrokes("ctrl-shift-tab");
    window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        assert_eq!(workspace.active, 0);
        let WorkspaceTab::Terminal { tree, .. } = &workspace.tabs[0] else { panic!() };
        assert_eq!(tree.leaves(), ids, "returning to the source tab must not revive the drag");
    });
    window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
    window.simulate_keystrokes("escape");
    window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { tree, .. } = &workspace.tabs[0] else { panic!() };
        assert_eq!(tree.leaves(), ids);
        assert!(workspace.pane_drag.is_none());
    });
    window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
    window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { tree, panes, focused, broadcast, .. } = &workspace.tabs[0]
        else {
            panic!()
        };
        assert_eq!(tree.leaves(), [ids[2], ids[1], ids[0]]);
        assert_eq!(*focused, ids[0]);
        assert!(*broadcast);
        assert_eq!(panes.iter().map(|pane| pane.view.clone()).collect::<Vec<_>>(), views);
        assert_eq!(workspace.tabs.len(), 2);
    });
    window.update(|window, cx| {
        assert!(views[0].read(cx).focus_handle(cx).is_focused(window));
    });
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let grip = window.debug_bounds(source_selector).unwrap().center();
    let body = workspace.read_with(&window, |workspace, _| {
        workspace.pane_bounds.borrow().get(&ids[0]).unwrap().center()
    });
    window.simulate_mouse_down(body, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(
        body + gpui::point(px(8.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    workspace.read_with(&window, |workspace, _| assert!(workspace.pane_drag.is_none()));
    window.simulate_mouse_up(body, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(source, Some(MouseButton::Left), Modifiers::default());
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_pane(0, ids[0], window, cx));
    });
    window.simulate_mouse_up(source, MouseButton::Left, Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { tree, panes, .. } = &workspace.tabs[0] else { panic!() };
        assert!(!tree.contains(ids[0]));
        assert_eq!(panes.len(), 2);
        assert!(workspace.pane_drag.is_none());
        assert_eq!(workspace.tabs.len(), 2);
    });
}
