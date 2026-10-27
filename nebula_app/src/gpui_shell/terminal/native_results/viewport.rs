//! 所有结果和普通终端行共用的纵向坐标；占位只影响显示，不改写 VT 网格。
use super::super::session::EventProxy;
use super::*;
use nebula_terminal::{Term, index::Line};
use std::cell::RefCell;

#[derive(Clone)]
struct Slot {
    id: u64,
    start: i64,
    rows: u16,
    height: f64,
    hide_source: bool,
}

#[derive(Clone, Default)]
pub(in crate::gpui_shell::terminal) struct Projection {
    slots: Vec<Slot>,
    top: f64,
    floor: i64,
    first: i64,
    bottom: i64,
    screen: usize,
    pub origin: i64,
    active: bool,
    scroll_generation: u64,
}

impl Projection {
    pub fn covers(&self, row: usize) -> bool {
        self.covers_source_row(row as i64)
    }

    pub fn covers_source_row(&self, row: i64) -> bool {
        let source = self.origin + row;
        self.active
            && self.slots.iter().any(|slot| {
                slot.hide_source
                    && source >= slot.start
                    && source < slot.start + i64::from(slot.rows)
            })
    }

    pub fn source_rows(&self, screen: usize) -> usize {
        if !self.active {
            return screen;
        }
        self.source_row(screen as f32).ceil().max(screen as f32).min(u16::MAX as f32) as usize
    }
    fn visual(&self, source: f64) -> f64 {
        let mut extra = 0.0;
        for slot in &self.slots {
            if source <= slot.start as f64 {
                break;
            }
            let used = (source - slot.start as f64).min(f64::from(slot.rows));
            extra += used * (slot.height / f64::from(slot.rows) - 1.0);
        }
        source + extra
    }
    fn source(&self, visual: f64) -> f64 {
        let mut extra = 0.0;
        for slot in &self.slots {
            let start = slot.start as f64 + extra;
            if visual <= start {
                break;
            }
            if visual < start + slot.height {
                return slot.start as f64 + (visual - start) * f64::from(slot.rows) / slot.height;
            }
            extra += slot.height - f64::from(slot.rows);
        }
        visual - extra
    }
    pub fn row(&self, source_viewport_row: f32) -> f32 {
        if !self.active {
            return source_viewport_row;
        }
        (self.visual(self.origin as f64 + f64::from(source_viewport_row)) - self.top) as f32
    }
    pub fn source_row(&self, visual_row: f32) -> f32 {
        if !self.active {
            return visual_row;
        }
        (self.source(self.top + f64::from(visual_row)) - self.origin as f64) as f32
    }
    pub fn scroll_state(&self) -> Option<(usize, usize)> {
        self.active.then(|| {
            let end = (self.visual(self.bottom as f64) - self.screen as f64)
                .max(self.visual(self.first as f64));
            (
                (end - self.top).max(0.0).round() as usize,
                (end - self.visual(self.first as f64)).max(0.0).ceil() as usize,
            )
        })
    }
    fn publish(&mut self, term: &mut Term<EventProxy>) {
        let min = self.visual(self.first as f64);
        let max = (self.visual(self.bottom as f64) - self.screen as f64).max(min);
        self.top = self.top.clamp(min, max);
        self.origin = self.source(self.top).floor() as i64;
        let line = Line((self.origin - self.floor) as i32);
        let offset = (-line.0).max(0) as usize;
        let current = term.grid().display_offset();
        if current != offset {
            term.scroll_display(nebula_terminal::grid::Scroll::Delta(
                offset as i32 - current as i32,
            ));
        }
        term.set_presentation_origin(Some(line));
        self.scroll_generation = term.presentation_scroll_generation();
    }
}

#[derive(Default)]
pub(in crate::gpui_shell::terminal) struct Views {
    pub entities: HashMap<u64, Entity<NativeResult>>,
    pub projection: RefCell<Projection>,
    epoch: Option<u64>,
    validated_history_end: i64,
    sources: Vec<(Weak<Surface>, u64, i64)>,
    expanded_folds: std::collections::HashSet<u64>,
    fold_anchor: Option<f64>,
}

impl Views {
    pub fn fold_expanded(&self, id: u64) -> bool {
        self.expanded_folds.contains(&id)
    }

    pub fn toggle_fold(&mut self, id: u64) {
        let p = self.projection.borrow();
        self.fold_anchor = Some(p.source(p.top));
        if !self.expanded_folds.remove(&id) {
            self.expanded_folds.insert(id);
        }
    }
    pub fn reconcile_retention(&mut self, cx: &mut gpui::App) {
        for entity in self.entities.values() {
            let result = entity.read(cx);
            if result.status != Status::Evicted
                && result
                    .source
                    .upgrade()
                    .is_some_and(|source| source.update_since(usize::MAX).status == Status::Evicted)
            {
                // 回收不等离屏内容重新进入视口；这里不做普通内容的解析或布局。
                entity.update(cx, |result, cx| result.sync(cx));
            }
        }
        let p = self.projection.borrow();
        self.entities.retain(|id, entity| {
            entity.read(cx).status != Status::Evicted
                || p.slots.iter().any(|slot| {
                    slot.id == *id && {
                        let top = p.visual(slot.start as f64) - p.top;
                        top < p.screen as f64 && top + slot.height > 0.0
                    }
                })
        });
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entities.len()
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn sync(&mut self, term: &mut Term<EventProxy>, screen: usize, cx: &gpui::App) {
        let floor = (term.grid().scrolled_out() + term.history_size()) as i64;
        let first = term.grid().scrolled_out() as i64;
        let enabled =
            !term.mode().intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE | TermMode::VI);
        let mut projection = self.projection.borrow_mut();
        if !enabled {
            projection.active = false;
            term.set_presentation_origin(None);
            return;
        }
        let source_layout_changed = self.epoch != Some(term.reading_epoch());
        if source_layout_changed {
            self.epoch = Some(term.reading_epoch());
            self.sources.clear();
            // 只在重排/新增锚点时重建稀疏索引，普通滚轮不扫描历史或重建 Markdown。
            for row in term.topmost_line().0..=term.bottommost_line().0 {
                if let Some(marker) = term.grid()[Point::new(Line(row), Column(0))].inline_marker()
                    && marker.row == 0
                {
                    self.sources.push((
                        Arc::downgrade(&marker.surface),
                        marker.surface.id,
                        floor + i64::from(row),
                    ));
                }
            }
        }
        let validated_history_end = self.validated_history_end;
        self.sources.retain(|(source, _, start)| {
            let Some(source) = source.upgrade() else {
                return false;
            };
            if source.has_grid_source() && source.update_since(usize::MAX).status == Status::Evicted
            {
                return false;
            }
            let line = *start - floor;
            *start >= first
                && line <= i64::from(term.bottommost_line().0)
                && term.grid()[Point::new(Line(line as i32), Column(0))]
                    .inline_marker()
                    .is_some_and(|marker| marker.surface.id == source.id)
                // 只跳过上次已验证的历史；刚滚入历史的行仍可能在本帧先被覆写。
                // 保存上轮边界，避免“覆写→滚屏”批量 VT 输入保留过期结果。
                && ((if source.has_grid_source() && !source_layout_changed {
                    (validated_history_end - *start).clamp(0, i64::from(source.rows())) as u16
                } else { 0 })..source.rows()).all(|row| {
                    let line = Line((line + i64::from(row)) as i32);
                    line <= term.bottommost_line()
                        && term.grid()[Point::new(line, Column(0))].inline_marker().is_some_and(
                            |marker| marker.surface.id == source.id && marker.row == row,
                        )
                        && if source.has_grid_source() {
                            // 已进历史的行只有 reflow/reset 才改变；滚轮只复核仍可被 VT 覆写的 live 尾。
                            !source_layout_changed && floor + i64::from(line.0) < validated_history_end
                                || source.source_row_matches(row, &term.grid()[line][..])
                        } else {
                            term.grid()[line][..].iter().skip(1).all(|cell| cell.is_empty())
                        }
                })
        });
        self.validated_history_end = floor;
        self.entities.retain(|id, _| self.sources.iter().any(|(_, source_id, _)| source_id == id));
        self.expanded_folds
            .retain(|id| self.sources.iter().any(|(_, source_id, _)| source_id == id));
        let anchor = projection.source(projection.top);
        let content_anchor = projection.slots.iter().find_map(|slot| {
            let start = projection.visual(slot.start as f64);
            (start <= projection.top && projection.top < start + slot.height)
                .then_some((slot.id, projection.top - start))
        });
        let follow = projection.slots.is_empty()
            || projection.visual(projection.bottom as f64)
                - projection.screen as f64
                - projection.top
                < 0.5;
        // resize 仅失效几何，只有真正的滚动命令才切换阅读锚点。
        let external_scroll = term.presentation_scroll_generation() != projection.scroll_generation;
        projection.slots = self
            .sources
            .iter()
            .filter_map(|(source, _, start)| {
                let source = source.upgrade()?;
                let rows = source.rows();
                if super::fold::is_fold(&source) {
                    if usize::from(rows) <= super::fold::KEEP * 2 {
                        return None;
                    }
                    let expanded = self.expanded_folds.contains(&source.id);
                    // 展开时在第10行后留一个控制行，原有网格行仍逐行绘制。
                    return Some(Slot {
                        id: source.id,
                        start: *start + super::fold::KEEP as i64 - i64::from(expanded),
                        rows: if expanded { 1 } else { rows - (super::fold::KEEP * 2) as u16 },
                        height: if expanded { 2.0 } else { 1.0 },
                        hide_source: !expanded,
                    });
                }
                let height = self
                    .entities
                    .get(&source.id)
                    .map_or(f64::from(rows), |entity| {
                        let result = entity.read(cx);
                        // 富文本不是整行网格；向上补齐会在结果尾部凭空留下接近一行的空白。
                        f64::from(result.measured_height.as_f32())
                            / f64::from(result.row_height.as_f32().max(1.0))
                    })
                    .max(if source.has_grid_source() { 1.0 } else { f64::from(rows) });
                Some(Slot { id: source.id, start: *start, rows, height, hide_source: true })
            })
            .collect();
        projection.floor = floor;
        projection.first = first;
        // 跟随实际内容末尾，不能把光标后的空白屏幕算成输出并将结果挤出视口。
        let last_content = (0..term.screen_lines())
            .rev()
            .find(|row| term.grid()[Line(*row as i32)][..].iter().any(|cell| !cell.is_empty()))
            .map_or(0, |row| row + 1);
        let live_end = last_content.max(term.grid().cursor.point.line.0.max(0) as usize + 1);
        projection.bottom = floor + live_end as i64;
        projection.screen = screen;
        projection.active = !projection.slots.is_empty();
        if !projection.active {
            term.set_presentation_origin(None);
            return;
        }
        projection.top = if let Some(anchor) = self.fold_anchor.take() {
            projection.visual(anchor)
        } else if external_scroll {
            if term.grid().display_offset() == 0 {
                projection.visual(projection.bottom as f64) - screen as f64
            } else {
                projection.visual((floor - term.grid().display_offset() as i64) as f64)
            }
        } else if follow {
            projection.visual(projection.bottom as f64) - screen as f64
        } else if let Some((id, offset)) = content_anchor
            && let Some(slot) = projection.slots.iter().find(|slot| slot.id == id)
        {
            // 保住内容内的实际偏移，不按整块高度比例缩放阅读位置。
            projection.visual(slot.start as f64) + offset.min((slot.height - 1.0).max(0.0))
        } else {
            projection.visual(anchor)
        };
        projection.publish(term);
    }

    pub fn placements(&self) -> Vec<(Arc<Surface>, f32, f32)> {
        let p = self.projection.borrow();
        if !p.active {
            return Vec::new();
        }
        self.sources
            .iter()
            .filter_map(|(source, _, _)| {
                let source = source.upgrade()?;
                let slot = p.slots.iter().find(|slot| slot.id == source.id)?;
                let fold = super::fold::is_fold(&source);
                let top = p.visual(slot.start as f64) - p.top
                    + if fold && !slot.hide_source { 1.0 } else { 0.0 };
                let height = if fold { 1.0 } else { slot.height as f32 };
                // 未测量的离屏结果继续使用估高，只在实际进入视口时创建和布局。
                (top < p.screen as f64 && top + f64::from(height) > 0.0)
                    .then(|| (source, top as f32, height))
            })
            .collect()
    }
    pub fn scroll_to(&self, offset: usize, term: &mut Term<EventProxy>) -> bool {
        let mut p = self.projection.borrow_mut();
        if !p.active {
            return false;
        }
        p.top = p.visual(p.bottom as f64) - p.screen as f64 - offset as f64;
        p.publish(term);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_fragments_follow_native_result_height_without_changing_texture_bounds() {
        use gpui::{point, px};
        use nebula_terminal::inline_image::{ImageLayout, ImageRun};
        for height in [1.0, 5.5] {
            let mut projection = Projection {
                slots: vec![Slot { id: 1, start: 1, rows: 3, height, hide_source: true }],
                active: true,
                ..Default::default()
            };
            if height == 1.0 {
                assert_eq!(projection.source_rows(4), 6);
            }
            let layout = ImageLayout {
                width: 50.0,
                height: 60.0,
                cell_width: 10.0,
                cell_height: 20.0,
                columns: 5,
                rows: 3,
            };
            let mut original = None;
            for source_row in 0..3usize {
                let run = ImageRun {
                    id: 1,
                    row: (4 + source_row) as u16,
                    column: 6,
                    columns: 2,
                    source_column: 2,
                    source_row,
                };
                let (image, clip) = super::super::super::inline_image::image_geometry(
                    layout,
                    &run,
                    point(px(5.0), px(7.0)),
                    px(10.0),
                    px(20.0),
                    &|row| px(20.0) * projection.row(row),
                );
                assert_eq!(image.origin.x, px(45.0));
                assert_eq!(clip.origin.y, px(7.0) + px(20.0) * projection.row(run.row as f32));
                assert_eq!(clip.size, gpui::size(px(20.0), px(20.0)));
                if let Some(original) = original {
                    assert_eq!(image, original);
                }
                original = Some(image);
            }
            let original = original.unwrap();
            projection.top += 1.0;
            let run =
                ImageRun { id: 1, row: 5, column: 6, columns: 2, source_column: 2, source_row: 1 };
            let (scrolled, _) = super::super::super::inline_image::image_geometry(
                layout,
                &run,
                point(px(5.0), px(7.0)),
                px(10.0),
                px(20.0),
                &|row| px(20.0) * projection.row(row),
            );
            assert_eq!(scrolled.origin.y, original.origin.y - px(20.0));
            assert_eq!(scrolled.size, original.size);
        }
    }

    #[test]
    fn native_result_projection_keeps_prompt_and_middle_of_document_addressable() {
        let p = Projection {
            slots: vec![Slot { id: 1, start: 2, rows: 1, height: 100.0, hide_source: true }],
            active: true,
            origin: 2,
            top: 52.0,
            ..Default::default()
        };
        assert_eq!(p.visual(3.0), 102.0);
        assert_eq!(p.row(1.0), 50.0);
        assert_eq!(p.source(102.0), 3.0);
        assert!((p.source_row(0.0) - 0.5).abs() < 0.0001);
        for offset in 0..100 {
            let source = p.source(2.0 + offset as f64);
            assert_eq!(source.floor(), 2.0);
            assert!((p.visual(source) - (2.0 + offset as f64)).abs() < 0.00001);
        }
    }

    #[test]
    fn native_result_projection_combines_multiple_results_without_moving_source_lines() {
        let p = Projection {
            slots: vec![
                Slot { id: 1, start: 1, rows: 1, height: 80.0, hide_source: true },
                Slot { id: 2, start: 5, rows: 2, height: 20.0, hide_source: true },
            ],
            ..Default::default()
        };
        for (source, visual) in
            [(0.0, 0.0), (1.0, 1.0), (2.0, 81.0), (5.0, 84.0), (7.0, 104.0), (8.0, 105.0)]
        {
            assert_eq!(p.visual(source), visual);
            assert_eq!(p.source(visual), source);
        }
    }

    #[test]
    fn native_result_height_keeps_fractional_rows_before_the_next_prompt() {
        let line_height = 17.5;
        let content_height = 123.25;
        let p = Projection {
            slots: vec![Slot {
                id: 1,
                start: 3,
                rows: 12,
                height: content_height / line_height,
                hide_source: true,
            }],
            ..Default::default()
        };
        assert!(((p.visual(15.0) - p.visual(3.0)) * line_height - content_height).abs() < 0.001);
        // shell 自己输出的空行仍然保留；只去除富文本占位额外补出来的整行空白。
        assert!(((p.visual(16.0) - p.visual(15.0)) * line_height - line_height).abs() < 0.001);
        for source in [3.0, 7.5, 15.0, 16.0] {
            assert!((p.source(p.visual(source)) - source).abs() < 0.001);
        }
    }
}
