use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

struct RenameFixture {
    workspace: Entity<NebulaWorkspace>,
    ids: [u64; 2],
    headers: [&'static str; 2],
    _directory: tempfile::TempDir,
}

impl RenameFixture {
    fn open(cx: &mut TestAppContext) -> (Self, VisualTestContext) {
        let directory = tempfile::tempdir().unwrap();
        // An absolute missing program avoids PATH lookup and never starts a user shell.
        let program = directory.path().join("pebrel-test-missing-shell");
        let hub = crate::runtime_api::RuntimeHub::new();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::gpui_shell::math_view::register(cx);
            crate::gpui_shell::file_editor::init(cx);
            super::super::init(cx);
            windowing::initialize(cx, hub.clone());
        });
        let mut result = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| {
                NebulaWorkspace::new(
                    window,
                    None,
                    None,
                    1,
                    hub,
                    windowing::WorkspaceStartup::Empty,
                    windowing::WindowRole::Regular,
                    cx,
                )
            });
            let ids = workspace.update(cx, |workspace, cx| {
                workspace.tabs_position = nebula_settings::TabsPositionName::Sidebar;
                let panes = (0..2)
                    .map(|_| {
                        workspace.new_pane(
                            (80, 24),
                            TerminalLaunch::Local {
                                cwd: Some(directory.path().to_path_buf()),
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
                let ids = [panes[0].id, panes[1].id];
                let mut tree = SplitTree::leaf(ids[0]);
                assert!(tree.split_leaf(ids[0], ids[1], SplitDirection::LeftRight, 0.5));
                workspace.insert_tab_at(
                    0,
                    WorkspaceTab::Terminal {
                        panes,
                        tree,
                        focused: ids[0],
                        zoomed: false,
                        broadcast: false,
                    },
                    TabMeta { custom_name: Some("group-title".into()), ..Default::default() },
                );
                workspace.focus_active(window, cx);
                cx.notify();
                ids
            });
            result = Some((workspace.clone(), ids));
            Root::new(workspace, window, cx)
        });
        let (workspace, ids) = result.unwrap();
        // GPUI's test selector API requires static strings; allocate once per pane.
        let headers = ids.map(|id| &*Box::leak(format!("pane-header-grip-{id}").into_boxed_str()));
        let mut window = window.clone();
        window.update(|window, _| window.activate_window());
        settle(&mut window);
        // Startup settings are asynchronous; choose the test layout afterwards.
        workspace.update(&mut window, |workspace, cx| {
            workspace.tabs_position = nebula_settings::TabsPositionName::Sidebar;
            workspace.sync_settings_layout(true);
            cx.notify();
        });
        settle(&mut window);
        (Self { workspace, ids, headers, _directory: directory }, window)
    }

    fn open_menu(&self, index: usize, cx: &mut VisualTestContext) {
        let position = self.workspace.read_with(cx, |workspace, _| {
            let pane = *workspace.pane_bounds.borrow().get(&self.ids[index]).unwrap();
            gpui::point(pane.center().x, pane.top() + px(14.0))
        });
        cx.simulate_mouse_move(position, None, Modifiers::default());
        settle(cx);
        cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
        settle(cx);
        self.workspace.read_with(cx, |workspace, _| {
            assert_eq!(
                workspace.tab_menu.as_ref().and_then(|menu| menu.pane),
                Some(self.ids[index]),
                "right click must open the addressed pane menu"
            );
        });
        // The menu stays open after leaving the pane's hover area.
        cx.simulate_mouse_move(gpui::point(px(1.0), px(1.0)), None, Modifiers::default());
        settle(cx);
        cx.update(|window, _| {
            assert!(
                window.context_stack().iter().any(|context| context.contains("PopupMenu")),
                "deferred terminal focus must not steal focus from the menu"
            );
        });
    }

    fn edit(&self, index: usize, cx: &mut VisualTestContext) {
        let position = self.workspace.read_with(cx, |workspace, _| {
            let bounds = *workspace.pane_bounds.borrow().get(&self.ids[index]).unwrap();
            gpui::point(bounds.center().x, bounds.top() + px(14.0))
        });
        cx.simulate_mouse_move(position, None, Modifiers::default());
        settle(cx);
        let position =
            cx.debug_bounds(self.headers[index]).expect("pane title is visible").center();
        cx.simulate_click(position, Modifiers::default());
        cx.simulate_event(gpui::MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 2,
        });
        settle(cx);
        assert_eq!(
            self.workspace.read_with(cx, |w, _| w.pane_rename.as_ref().unwrap().pane_id),
            self.ids[index],
        );
    }

    fn save(&self, index: usize, value: &str, cx: &mut VisualTestContext) {
        self.edit(index, cx);
        cx.simulate_input(value);
        save_outside(cx);
        assert_eq!(self.name(index, cx).as_deref(), Some(value));
    }

    fn name(&self, index: usize, cx: &VisualTestContext) -> Option<String> {
        self.workspace.read_with(cx, |w, _| {
            let WorkspaceTab::Terminal { panes, .. } = &w.tabs[0] else { panic!() };
            panes.iter().find(|pane| pane.id == self.ids[index]).unwrap().custom_name.clone()
        })
    }

    fn value(&self, cx: &VisualTestContext) -> String {
        self.workspace.read_with(cx, |w, cx| {
            w.pane_rename.as_ref().unwrap().input.read(cx).value().to_string()
        })
    }
}

fn save_outside(cx: &mut VisualTestContext) {
    cx.simulate_click(gpui::point(px(2.0), px(200.0)), Modifiers::default());
    settle(cx);
}

fn undo(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-z",
        _ => "ctrl-z",
    });
}

fn redo(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-shift-z",
        _ => "ctrl-y",
    });
}

#[gpui::test]
fn saved_names_undo_redo_then_typing_uses_native_history(cx: &mut TestAppContext) {
    let (fixture, mut cx) = RenameFixture::open(cx);
    for name in ["第一次命名", "第二次命名", "第三次命名"] {
        fixture.save(0, name, &mut cx);
    }
    fixture.edit(0, &mut cx);
    for name in ["第二次命名", "第一次命名", ""] {
        undo(&mut cx);
        assert_eq!(fixture.value(&cx), name);
    }
    undo(&mut cx);
    assert_eq!(fixture.value(&cx), "", "the oldest value is automatic mode");
    for name in ["第一次命名", "第二次命名", "第三次命名"] {
        redo(&mut cx);
        assert_eq!(fixture.value(&cx), name);
    }
    redo(&mut cx);
    assert_eq!(fixture.value(&cx), "第三次命名");
    cx.simulate_input("尚未保存");
    undo(&mut cx);
    assert_eq!(fixture.value(&cx), "第三次命名");
    undo(&mut cx);
    assert_eq!(fixture.value(&cx), "第三次命名", "text undo must not enter saved-name history");
    redo(&mut cx);
    assert_eq!(fixture.value(&cx), "尚未保存");
    cx.simulate_keystrokes("escape");
    settle(&mut cx);
    assert_eq!(fixture.name(0, &cx).as_deref(), Some("第三次命名"));
    fixture.edit(0, &mut cx);
    undo(&mut cx);
    assert_eq!(fixture.value(&cx), "第二次命名", "cancelled edits must not add a saved name");
    assert_eq!(fixture.name(1, &cx), None);
}

#[gpui::test]
fn saved_names_keep_only_32_previous_changes(cx: &mut TestAppContext) {
    let (fixture, mut cx) = RenameFixture::open(cx);
    for index in 0..35 {
        fixture.save(0, &format!("name-{index}"), &mut cx);
    }
    // Saving an unchanged value must not consume another history slot.
    fixture.edit(0, &mut cx);
    save_outside(&mut cx);
    fixture.edit(0, &mut cx);
    for index in (2..34).rev() {
        undo(&mut cx);
        assert_eq!(fixture.value(&cx), format!("name-{index}"));
    }
    undo(&mut cx);
    assert_eq!(fixture.value(&cx), "name-2", "older names have been evicted");
    for index in 3..35 {
        redo(&mut cx);
        assert_eq!(fixture.value(&cx), format!("name-{index}"));
    }
    redo(&mut cx);
    assert_eq!(fixture.value(&cx), "name-34");
    assert_eq!(fixture.name(1, &cx), None);
}

#[gpui::test]
fn custom_pane_names_survive_snapshot_json_and_workspace_restore(cx: &mut TestAppContext) {
    let (fixture, mut cx) = RenameFixture::open(cx);
    fixture.save(0, "前端", &mut cx);
    fixture.save(1, "后端", &mut cx);
    let snapshot = fixture.workspace.read_with(&cx, |w, cx| w.snapshot_session(cx));
    let json = serde_json::to_value(&snapshot).unwrap();
    // Check the actual payload, not just equality of two potentially empty snapshots.
    assert_eq!(json["tabs"][0]["layout"]["first"]["custom_name"], "前端");
    assert_eq!(json["tabs"][0]["layout"]["second"]["custom_name"], "后端");
    let restored: crate::session::Session = serde_json::from_value(json).unwrap();
    assert_eq!(snapshot, restored);
    cx.update(|window, cx| {
        fixture.workspace.update(cx, |w, cx| {
            assert!(w.restore_tab(&restored.tabs[0], false, window, cx));
        });
    });
    fixture.workspace.read_with(&cx, |w, cx| {
        let WorkspaceTab::Terminal { panes, tree, .. } = &w.tabs[1] else { panic!() };
        let names = tree
            .leaves()
            .iter()
            .map(|id| {
                let pane = panes.iter().find(|pane| pane.id == *id).unwrap();
                assert!(!fixture.ids.contains(id), "restored panes have new identities");
                assert!(pane.name_history.is_empty(), "undo history is not persisted");
                pane.custom_name.as_deref()
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec![Some("前端"), Some("后端")]);
        assert_eq!(w.meta(1).custom_name.as_deref(), Some("group-title"));
        let again = w.snapshot_session(cx);
        assert_eq!(again.tabs[1].layout, restored.tabs[0].layout);
    });
}

#[gpui::test]
fn sidebar_panes_keep_their_full_surface_and_center_the_title(cx: &mut TestAppContext) {
    let (fixture, mut window) = RenameFixture::open(cx);
    window.simulate_resize(gpui::size(px(1100.0), px(720.0)));
    settle(&mut window);
    let body_selector = &*Box::leak(format!("pane-body-{}", fixture.ids[0]).into_boxed_str());
    let before = window.debug_bounds(body_selector).unwrap();
    fixture.workspace.read_with(&window, |workspace, _| {
        let pane = *workspace.pane_bounds.borrow().get(&fixture.ids[0]).unwrap();
        assert_eq!(before, pane, "sidebar chrome must not reserve a header row");
    });
    let hover = gpui::point(before.center().x, before.top() + px(14.0));
    window.simulate_mouse_move(hover, None, Modifiers::default());
    settle(&mut window);
    let grip = window.debug_bounds(fixture.headers[0]).unwrap();
    assert_eq!(grip.center().x, before.center().x);
    assert!(grip.size.width >= px(32.0));
    assert_eq!(window.debug_bounds(body_selector).unwrap(), before);
    window.simulate_mouse_move(gpui::point(px(1.0), px(1.0)), None, Modifiers::default());
    settle(&mut window);
    assert_eq!(window.debug_bounds(body_selector).unwrap(), before);

    for width in [1100.0, 620.0] {
        window.simulate_resize(gpui::size(px(width), px(720.0)));
        for collapsed in [false, true] {
            fixture.workspace.update(&mut window, |workspace, cx| {
                workspace.sidebar_collapsed = collapsed;
                if let WorkspaceTab::Terminal { panes, .. } = &mut workspace.tabs[0] {
                    panes[0].custom_name =
                        Some("A very long window title with 中文 and a distant tail".repeat(4));
                }
                cx.notify();
            });
            settle(&mut window);
            let title = window.debug_bounds("active-pane-title").unwrap();
            let broadcast = window.debug_bounds("toolbar-broadcast").unwrap();
            assert!((f32::from(title.center().x) - width / 2.0).abs() < 1.0);
            assert!(title.right() <= broadcast.left());
        }
    }
    // The retained top-tab layout still reserves the original 24px header.
    fixture.workspace.update(&mut window, |workspace, cx| {
        workspace.tabs_position = nebula_settings::TabsPositionName::Top;
        cx.notify();
    });
    settle(&mut window);
    let body = window.debug_bounds(body_selector).unwrap();
    fixture.workspace.read_with(&window, |workspace, _| {
        let pane = *workspace.pane_bounds.borrow().get(&fixture.ids[0]).unwrap();
        assert_eq!(body.top() - pane.top(), px(super::super::pane_header::PANE_HEADER_H));
    });
    fixture.edit(0, &mut window);
    window.simulate_input("Top-mode name");
    window.simulate_keystrokes("enter");
    settle(&mut window);
    assert_eq!(fixture.name(0, &window).as_deref(), Some("Top-mode name"));
}

#[gpui::test]
fn sidebar_broadcast_and_grip_menu_target_the_current_pane(cx: &mut TestAppContext) {
    let (fixture, mut window) = RenameFixture::open(cx);
    let broadcast = window.debug_bounds("toolbar-broadcast").unwrap().center();
    window.simulate_click(broadcast, Modifiers::default());
    fixture.workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { focused, broadcast, .. } = &workspace.tabs[0] else {
            panic!()
        };
        assert!(*broadcast);
        assert_eq!(*focused, fixture.ids[0]);
    });
    window.update(|window, cx| {
        let view = fixture.workspace.read(cx).tabs[0].focused_view().unwrap();
        assert!(view.read(cx).focus_handle(cx).is_focused(window));
    });
    // Open the second pane's real context menu, then use its keyboard navigation.
    fixture.open_menu(1, &mut window);
    window.simulate_keystrokes("down");
    settle(&mut window);
    window.simulate_keystrokes("enter");
    settle(&mut window);
    fixture.workspace.read_with(&window, |workspace, _| {
        assert_eq!(
            workspace.pane_rename.as_ref().expect("rename action opens the title editor").pane_id,
            fixture.ids[1]
        );
    });
    window.simulate_input("Second pane");
    window.simulate_keystrokes("enter");
    settle(&mut window);
    assert_eq!(fixture.name(1, &window).as_deref(), Some("Second pane"));
    assert_eq!(
        fixture
            .workspace
            .read_with(&window, |workspace, _| workspace.meta(0).custom_name.clone())
            .as_deref(),
        Some("group-title")
    );
    for expected_zoom in [true, false] {
        fixture.open_menu(1, &mut window);
        window.simulate_keystrokes("down down enter");
        settle(&mut window);
        fixture.workspace.read_with(&window, |workspace, _| {
            let WorkspaceTab::Terminal { zoomed, focused, .. } = &workspace.tabs[0] else {
                panic!()
            };
            assert_eq!(*zoomed, expected_zoom);
            assert_eq!(*focused, fixture.ids[1]);
            assert!(workspace.tab_menu.is_none());
        });
        window.update(|window, cx| {
            let view = fixture.workspace.read(cx).tabs[0].focused_view().unwrap();
            assert!(view.read(cx).focus_handle(cx).is_focused(window));
        });
    }
    fixture.open_menu(1, &mut window);
    window.simulate_keystrokes("escape");
    settle(&mut window);
    window.update(|window, cx| {
        let workspace = fixture.workspace.read(cx);
        assert!(workspace.tab_menu.is_none());
        assert!(
            workspace.tabs[0].focused_view().unwrap().read(cx).focus_handle(cx).is_focused(window)
        );
    });
    fixture.open_menu(1, &mut window);
    window.simulate_keystrokes("down down down enter");
    settle(&mut window);
    fixture.workspace.read_with(&window, |workspace, _| {
        let WorkspaceTab::Terminal { panes, focused, broadcast, .. } = &workspace.tabs[0] else {
            panic!()
        };
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].id, fixture.ids[0]);
        assert_eq!(*focused, fixture.ids[0]);
        assert!(!broadcast, "closing to one pane disables broadcast");
    });
    assert!(window.debug_bounds("toolbar-broadcast").is_none());
}
