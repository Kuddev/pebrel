//! 粘性目录复用虚拟列表同帧的滚动值，不另起滚轮监听或滚动状态。

use super::*;
use gpui::{App, Bounds, UniformListDecoration, WeakEntity};

fn pinned_directory(
    rows: &[crate::display::side_panel::FileRow],
    first: usize,
    offset: Pixels,
    height: Pixels,
) -> Option<(usize, Pixels)> {
    let first_row = rows.get(first)?;
    let index = (0..=first).rev().find(|&ix| {
        let row = &rows[ix];
        row.is_dir
            && row.expanded
            && !row.is_parent
            && first_row.path.starts_with(&row.path)
            && height * ix + offset < px(0.0)
    })?;
    let end = rows[index + 1..]
        .iter()
        .position(|row| row.depth <= rows[index].depth)
        .map_or(rows.len(), |relative| index + 1 + relative);
    Some((index, (height * end + offset - height).min(px(0.0))))
}

pub(super) struct DirectoryHeader {
    workspace: WeakEntity<NebulaWorkspace>,
}

impl DirectoryHeader {
    pub(super) fn new(workspace: WeakEntity<NebulaWorkspace>) -> Self {
        Self { workspace }
    }
}

impl UniformListDecoration for DirectoryHeader {
    fn compute(
        &self,
        visible: Range<usize>,
        _bounds: Bounds<Pixels>,
        offset: Point<Pixels>,
        height: Pixels,
        _count: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> gpui::AnyElement {
        self.workspace
            .update(cx, |workspace, cx| {
                if !workspace.side_panel.search.trim().is_empty() {
                    return div().into_any_element();
                }
                let Some((index, push)) = pinned_directory(
                    workspace.side_panel.file_rows(),
                    visible.start,
                    offset.y,
                    height,
                ) else {
                    return div().into_any_element();
                };
                // decoration 的坐标原点已含 offset；抵消它才能钉在视口顶端。
                // 子树末尾推动标题离开，后续兄弟目录不会被旧标题遮挡。
                div()
                    .relative()
                    .w_full()
                    .child(
                        div()
                            .id("file-tree-sticky-header")
                            .debug_selector(|| "file-tree-sticky-header".into())
                            .absolute()
                            .top(-offset.y + push)
                            .left(-offset.x)
                            .w_full()
                            .h(height)
                            .bg(cx.theme().sidebar)
                            .occlude()
                            .child(workspace.render_file_tree_row_at(index, true, cx)),
                    )
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, depth: usize, directory: bool) -> crate::display::side_panel::FileRow {
        crate::display::side_panel::FileRow {
            path: path.into(),
            guest_path: None,
            name: path.into(),
            depth,
            is_dir: directory,
            expanded: directory,
            is_parent: false,
            ignored: false,
        }
    }

    #[test]
    fn header_pins_only_inside_its_subtree_and_yields_at_the_end() {
        let rows = vec![
            row("a", 0, true),
            row("a/x", 1, false),
            row("a/y", 1, false),
            row("b", 0, true),
            row("b/z", 1, false),
        ];
        let height = px(34.0);
        assert_eq!(pinned_directory(&rows, 0, px(0.0), height), None);
        assert_eq!(pinned_directory(&rows, 1, px(-34.0), height), Some((0, px(0.0))));
        assert_eq!(pinned_directory(&rows, 2, px(-85.0), height), Some((0, px(-17.0))));
        assert_eq!(pinned_directory(&rows, 3, px(-102.0), height), None);
        assert_eq!(pinned_directory(&rows, 4, px(-136.0), height), Some((3, px(0.0))));
    }

    #[test]
    fn nested_directory_returns_to_its_parent_for_a_sibling_file() {
        let rows = vec![
            row("a", 0, true),
            row("a/nested", 1, true),
            row("a/nested/x", 2, false),
            row("a/y", 1, false),
            row("b", 0, true),
        ];
        assert_eq!(pinned_directory(&rows, 2, px(-68.0), px(34.0)), Some((1, px(0.0))));
        assert_eq!(pinned_directory(&rows, 3, px(-102.0), px(34.0)), Some((0, px(0.0))));
    }
}
