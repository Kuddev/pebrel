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
        crate::gpui_shell::init(cx);
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
            workspace.vcs_list.sync(Some(data.clone()), GitPanelView::Changes, Some(root.clone()));
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
            );
            assert_eq!(workspace.vcs_list.scroll.logical_scroll_top().item_ix, before.item_ix);
            workspace.vcs_list.rendered.clear();
        });
        probe.update(cx, |_, cx| cx.notify());
        let _ = window.draw(cx);
    });
    let rendered = owner.read_with(cx, |workspace, _| workspace.vcs_list.rendered.clone());
    assert!(!rendered.is_empty() && rendered.len() < 32, "5000 rows: {rendered:?}");
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
    list.sync(Some(data.clone()), GitPanelView::Changes, Some(PathBuf::from("/repo")));
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
    list.sync(Some(data), GitPanelView::Conflicts, Some(PathBuf::from("/repo")));
    assert_eq!(list.scroll.item_count(), 2);
    assert_eq!(list.scroll.logical_scroll_top().item_ix, 0);
}
