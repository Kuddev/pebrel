use super::*;
use crate::display::side_panel::{GitInfo, GitPanelView, PanelView};
use gpui::{FocusHandle, ListOffset, Modifiers, TestAppContext, VisualTestContext};
use list_model::{ChangeLayout, RowOps, VcsRow};

fn files(list: &VcsList) -> Vec<(String, RowOps, usize)> {
    (0..list.scroll.item_count())
        .filter_map(|ix| match list.row(ix).unwrap() {
            VcsRow::Change { relative_path, ops, index, .. } => {
                Some((relative_path.clone(), *ops, *index))
            },
            _ => None,
        })
        .collect()
}

fn find_file(list: &VcsList, path: &str) -> usize {
    (0..list.scroll.item_count()).find(|ix| {
        matches!(list.row(*ix), Some(VcsRow::Change { relative_path, .. }) if relative_path == path)
    }).unwrap()
}

#[test]
fn tree_projection_keeps_original_paths_indices_operations_and_section_counts() {
    let data = Arc::new(GitInfo {
        conflicts: vec![('U', "module/conflict.rs".into())],
        staged: vec![('U', "module/conflict.rs".into()), ('R', "module/new name.rs".into())],
        unstaged: vec![
            ('U', "module/conflict.rs".into()),
            ('M', "module/deep/文件.rs".into()),
            ('?', "root.txt".into()),
            ('M', "other/文件.rs".into()),
            ('M', "literal\\name.rs".into()),
        ],
        ..GitInfo::default()
    });
    let mut list = VcsList::default();
    list.sync(Some(data), GitPanelView::Changes, Some(PathBuf::from("/repo")));
    let mut expected = files(&list);
    list.set_layout(ChangeLayout::Tree);
    let mut actual = files(&list);
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    actual.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(actual == expected);
    assert!(actual.iter().any(|(path, ops, index)| path == "module/new name.rs"
        && *ops == RowOps::Staged
        && *index == 1));
    assert!(actual.iter().any(|(path, ops, index)| path == "module/deep/文件.rs"
        && *ops == RowOps::Unstaged
        && *index == 1));
    let folders: Vec<_> = (0..list.scroll.item_count())
        .filter_map(|ix| match list.row(ix).unwrap() {
            VcsRow::Directory { section_id, relative_path, depth, count, .. } => {
                Some((*section_id, relative_path.as_str(), *depth, *count))
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        folders,
        [
            ("conflicts", "module", 0, 1),
            ("staged", "module", 0, 1),
            ("changes", "module", 0, 1),
            ("changes", "module/deep", 1, 1),
            ("changes", "other", 0, 1),
        ]
    );
    assert!(matches!(
        list.row(find_file(&list, "root.txt")),
        Some(VcsRow::Change { depth: 0, .. })
    ));
    list.toggle_directory("changes", "module");
    assert!(!files(&list).iter().any(|(path, _, _)| path == "module/deep/文件.rs"));
    assert!(files(&list).iter().any(|(path, _, _)| path == "module/new name.rs"));
    assert!(files(&list).iter().any(|(path, _, _)| path == "module/conflict.rs"));
    list.set_layout(ChangeLayout::List);
    assert_eq!(files(&list).len(), expected.len());
    list.set_layout(ChangeLayout::Tree);
    assert_eq!(files(&list).len(), expected.len() - 1);
}

#[test]
fn tree_refresh_preserves_scroll_identity_folds_and_resets_them_for_another_repository() {
    let make = |extra: bool| {
        Arc::new(GitInfo {
            unstaged: [
                if extra { Some(('M', "aaa/added.rs".into())) } else { None },
                Some(('M', "module/deep/a.rs".into())),
                Some(('M', "other/b.rs".into())),
            ]
            .into_iter()
            .flatten()
            .collect(),
            ..GitInfo::default()
        })
    };
    let mut list = VcsList::default();
    list.sync(Some(make(false)), GitPanelView::Changes, Some(PathBuf::from("/repo")));
    list.set_layout(ChangeLayout::Tree);
    let file = find_file(&list, "other/b.rs");
    list.scroll.scroll_to(ListOffset { item_ix: file, offset_in_item: px(3.0) });
    list.sync(Some(make(true)), GitPanelView::Changes, Some(PathBuf::from("/repo")));
    assert_eq!(list.scroll.logical_scroll_top().item_ix, find_file(&list, "other/b.rs"));
    list.toggle_directory("changes", "module/deep");
    list.sync(Some(make(false)), GitPanelView::Conflicts, Some(PathBuf::from("/repo")));
    list.sync(Some(make(true)), GitPanelView::Changes, Some(PathBuf::from("/repo")));
    assert!(!files(&list).iter().any(|(path, _, _)| path == "module/deep/a.rs"));
    list.sync(Some(make(true)), GitPanelView::Changes, Some(PathBuf::from("/another")));
    assert!(files(&list).iter().any(|(path, _, _)| path == "module/deep/a.rs"));
    assert_eq!(list.scroll.logical_scroll_top().item_ix, 0);
}

#[test]
fn tree_mode_keeps_history_and_svn_flat() {
    let mut list = VcsList::default();
    list.set_layout(ChangeLayout::Tree);
    list.sync(
        Some(Arc::new(GitInfo {
            unstaged: vec![('M', "a/b.rs".into())],
            vcs: crate::display::side_panel::VcsKind::Svn,
            ..GitInfo::default()
        })),
        GitPanelView::Changes,
        None,
    );
    assert!(!list.can_choose_layout());
    assert_eq!(list.scroll.item_count(), 2);
    list.sync(Some(Arc::new(GitInfo::default())), GitPanelView::History, None);
    assert!(!list.can_choose_layout());
    assert!(matches!(list.row(0), Some(VcsRow::Empty(Message::VcsNoHistory))));
}

struct TreeProbe {
    workspace: Entity<NebulaWorkspace>,
    focus: FocusHandle,
}

impl Render for TreeProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().track_focus(&self.focus).w(px(320.0)).h(px(340.0)).child(self.workspace.update(
            cx,
            |workspace, cx| {
                v_flex()
                    .size_full()
                    .child(workspace.render_vcs_layout_toggle(cx))
                    .child(workspace.render_vcs_list(cx))
            },
        ))
    }
}

fn tree_window<'a>(
    cx: &'a mut TestAppContext,
    data: Arc<GitInfo>,
    root: &Path,
) -> (Entity<NebulaWorkspace>, Entity<TreeProbe>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::init(cx, None);
        cx.set_reduce_motion(true);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
    });
    let mut owner = None;
    let mut probe = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                crate::runtime_api::RuntimeHub::new(),
                windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular,
                cx,
            )
        });
        workspace.update(cx, |workspace, _| {
            workspace.vcs_list.sync(Some(data), GitPanelView::Changes, Some(root.to_path_buf()));
        });
        let view =
            cx.new(|cx| TreeProbe { workspace: workspace.clone(), focus: cx.focus_handle() });
        owner = Some(workspace);
        probe = Some(view.clone());
        Root::new(view, window, cx)
    });
    (owner.unwrap(), probe.unwrap(), window)
}

fn draw(owner: &Entity<NebulaWorkspace>, probe: &Entity<TreeProbe>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        owner.update(cx, |workspace, _| workspace.vcs_list.rendered.clear());
        probe.update(cx, |_, cx| cx.notify());
        let _ = window.draw(cx);
    });
}

fn press(key: &str, cx: &mut VisualTestContext) {
    // simulate_keystrokes dispatches only KeyDown; native Button activation
    // requires the same unmodified key to be released without changing focus.
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    cx.run_until_parked();
}

#[gpui::test]
fn tree_controls_keyboard_and_scrolled_click_keep_the_file_identity(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("aa/deep")).unwrap();
    std::fs::write(root.join("aa/deep/同名.rs"), "// synthetic file\n").unwrap();
    let data = Arc::new(GitInfo {
        unstaged: [('M', "aa/deep/同名.rs".to_owned()), ('M', "bb/同名.rs".to_owned())]
            .into_iter()
            .chain((0..1000).map(|ix| ('M', format!("zz/file-{ix:04}.rs"))))
            .collect(),
        ..GitInfo::default()
    });
    let (owner, probe, cx) = tree_window(cx, data.clone(), root);
    draw(&owner, &probe, cx);
    cx.update(|window, cx| {
        let focus = probe.read(cx).focus.clone();
        focus.focus(window, cx);
    });
    // Tab into the real List and Tree controls, then activate Tree from the keyboard.
    press("tab", cx);
    press("tab", cx);
    press("enter", cx);
    assert!(owner.read_with(cx, |workspace, _| workspace.vcs_list.tree_active()));
    draw(&owner, &probe, cx);
    assert!(cx.debug_bounds("git-directory-changes-aa").is_some());
    press("tab", cx);
    press("left", cx);
    draw(&owner, &probe, cx);
    assert!(
        !owner
            .read_with(cx, |workspace, _| files(&workspace.vcs_list))
            .iter()
            .any(|(path, _, _)| path == "aa/deep/同名.rs")
    );
    press("right", cx);
    draw(&owner, &probe, cx);
    assert!(cx.debug_bounds("git-directory-changes-aa/deep").is_some());
    let folder = cx.debug_bounds("git-directory-changes-aa").unwrap();
    assert!(folder.size.height >= px(30.0));
    let padding = gpui::point(folder.left() + px(2.0), folder.top() + px(2.0));
    cx.simulate_click(padding, Modifiers::default());
    draw(&owner, &probe, cx);
    assert!(
        !owner
            .read_with(cx, |workspace, _| files(&workspace.vcs_list))
            .iter()
            .any(|(path, _, _)| path == "aa/deep/同名.rs")
    );
    cx.simulate_click(padding, Modifiers::default());
    draw(&owner, &probe, cx);
    let bounds = cx.debug_bounds("git-tree-row-changes-0-aa/deep/同名.rs").unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    assert_eq!(
        owner.read_with(cx, |workspace, _| workspace.side_panel.selected.clone()),
        Some(root.join("aa/deep/同名.rs"))
    );
    press("enter", cx);
    assert!(owner.read_with(cx, |workspace, cx| workspace.tabs.iter().any(|tab| {
        matches!(tab, WorkspaceTab::Code { view, .. } if view.read(cx).is_regular_path(&root.join("aa/deep/同名.rs")))
    })));
    cx.update(|_, cx| {
        owner.update(cx, |workspace, _| {
            let target = find_file(&workspace.vcs_list, "zz/file-0800.rs");
            workspace
                .vcs_list
                .scroll
                .scroll_to(ListOffset { item_ix: target, offset_in_item: px(0.0) });
        })
    });
    draw(&owner, &probe, cx);
    let rendered = owner.read_with(cx, |workspace, _| workspace.vcs_list.rendered.clone());
    assert!(!rendered.is_empty() && rendered.len() < 40, "1000-file viewport: {rendered:?}");
    let bounds = cx.debug_bounds("git-tree-row-changes-802-zz/file-0800.rs").unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    assert_eq!(
        owner.read_with(cx, |workspace, _| workspace.side_panel.selected.clone()),
        Some(root.join("zz/file-0800.rs"))
    );
    let mut refreshed = (*data).clone();
    refreshed.unstaged.insert(0, ('M', "a-new/file.rs".into()));
    cx.update(|_, cx| {
        owner.update(cx, |workspace, _| {
            workspace.vcs_list.sync(
                Some(Arc::new(refreshed)),
                GitPanelView::Changes,
                Some(root.to_path_buf()),
            );
            assert_eq!(
                workspace.vcs_list.scroll.logical_scroll_top().item_ix,
                find_file(&workspace.vcs_list, "zz/file-0800.rs")
            );
        })
    });
    draw(&owner, &probe, cx);
    assert!(cx.debug_bounds("git-tree-row-changes-803-zz/file-0800.rs").is_some());
    let flat = cx.debug_bounds("git-layout-list").unwrap();
    cx.simulate_click(flat.center(), Modifiers::default());
    draw(&owner, &probe, cx);
    assert!(!owner.read_with(cx, |workspace, _| workspace.vcs_list.tree_active()));
    assert!(cx.debug_bounds("git-tree-row-changes-803-zz/file-0800.rs").is_some());
}

struct GitFixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    configuration: PathBuf,
}

impl GitFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        let configuration = directory.path().join("configuration");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&configuration).unwrap();
        std::fs::create_dir(configuration.join("hooks")).unwrap();
        for name in ["global", "attributes"] {
            std::fs::write(configuration.join(name), b"").unwrap();
        }
        Self { _directory: directory, root, configuration }
    }

    fn command(&self) -> std::process::Command {
        // Git for Windows rejects NUL as a config file. Real empty files and
        // hooks stay outside the repository and live as long as this fixture.
        let mut command = std::process::Command::new("git");
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.configuration.join("global"))
            .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .arg("-C")
            .arg(&self.root)
            .stdin(std::process::Stdio::null());
        crate::platform::process::hidden_command(&mut command);
        command
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.command().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn configure(&self) {
        for (name, path) in [("core.hooksPath", "hooks"), ("core.attributesFile", "attributes")] {
            let output = self
                .command()
                .args(["config", "--local", name])
                .arg(self.configuration.join(path))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git config {name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

fn settle_panel(owner: &Entity<NebulaWorkspace>, root: &Path, cx: &mut VisualTestContext) {
    for _ in 0..1000 {
        let ready = cx.update(|_, cx| {
            owner.update(cx, |workspace, _| {
                workspace.side_panel.sync_at(Some(root.to_path_buf()), None);
                !workspace.side_panel.op_running()
                    && !workspace.side_panel.snapshot_pending()
                    && workspace.side_panel.git().is_some()
            })
        });
        if ready {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("synthetic Git panel operation did not settle");
}

#[gpui::test]
fn tree_stage_button_and_keyboard_unstage_target_the_same_named_file(cx: &mut TestAppContext) {
    let fixture = GitFixture::new();
    let root = fixture.root.as_path();
    fixture.git(&["init", "-q"]);
    fixture.configure();
    for path in ["aa/same.rs", "bb/same.rs"] {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::write(root.join(path), "initial\n").unwrap();
    }
    fixture.git(&["add", "--", "aa/same.rs", "bb/same.rs"]);
    fixture.git(&[
        "-c",
        "user.name=Regression test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-qm",
        "fixture",
    ]);
    for path in ["aa/same.rs", "bb/same.rs"] {
        std::fs::write(root.join(path), "modified\n").unwrap();
    }
    let data = Arc::new(crate::display::side_panel::read_git(root).unwrap());
    let (owner, probe, cx) = tree_window(cx, data, root);
    cx.update(|_, cx| {
        owner.update(cx, |workspace, _| {
            workspace.side_panel.toggle(PanelView::Git);
            workspace.side_panel.sync_at(Some(root.to_path_buf()), None);
            workspace.vcs_list.set_layout(ChangeLayout::Tree);
        })
    });
    settle_panel(&owner, root, cx);
    draw(&owner, &probe, cx);
    let row = cx.debug_bounds("git-tree-row-changes-1-bb/same.rs").unwrap();
    cx.simulate_mouse_move(row.center(), None, Modifiers::default());
    draw(&owner, &probe, cx);
    let stage = cx.debug_bounds("vcs-stage-changes-1").unwrap();
    cx.simulate_click(stage.center(), Modifiers::default());
    settle_panel(&owner, root, cx);
    assert_eq!(fixture.git(&["diff", "--cached", "--name-only"]), "bb/same.rs\n");
    cx.update(|_, cx| {
        owner.update(cx, |workspace, _| {
            workspace.vcs_list.sync(
                workspace.side_panel.git_snapshot(),
                GitPanelView::Changes,
                Some(root.to_path_buf()),
            );
        })
    });
    draw(&owner, &probe, cx);
    let row = cx.debug_bounds("git-tree-row-staged-0-bb/same.rs").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());
    // The focused file row's next tab stop is its real Unstage button.
    press("tab", cx);
    press("enter", cx);
    settle_panel(&owner, root, cx);
    assert!(fixture.git(&["diff", "--cached", "--name-only"]).is_empty());
    assert_eq!(
        owner.read_with(cx, |workspace, _| workspace.tabs.len()),
        0,
        "keyboard activation of Unstage must not open the containing file row"
    );
    assert_eq!(std::fs::read_to_string(root.join("aa/same.rs")).unwrap(), "modified\n");
}

#[gpui::test]
fn tree_conflict_keyboard_opens_the_matching_merge_editor(cx: &mut TestAppContext) {
    let fixture = GitFixture::new();
    let root = fixture.root.as_path();
    fixture.git(&["init", "-q", "-b", "fixture-main"]);
    fixture.configure();
    std::fs::create_dir_all(root.join("module/deep")).unwrap();
    let relative = "module/deep/文件.rs";
    let path = root.join(relative);
    std::fs::write(&path, "base\n").unwrap();
    fixture.git(&["add", "--", relative]);
    let commit = |message: &str| {
        fixture.git(&[
            "-c",
            "user.name=Regression test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qam",
            message,
        ]);
    };
    commit("base");
    fixture.git(&["checkout", "-qb", "fixture-side"]);
    std::fs::write(&path, "theirs\n").unwrap();
    commit("theirs");
    fixture.git(&["checkout", "-q", "fixture-main"]);
    std::fs::write(&path, "ours\n").unwrap();
    commit("ours");
    let merged = fixture
        .command()
        .args([
            "-c",
            "user.name=Regression test",
            "-c",
            "user.email=test@example.invalid",
            "merge",
            "--no-edit",
            "fixture-side",
        ])
        .output()
        .unwrap();
    assert!(!merged.status.success());
    let data = Arc::new(crate::display::side_panel::read_git(root).unwrap());
    assert_eq!(data.conflicts, [('U', relative.to_owned())]);
    let (owner, probe, cx) = tree_window(cx, data, root);
    cx.update(|_, cx| {
        owner.update(cx, |workspace, _| {
            workspace.side_panel.toggle(PanelView::Git);
            workspace.side_panel.sync_at(Some(root.to_path_buf()), None);
            workspace.vcs_list.set_layout(ChangeLayout::Tree);
        })
    });
    settle_panel(&owner, root, cx);
    draw(&owner, &probe, cx);
    let row = cx.debug_bounds("git-tree-row-conflicts-0-module/deep/文件.rs").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());
    cx.simulate_keystrokes("enter");
    let location = crate::display::side_panel::GitLocation::Local { root: root.to_path_buf() };
    assert!(owner.read_with(cx, |workspace, cx| workspace.tabs.iter().any(|tab| {
        matches!(tab, WorkspaceTab::Code { view, .. } if view.read(cx).matches_git_merge(&location, relative))
    })));
    cx.run_until_parked();
}
