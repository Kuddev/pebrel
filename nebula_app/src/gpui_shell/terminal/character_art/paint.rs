//! 图案压缩后仍可见的相邻源行，复用快照颜色与选区而不缓存旧字形。
use super::Frame;
use crate::gpui_shell::terminal::{colors::Palette, cursor_painter::paint_box_glyph};
use gpui::{App, Bounds, Font, Pixels, Point, Rgba, TextAlign, TextRun, Window, fill, point, size};
use nebula_terminal::{
    render::{RenderSnapshot, boxdraw},
    term::color::Colors,
};

pub(in crate::gpui_shell::terminal) fn paint_overflow(
    frame: &Frame,
    source: &RenderSnapshot,
    offset: i64,
    visible_rows: u16,
    origin: Point<Pixels>,
    metrics: (Pixels, Pixels),
    theme: &Palette,
    overrides: &Colors,
    selection: Rgba,
    selection_foreground: Option<Rgba>,
    font_size: Pixels,
    font: &impl Fn(bool, bool, bool) -> Font,
    window: &mut Window,
    cx: &mut App,
) {
    let (width, height) = metrics;
    let outside = |row: u16| {
        let row = i64::from(row) + offset;
        row < 0 || row >= i64::from(visible_rows)
    };
    let rect = |col: usize, count: usize, y: f32, h: f32| {
        Bounds::new(
            point(origin.x + width * col as f32, origin.y + height * y),
            size(width * count as f32, height * h),
        )
    };
    for run in source.selection_runs.iter().filter(|run| outside(run.row)) {
        let row = i64::from(run.row) + offset;
        frame.spans(row, usize::from(run.start)..usize::from(run.end), row as f32, |cols, y, h| {
            if h < 1.0 {
                window.paint_quad(fill(rect(cols.start, cols.len(), y, h), selection));
            }
        });
    }
    let foreground = |row, col, fg, bold, dim| {
        selection_foreground
            .filter(|_| {
                source.selection_runs.iter().any(|r| r.row == row && r.start <= col && col < r.end)
            })
            .unwrap_or_else(|| theme.resolve_styled(fg, overrides, bold, dim))
    };
    for glyph in source.box_glyphs.iter().filter(|glyph| outside(glyph.row)) {
        let row = i64::from(glyph.row) + offset;
        let (y, h) = frame.cell_at(row, usize::from(glyph.col), row as f32);
        if h >= 1.0 {
            continue;
        }
        let Some(primitives) = boxdraw::primitives(
            glyph.ch,
            width.as_f32(),
            height.as_f32() * h,
            window.scale_factor(),
        ) else {
            continue;
        };
        let snap = crate::gpui_shell::terminal::element::snap_to_device;
        paint_box_glyph(
            window,
            &primitives,
            point(
                snap(origin.x + width * f32::from(glyph.col), window.scale_factor()),
                snap(origin.y + height * y, window.scale_factor()),
            ),
            foreground(glyph.row, glyph.col, glyph.fg, glyph.bold, glyph.dim),
        );
    }
    for segment in source.segments.iter().filter(|segment| outside(segment.row)) {
        let row = i64::from(segment.row) + offset;
        for cell in &segment.cells {
            let (y, h) = frame.cell_at(row, usize::from(cell.col), row as f32);
            if h >= 1.0 {
                continue;
            }
            let run = TextRun {
                len: cell.text.len(),
                font: font(cell.bold, cell.italic, segment.wide),
                color: foreground(segment.row, cell.col, cell.fg, cell.bold, cell.dim).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line =
                window.text_system().shape_line(cell.text.clone().into(), font_size, &[run], None);
            let _ = line.paint(
                point(origin.x + width * f32::from(cell.col), origin.y + height * y),
                height * h,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
}
