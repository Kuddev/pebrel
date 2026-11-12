//! Window pointer capture and lightweight preview for a split divider.

use super::*;
use gpui::{DispatchPhase, MouseMoveEvent, MouseUpEvent, Point};

pub(super) struct SplitDrag {
    tab: usize,
    tab_id: crate::runtime_api::tabs::TabId,
    path: Vec<bool>,
    pub(super) direction: SplitDirection,
    cell: f32,
    press: Point<Pixels>,
    pointer_offset: f32,
    preview_ratio: f32,
    close_target: Option<bool>,
}

impl SplitDrag {
    fn ratio(&self, viewport: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
        let rect = to_split_rect(&viewport);
        nebula_split::drag_ratio(
            self.direction,
            rect,
            DIVIDER_GAP,
            self.cell,
            f32::from(position.x),
            f32::from(position.y),
        ) - self.pointer_offset
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SplitDragVisual {
    pub(super) divider: nebula_split::Rect,
    pub(super) close_area: Option<nebula_split::Rect>,
}

/// Compute the cheap split-drag overlay without changing either pane's layout.
/// `close_target` uses [`nebula_split::drag_close_target`] semantics: `false`
/// is the first (left/top) child, `true` the second (right/bottom) child.
pub(super) fn split_drag_visual_geometry(
    direction: SplitDirection,
    viewport: nebula_split::Rect,
    preview_ratio: f32,
    close_target: Option<bool>,
) -> SplitDragVisual {
    let usable = match direction {
        SplitDirection::LeftRight => (viewport.w - DIVIDER_GAP).max(0.0),
        SplitDirection::TopBottom => (viewport.h - DIVIDER_GAP).max(0.0),
    };
    let split = usable * preview_ratio.clamp(0.0, 1.0);
    let divider = match direction {
        SplitDirection::LeftRight => {
            nebula_split::Rect::new(viewport.x + split, viewport.y, DIVIDER_GAP, viewport.h)
        },
        SplitDirection::TopBottom => {
            nebula_split::Rect::new(viewport.x, viewport.y + split, viewport.w, DIVIDER_GAP)
        },
    };
    let close_area = close_target.map(|second| match (direction, second) {
        (SplitDirection::LeftRight, false) => {
            nebula_split::Rect::new(viewport.x, viewport.y, split, viewport.h)
        },
        (SplitDirection::LeftRight, true) => nebula_split::Rect::new(
            divider.x + divider.w,
            viewport.y,
            (viewport.w - split - divider.w).max(0.0),
            viewport.h,
        ),
        (SplitDirection::TopBottom, false) => {
            nebula_split::Rect::new(viewport.x, viewport.y, viewport.w, split)
        },
        (SplitDirection::TopBottom, true) => nebula_split::Rect::new(
            viewport.x,
            divider.y + divider.h,
            viewport.w,
            (viewport.h - split - divider.h).max(0.0),
        ),
    });
    SplitDragVisual { divider, close_area }
}

impl NebulaWorkspace {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn begin_split_drag(
        &mut self,
        tab: usize,
        path: &[bool],
        direction: SplitDirection,
        ratio: f32,
        press: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(bounds) = self.split_bounds.borrow().get(&(tab, path.to_vec())).copied() else {
            return false;
        };
        let Some(meta) = self.tab_meta.get(tab) else { return false };
        let Some(WorkspaceTab::Terminal { tree, .. }) = self.tabs.get_mut(tab) else {
            return false;
        };
        let Some(SplitTree::Split { preview_ratio, dragging, .. }) = tree.node_mut(path) else {
            return false;
        };
        let (width, height) = TerminalView::cell_metrics(window, cx);
        let cell = f32::from(if direction == SplitDirection::LeftRight { width } else { height });
        let mut drag = SplitDrag {
            tab,
            tab_id: meta.runtime_id.clone(),
            path: path.to_vec(),
            direction,
            cell,
            press,
            pointer_offset: 0.0,
            preview_ratio: ratio,
            close_target: None,
        };
        // The wider hit target can be grabbed on either side of the visible line.
        drag.pointer_offset = drag.ratio(bounds, press) - ratio;
        self.split_drag = Some(drag);
        *preview_ratio = None;
        *dragging = true;
        cx.notify();
        true
    }

    fn owns_split_drag(&self, drag: &SplitDrag) -> bool {
        self.tab_meta.get(drag.tab).is_some_and(|meta| meta.runtime_id == drag.tab_id)
    }

    pub(super) fn cancel_split_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.split_drag.take() else { return };
        if let Some(ix) = self.tab_meta.iter().position(|meta| meta.runtime_id == drag.tab_id)
            && let Some(WorkspaceTab::Terminal { tree, .. }) = self.tabs.get_mut(ix)
            && let Some(SplitTree::Split { preview_ratio, dragging, .. }) =
                tree.node_mut(&drag.path)
        {
            *preview_ratio = None;
            *dragging = false;
        }
        cx.notify();
    }

    pub(super) fn cancel_hidden_split_drag(&mut self, cx: &mut Context<Self>) {
        if self.split_drag.as_ref().is_some_and(|drag| {
            self.settings_open || drag.tab != self.active || !self.owns_split_drag(drag)
        }) {
            self.cancel_split_drag(cx);
        }
    }

    fn split_drag_bounds(&self, drag: &SplitDrag) -> Option<Bounds<Pixels>> {
        if !self.owns_split_drag(drag) {
            return None;
        }
        self.split_bounds.borrow().get(&(drag.tab, drag.path.clone())).copied()
    }

    fn update_split_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.split_drag.as_ref() else { return };
        let Some(bounds) = self.split_drag_bounds(drag) else { return };
        let raw = drag.ratio(bounds, position);
        let preview_ratio = nebula_split::preview_ratio(raw);
        let close_target = nebula_split::drag_close_target(raw);
        let drag = self.split_drag.as_mut().expect("drag already checked");
        if drag.preview_ratio != preview_ratio || drag.close_target != close_target {
            drag.preview_ratio = preview_ratio;
            drag.close_target = close_target;
            // GPUI coalesces frame requests. Every new position requests a frame,
            // including the last move of a short burst.
            cx.notify();
        }
    }

    /// Register before the press, so a quick move or release needs no intervening frame.
    pub(super) fn render_split_drag_capture(&self, cx: &Context<Self>) -> impl IntoElement {
        let workspace = cx.weak_entity();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let moves = workspace.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture
                        && let Some(workspace) = moves.upgrade()
                        && workspace.read(cx).split_drag.is_some()
                    {
                        // Windows can omit pressed_button on non-client moves.
                        // Only the actual release ends a held divider.
                        workspace.update(cx, |this, cx| this.update_split_drag(event.position, cx));
                        cx.stop_propagation();
                    }
                });
                let releases = workspace.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture
                        && event.button == MouseButton::Left
                        && let Some(workspace) = releases.upgrade()
                        && workspace.read(cx).split_drag.is_some()
                    {
                        workspace.update(cx, |this, cx| {
                            this.finish_split_drag(event.position, window, cx)
                        });
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    pub(super) fn split_capture_needed(&self) -> bool {
        self.split_drag.is_some()
            || (!self.settings_open
                && matches!(
                    self.tabs.get(self.active), Some(WorkspaceTab::Terminal { tree, zoomed: false, .. }) if !tree.is_leaf()
                ))
    }

    /// Visual-only divider/close preview. It lives above the stable pane tree,
    /// so moving it does not invoke `TerminalElement::prepaint` with new bounds.
    pub(super) fn split_drag_visual(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let drag = self.split_drag.as_ref()?;
        let viewport = self.split_drag_bounds(drag)?;
        let visual = split_drag_visual_geometry(
            drag.direction,
            to_split_rect(&viewport),
            drag.preview_ratio,
            drag.close_target,
        );
        let line = visual.divider;
        let close = visual.close_area.map(|area| {
            div()
                .absolute()
                .left(px(area.x))
                .top(px(area.y))
                .w(px(area.w))
                .h(px(area.h))
                .bg(cx.theme().warning.opacity(0.12))
        });
        Some(
            div()
                .absolute()
                .inset_0()
                .children(close)
                .child(
                    div()
                        .id("split-drag-preview-line")
                        .debug_selector(|| "split-drag-preview-line".into())
                        .absolute()
                        .left(px(line.x))
                        .top(px(line.y))
                        .w(px(line.w))
                        .h(px(line.h))
                        .bg(cx.theme().primary.opacity(0.88)),
                )
                .into_any_element(),
        )
    }

    /// 松手：关闭区按被挤压侧收尾（仅当那一侧是单叶——子树不做整树误杀，
    /// 按钳制带提交）；常规带比例吸附整数单元格后写回提交值。
    fn finish_split_drag(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.split_drag.take() else { return };
        if !self.owns_split_drag(&drag) {
            cx.notify();
            return;
        }
        let viewport = self.split_drag_bounds(&drag);
        let raw =
            viewport.filter(|_| position != drag.press).map(|bounds| drag.ratio(bounds, position));
        let cell = drag.cell;

        // 先裁定拖拽关闭；借用两段式（close_pane 需要 &mut self）。
        let mut close_target: Option<u64> = None;
        if let (Some(raw), Some(WorkspaceTab::Terminal { tree, .. })) =
            (raw, self.tabs.get_mut(drag.tab))
        {
            if let Some(squeezed_second) = nebula_split::drag_close_target(raw) {
                if let Some(SplitTree::Split { first, second, .. }) = tree.node_mut(&drag.path) {
                    let child = if squeezed_second { second.as_ref() } else { first.as_ref() };
                    if let SplitTree::Leaf(id) = child {
                        close_target = Some(*id);
                    }
                }
            }
        }
        if let Some(pane_id) = close_target {
            // 繁忙 pane 会先弹确认框而不会立刻 remove_leaf；必须先清掉拖拽态，
            // 否则确认框背后会残留一条永久高亮的分隔线。
            if let Some(WorkspaceTab::Terminal { tree, .. }) = self.tabs.get_mut(drag.tab)
                && let Some(SplitTree::Split { preview_ratio, dragging, .. }) =
                    tree.node_mut(&drag.path)
            {
                *preview_ratio = None;
                *dragging = false;
            }
            self.request_close_pane(drag.tab, pane_id, window, cx);
            cx.notify();
            return;
        }

        if let Some(WorkspaceTab::Terminal { tree, .. }) = self.tabs.get_mut(drag.tab) {
            if let Some(SplitTree::Split { ratio, preview_ratio, dragging, .. }) =
                tree.node_mut(&drag.path)
            {
                if let (Some(raw), Some(viewport)) = (raw, viewport) {
                    let vp = to_split_rect(&viewport);
                    let extent = match drag.direction {
                        SplitDirection::LeftRight => vp.w,
                        SplitDirection::TopBottom => vp.h,
                    };
                    *ratio = nebula_split::commit_ratio(
                        nebula_split::preview_ratio(raw),
                        extent,
                        DIVIDER_GAP,
                        cell,
                    );
                }
                *preview_ratio = None;
                *dragging = false;
            }
        }
        cx.notify();
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use crate::gpui_shell::terminal::view::TerminalLaunch;
    use gpui::{Modifiers, TestAppContext, point};

    #[gpui::test]
    fn a_short_drag_repaints_its_last_position_without_resizing_before_release(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
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
        let (_, visual) = cx.add_window_view(|window, cx| {
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
            workspace.update(cx, |workspace, cx| {
                let panes = (0..2)
                    .map(|_| {
                        workspace.new_pane(
                            (80, 24),
                            TerminalLaunch::Local {
                                cwd: Some(dir.path().into()),
                                shell: Some(nebula_terminal::tty::Shell::new(
                                    dir.path()
                                        .join("missing-fixture-shell")
                                        .to_string_lossy()
                                        .into_owned(),
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
                let tree = SplitTree::leaf(ids[0])
                    .joined(SplitTree::leaf(ids[1]), nebula_split::SplitNav::Right);
                workspace.insert_tab_at(
                    0,
                    WorkspaceTab::Terminal {
                        panes,
                        tree,
                        focused: ids[0],
                        zoomed: false,
                        broadcast: false,
                    },
                    TabMeta::default(),
                );
                cx.notify();
            });
            fixture = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let workspace = fixture.unwrap();
        let mut visual = visual.clone();
        let draw = |visual: &mut gpui::VisualTestContext| {
            visual.update(|window, cx| {
                window.refresh();
                let _ = window.draw(cx);
            });
        };
        draw(&mut visual);
        let hit = visual.debug_bounds("split-divider-0-[]").unwrap();
        // The half of the hit target over the second terminal must also receive the press.
        let start = point(hit.right() - px(1.0), hit.center().y);
        visual.simulate_mouse_move(start, None, Modifiers::default());
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        draw(&mut visual);
        let start_line = visual.debug_bounds("split-drag-preview-line").unwrap();
        let first_move = start + point(px(15.0), px(0.0));
        let last_move = start + point(px(43.0), px(0.0));
        visual.simulate_mouse_move(first_move, Some(MouseButton::Left), Modifiers::default());
        // Non-client Windows moves can have no reported pressed button.
        visual.simulate_mouse_move(last_move, None, Modifiers::default());
        draw(&mut visual);
        let line = visual.debug_bounds("split-drag-preview-line").unwrap();
        assert!((line.left() - start_line.left() - px(43.0)).abs() <= px(1.0));
        let ratio = |workspace: &NebulaWorkspace| match &workspace.tabs[0] {
            WorkspaceTab::Terminal { tree: SplitTree::Split { ratio, .. }, .. } => *ratio,
            _ => panic!("fixture split"),
        };
        assert_eq!(workspace.read_with(&visual, |workspace, _| ratio(workspace)), 0.5);
        visual.simulate_mouse_up(last_move, MouseButton::Left, Modifiers::default());
        draw(&mut visual);
        assert!(visual.debug_bounds("split-drag-preview-line").is_none());
        assert!(workspace.read_with(&visual, |workspace, _| ratio(workspace)) > 0.5);
    }
}
