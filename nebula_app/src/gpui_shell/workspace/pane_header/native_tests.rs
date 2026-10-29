use super::super::TabMeta;
use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use gpui::{AppContext as _, Focusable as _, Modifiers, TestAppContext};
use gpui_component::Root;

#[gpui::test]
fn header_drag_exchanges_existing_views_and_escape_cancels(cx: &mut TestAppContext) {
    exercise_drag(cx, nebula_settings::TabsPositionName::Top);
}

#[gpui::test]
fn sidebar_grip_exchanges_existing_views_and_escape_cancels(cx: &mut TestAppContext) {
    exercise_drag(cx, nebula_settings::TabsPositionName::Sidebar);
}

fn hover(window: &mut gpui::VisualTestContext, position: gpui::Point<gpui::Pixels>) {
    window.simulate_mouse_move(position, None, Modifiers::default());
    window.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
}

fn exercise_drag(cx: &mut TestAppContext, mode: nebula_settings::TabsPositionName) {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("missing-test-shell");
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        super::super::windowing::initialize(cx, hub.clone());
        cx.set_reduce_motion(true);
    });
    let mut fixture = None;
    let (_, window) = cx.add_window_view(|window, cx| {
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
            workspace.tabs_position = mode;
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
            let mut tree = SplitTree::leaf(ids[0]).joined(
                SplitTree::leaf(ids[1])
                    .joined(SplitTree::leaf(ids[2]), nebula_split::SplitNav::Right),
                nebula_split::SplitNav::Right,
            );
            // The middle column's centre is left of the full workspace centre,
            // exposing stale geometry when the left pane is zoomed.
            if let SplitTree::Split { ratio, .. } = &mut tree {
                *ratio = 0.25;
            }
            workspace.insert_tab_at(
                0,
                WorkspaceTab::Terminal {
                    panes,
                    tree,
                    focused: ids[1],
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
    let mut window = window.clone();
    let (workspace, ids) = fixture.unwrap();
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
    workspace.update(&mut window, |workspace, cx| {
        workspace.tabs_position = mode;
        workspace.sync_settings_layout(true);
        cx.notify();
    });
    window.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    let source_selector = &*Box::leak(format!("pane-header-grip-{}", ids[0]).into_boxed_str());
    let source = workspace.read_with(&window, |workspace, _| {
        let bounds = *workspace.pane_bounds.borrow().get(&ids[0]).unwrap();
        gpui::point(bounds.center().x, bounds.top() + px(14.0))
    });
    hover(&mut window, source);
    let source = window
        .debug_bounds(source_selector)
        .unwrap_or_else(|| panic!("grip missing after hover at {source:?}; mode={mode:?}"))
        .center();
    let target = workspace.read_with(&window, |workspace, _| {
        workspace.pane_bounds.borrow().get(&ids[2]).unwrap().center()
    });
    let views = workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { panes, .. } = &workspace.tabs[0] else { panic!() };
        panes.iter().map(|pane| pane.view.clone()).collect::<Vec<_>>()
    });
    hover(&mut window, source);
    window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { focused, .. } = &workspace.tabs[0] else { panic!() };
        assert_eq!(*focused, ids[0], "the actual source grip must receive the press");
        let drag = workspace.pane_drag.as_ref().expect("header press must arm the gesture");
        assert_eq!(drag.pane, ids[0]);
        assert!(!drag.active, "the press alone must remain below the drag threshold");
    });
    window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        assert!(workspace.pane_drag.as_ref().is_some_and(|drag| drag.active));
        assert_eq!(workspace.pane_drag.as_ref().unwrap().target, Some(ids[2]));
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
    hover(&mut window, source);
    window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
    window.simulate_keystrokes("escape");
    window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
    workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { tree, .. } = &workspace.tabs[0] else { panic!() };
        assert_eq!(tree.leaves(), ids);
        assert!(workspace.pane_drag.is_none());
    });
    hover(&mut window, source);
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
    for (keys, leaves) in [
        ("ctrl-alt-shift-left", [ids[2], ids[0], ids[1]]),
        ("ctrl-alt-shift-up", [ids[2], ids[0], ids[1]]),
        ("ctrl-alt-shift-right", [ids[2], ids[1], ids[0]]),
    ] {
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        window.simulate_keystrokes(keys);
        workspace.read_with(&window, |workspace, _| {
            let WorkspaceTab::Terminal { tree, panes, focused, .. } = &workspace.tabs[0] else {
                panic!()
            };
            assert_eq!(tree.leaves(), leaves, "{keys}");
            assert_eq!(*focused, ids[0], "the moved pane keeps focus");
            assert_eq!(panes.iter().map(|pane| pane.view.clone()).collect::<Vec<_>>(), views);
        });
    }
    window.update(|window, cx| {
        assert!(views[0].read(cx).focus_handle(cx).is_focused(window));
    });
    // Paint the left pane while zoomed before resolving Right. Its cached
    // rectangle now spans the workspace; the middle pane is still the neighbour.
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    let split_width = workspace.read_with(&window, |workspace, _| {
        workspace.pane_bounds.borrow().get(&ids[2]).unwrap().size.width
    });
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_pane(0, ids[2], window, cx));
    });
    window.simulate_keystrokes("ctrl-shift-enter");
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { zoomed, .. } = &workspace.tabs[0] else { panic!() };
        assert!(*zoomed);
        assert!(workspace.pane_bounds.borrow().get(&ids[2]).unwrap().size.width > split_width);
        let bounds = workspace.pane_bounds.borrow();
        assert!(
            bounds.get(&ids[1]).unwrap().center().x < bounds.get(&ids[2]).unwrap().center().x,
            "the painted middle pane must be behind the zoomed centre: {bounds:?}"
        );
    });
    window.simulate_keystrokes("ctrl-alt-shift-right");
    workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { tree, panes, focused, zoomed, .. } = &workspace.tabs[0] else {
            panic!()
        };
        assert_eq!(tree.leaves(), [ids[1], ids[2], ids[0]], "zoom must not change the neighbour");
        assert_eq!(*focused, ids[2]);
        assert!(!*zoomed);
        assert_eq!(panes.iter().map(|pane| pane.view.clone()).collect::<Vec<_>>(), views);
    });
    window.update(|window, cx| {
        assert!(views[2].read(cx).focus_handle(cx).is_focused(window));
    });
    window.simulate_keystrokes("ctrl-alt-shift-left");
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_pane(0, ids[0], window, cx));
    });
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    let grip = workspace.read_with(&window, |workspace, _| {
        let bounds = *workspace.pane_bounds.borrow().get(&ids[0]).unwrap();
        gpui::point(bounds.center().x, bounds.top() + px(14.0))
    });
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
    hover(&mut window, grip);
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
