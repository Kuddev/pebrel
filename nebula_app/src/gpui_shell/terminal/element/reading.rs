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
        for row in 0..layout.rows {
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
            window.paint_quad(fill(
                Bounds::new(
                    point(
                        bounds.origin.x + layout.cell_width * start as f32,
                        bounds.origin.y + layout.line_height * row as f32,
                    ),
                    size(layout.cell_width * (end - start) as f32, layout.line_height),
                ),
                color,
            ));
        }
    }
}
