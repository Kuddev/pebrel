//! 搜索直接标出原始网格范围，避免与公式视觉投影混用列号。
use super::*;
use gpui_component::Colorize as _;

impl TerminalElement {
    pub(super) fn paint_reading_highlight(
        &self,
        bounds: Bounds<Pixels>,
        layout: &TermLayout,
        viewport_top: i64,
        window: &mut Window,
        cx: &App,
    ) {
        let view = self.view.read(cx);
        let Some(span) = view.reading.panel.as_ref().and_then(|p| p.highlight) else {
            return;
        };
        let Some(session) = &view.session else {
            return;
        };
        if session.term.lock().reading_epoch() != span.epoch {
            return;
        }
        let color = cx.theme().primary.opacity(0.30);
        let projection = view.native_results.projection.borrow();
        // 折叠后的可见尾行可能来自屏幕范围以外；隐藏原文也不能把高亮压到控制行上。
        for row in 0..projection.source_rows(layout.rows) {
            if projection.covers(row) {
                continue;
            }
            let absolute = viewport_top + row as i64;
            if absolute < 0 {
                continue;
            }
            let line = absolute as usize;
            if line < span.start.line || line > span.end.line {
                continue;
            }
            let start =
                if line == span.start.line { span.start.column } else { 0 }.min(layout.cols);
            let end =
                if line == span.end.line { span.end.column } else { layout.cols }.min(layout.cols);
            if start >= end {
                continue;
            }
            let highlight = Bounds::new(
                point(
                    bounds.origin.x + layout.cell_width * start as f32,
                    bounds.origin.y + layout.line_height * projection.row(row as f32),
                ),
                size(layout.cell_width * (end - start) as f32, layout.line_height),
            );
            #[cfg(all(test, feature = "gpui-test-support"))]
            snapshot_probe::record_reading_highlight(self.view.entity_id(), highlight, cx);
            window.paint_quad(fill(highlight, color));
        }
    }
}
