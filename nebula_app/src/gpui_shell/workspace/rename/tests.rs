use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.update(|window, cx| {
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
        (Self { workspace, ids, headers, _directory: directory }, window)
    }

    fn edit(&self, index: usize, cx: &mut VisualTestContext) {
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
