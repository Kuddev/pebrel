use super::*;
use crate::display::side_panel::{GitInfo, GitPanelView};
use gpui::{ListOffset, Modifiers, TestAppContext};

struct ListProbe(Entity<NebulaWorkspace>);
impl Render for ListProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(320.0))
            .h(px(300.0))
            .flex()
            .child(self.0.update(cx, |workspace, cx| workspace.render_vcs_list(cx)))
    }
}

fn snapshot(count: usize) -> Arc<GitInfo> {
    Arc::new(GitInfo {
        unstaged: (0..count).map(|ix| ('M', format!("src/file-{ix}.rs"))).collect(),
        ..GitInfo::default()
    })
}

#[gpui::test]
fn git_list_repaints_visible_rows_and_clicks_the_scrolled_path(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::init(cx, None);
        cx.set_reduce_motion(true);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
    });
    let data = snapshot(237);
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let mut owner = None;
    let mut probe = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
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
            workspace.vcs_list.sync(
                Some(data.clone()),
                GitPanelView::Changes,
                Some(root.clone()),
                false,
            );
        });
        let view = cx.new(|_| ListProbe(workspace.clone()));
        owner = Some(workspace);
        probe = Some(view.clone());
        Root::new(view, window, cx)
    });
    let owner = owner.unwrap();
    let probe = probe.unwrap();
    cx.run_until_parked();
    for _ in 0..3 {
        cx.update(|window, cx| {
            owner.update(cx, |workspace, _| {
                workspace.vcs_list.rendered.clear();
                workspace.vcs_list.sync(
                    Some(data.clone()),
                    GitPanelView::Changes,
                    Some(root.clone()),
                    false,
                );
            });
            probe.update(cx, |_, cx| cx.notify());
            let _ = window.draw(cx);
        });
        let rendered = owner.read_with(cx, |workspace, _| workspace.vcs_list.rendered.clone());
        assert!(!rendered.is_empty());
        assert!(rendered.len() < 32, "300px viewport must not build 237 rows: {rendered:?}");
        assert!(!rendered.contains(&230));
    }
    cx.update(|window, cx| {
        owner.update(cx, |workspace, _| {
            workspace
                .vcs_list
                .scroll
                .scroll_to(ListOffset { item_ix: 220, offset_in_item: px(0.0) });
        });
        probe.update(cx, |_, cx| cx.notify());
        let _ = window.draw(cx);
    });
    // Descriptor 0 is a heading, so row 220 is file 219, not file 220.
    let bounds = cx.debug_bounds("git-tree-row-changes-219-src/file-219.rs").unwrap();
    assert!(bounds.size.height >= px(30.0));
    cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
    assert_eq!(
        owner.read_with(cx, |workspace, _| workspace.side_panel.selected.clone()),
        Some(root.join("src/file-219.rs"))
    );

    cx.update(|window, cx| {
        owner.update(cx, |workspace, _| {
            let before = workspace.vcs_list.scroll.logical_scroll_top();
            workspace.vcs_list.sync(
                Some(snapshot(5000)),
                GitPanelView::Changes,
                Some(root.clone()),
                false,
            );
            assert_eq!(workspace.vcs_list.scroll.logical_scroll_top().item_ix, before.item_ix);
            workspace.vcs_list.rendered.clear();
        });
        probe.update(cx, |_, cx| cx.notify());
        let _ = window.draw(cx);
    });
    let rendered = owner.read_with(cx, |workspace, _| workspace.vcs_list.rendered.clone());
    assert!(!rendered.is_empty() && rendered.len() < 32, "5000 rows: {rendered:?}");

    let history = Arc::new(GitInfo {
        history: vec![
            crate::display::side_panel::GitCommit {
                full_hash: "child".into(),
                parent_hashes: vec!["parent".into()],
                author: "Alice".into(),
                ..Default::default()
            },
            crate::display::side_panel::GitCommit {
                full_hash: "parent".into(),
                author: "Bob".into(),
                ..Default::default()
            },
        ],
        ..GitInfo::default()
    });
    cx.update(|window, cx| {
        owner.update(cx, |workspace, _| {
            workspace.vcs_list.sync(
                Some(history),
                GitPanelView::History,
                Some(root.clone()),
                false,
            );
        });
        probe.update(cx, |_, cx| cx.notify());
        let _ = window.draw(cx);
    });
    let child = cx.debug_bounds("git-history-0").unwrap();
    let parent = cx.debug_bounds("git-history-1").unwrap();
    assert_eq!(child.bottom(), parent.top(), "history graph rows must join without padding");
}

/// 悬浮展示完整文件名/完整父目录，且**真被截断才展示**（用户 10-23 需求）：
/// 宽裕行（文件名与父目录都放得下）悬浮不出任何提示；超宽行悬浮在文件名
/// 上给完整文件名、在父目录上给完整父目录，且互不越位。本地与 SSH 远端
/// 共用 `render_vcs_row`，本地形态的断言即代表两个面板。
#[gpui::test]
fn change_rows_hover_reveals_truncated_name_and_path_only(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::init(cx, None);
        cx.set_reduce_motion(true);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
    });
    // 行 0 整行放得下（不截断）；行 1 文件名与父目录都远超行宽（截断）。
    // 应用内嵌字体的度量跨平台一致，宽度断言不随宿主字体漂移。
    let data = Arc::new(GitInfo {
        unstaged: vec![
            ('M', "src/gpui_shell/file.rs".into()),
            (
                'M',
                "deeply/nested/directory/tree/a_very_long_file_name_that_will_surely_truncate.rs"
                    .into(),
            ),
        ],
        ..GitInfo::default()
    });
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
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
            workspace.vcs_list.sync(
                Some(data.clone()),
                GitPanelView::Changes,
                Some(root.clone()),
                false,
            );
        });
        let view = cx.new(|_| ListProbe(workspace.clone()));
        owner = Some(workspace);
        probe = Some(view.clone());
        Root::new(view, window, cx)
    });
    let owner = owner.unwrap();
    let _probe = probe.unwrap();
    // add_window_view 借出的是 &mut VisualTestContext；clone 成持有值，hover
    // 序列里可变借用（同 settings_navigation::ui_tests 的驱动句柄形态）。
    let mut cx = window.clone();
    cx.run_until_parked();

    // tooltip 的显示延迟走测试虚拟时钟：移动指针 → 推进时钟 → 泵完任务 →
    // 重画一帧（tooltip 是窗口覆盖层，画帧时才进 debug_bounds）。
    let hover = |cx: &mut gpui::VisualTestContext, point: gpui::Point<gpui::Pixels>| {
        cx.simulate_mouse_move(point, None, Modifiers::default());
        cx.executor().advance_clock(std::time::Duration::from_millis(600));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    };
    let _ = &owner;

    // 宽裕行：文件名与父目录都不出提示（真被截断才显示）。
    let fits_name =
        cx.debug_bounds("vcs-file-name-changes-0").expect("file name element is addressable");
    hover(&mut cx, fits_name.center());
    assert!(cx.debug_bounds("vcs-file-tip").is_none(), "a file name that fits shows no tooltip");
    let fits_parent =
        cx.debug_bounds("vcs-parent-changes-0").expect("parent path element is addressable");
    hover(&mut cx, fits_parent.center());
    assert!(cx.debug_bounds("vcs-path-tip").is_none(), "a parent path that fits shows no tooltip");

    // 超宽行：悬浮文件名给完整文件名，不出路径提示。
    let long_name =
        cx.debug_bounds("vcs-file-name-changes-1").expect("long file name element is addressable");
    hover(&mut cx, long_name.center());
    assert!(
        cx.debug_bounds("vcs-file-tip").is_some(),
        "hovering a truncated file name shows the full name"
    );
    assert!(
        cx.debug_bounds("vcs-path-tip").is_none(),
        "the file name tooltip does not preempt the parent path tooltip"
    );

    // 悬浮父目录给完整父目录，文件名提示让位。
    let long_parent =
        cx.debug_bounds("vcs-parent-changes-1").expect("long parent path element is addressable");
    hover(&mut cx, long_parent.center());
    assert!(
        cx.debug_bounds("vcs-path-tip").is_some(),
        "hovering a truncated parent path shows the full directory"
    );
    assert!(
        cx.debug_bounds("vcs-file-tip").is_none(),
        "the file name tooltip yields once the pointer moves to the parent path"
    );
}

#[test]
fn grouping_keeps_conflict_and_stage_actions_attached_to_the_correct_paths() {
    use list_model::{RowOps, VcsRow};
    let data = Arc::new(GitInfo {
        conflicts: vec![('U', "conflict.rs".into())],
        staged: vec![('U', "conflict.rs".into()), ('A', "staged.rs".into())],
        unstaged: vec![('U', "conflict.rs".into()), ('M', "modified.rs".into())],
        ..GitInfo::default()
    });
    let mut list = VcsList::default();
    list.sync(Some(data.clone()), GitPanelView::Changes, Some(PathBuf::from("/repo")), false);
    let paths: Vec<_> = (0..list.scroll.item_count())
        .filter_map(|ix| match list.row(ix).unwrap() {
            VcsRow::Change { relative_path, ops, .. } => Some((relative_path.as_str(), *ops)),
            _ => None,
        })
        .collect();
    assert!(
        paths
            == [
                ("conflict.rs", RowOps::Conflict),
                ("staged.rs", RowOps::Staged),
                ("modified.rs", RowOps::Unstaged)
            ]
    );
    list.sync(Some(data), GitPanelView::Conflicts, Some(PathBuf::from("/repo")), false);
    assert_eq!(list.scroll.item_count(), 2);
    assert_eq!(list.scroll.logical_scroll_top().item_ix, 0);
}
