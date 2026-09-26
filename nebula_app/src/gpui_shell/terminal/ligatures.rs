//! Shape programming ligatures without giving the shaper control of the grid.
//!
//! Only contiguous ASCII graphic cells with identical styles share shaping.
//! Wide cells, combining sequences, whitespace and presentation boundaries keep
//! the existing per-cell path. Glyph cluster indices map back to fixed columns;
//! no `force_width` tolerance or natural advances accumulate along the row.

use std::sync::Arc;

use gpui::{LineLayout, Pixels, ShapedLine, SharedString, TextRun, WindowTextSystem};
use nebula_terminal::render::SnapCell;

fn ascii_graphic(cell: &SnapCell) -> bool {
    cell.text.len() == 1 && cell.text.as_bytes()[0].is_ascii_graphic()
}

/// The caller adds viewport, selection, cursor and overlay boundaries. Keeping
/// this scan separate from painting lets a singleton use the original cache.
pub(super) fn span_len(cells: &[SnapCell], allowed: impl Fn(&SnapCell, usize) -> bool) -> usize {
    let Some(first) = cells.first() else { return 0 };
    if !ascii_graphic(first) || !allowed(first, 0) {
        return 1;
    }
    cells
        .iter()
        .enumerate()
        .take_while(|(offset, cell)| {
            ascii_graphic(cell)
                && usize::from(cell.col) == usize::from(first.col) + offset
                && cell.fg == first.fg
                && cell.bg == first.bg
                && cell.bold == first.bold
                && cell.italic == first.italic
                && cell.underline == first.underline
                && cell.strikethrough == first.strikethrough
                && allowed(cell, *offset)
        })
        .count()
}

pub(super) fn shape_ascii_span(
    text_system: &WindowTextSystem,
    text: SharedString,
    font_size: Pixels,
    mut run: TextRun,
    cell_width: Pixels,
) -> ShapedLine {
    debug_assert!(text.bytes().all(|byte| byte.is_ascii_graphic()));
    run.len = text.len();
    let mut shaped = text_system.shape_line(text, font_size, &[run], None);
    // The cached natural layout is shared with other views. Replace only this
    // ShapedLine's Arc, preserving its text and decoration runs.
    *shaped = Arc::new(grid_layout(&shaped, cell_width));
    shaped
}

fn grid_layout(natural: &LineLayout, cell_width: Pixels) -> LineLayout {
    let mut runs = natural.runs.clone();
    let mut cluster = None;
    for glyph in runs.iter_mut().flat_map(|run| &mut run.glyphs) {
        let origin = match cluster {
            Some((index, origin)) if index == glyph.index => origin,
            _ => {
                cluster = Some((glyph.index, glyph.position.x));
                glyph.position.x
            },
        };
        // Multiple glyphs may share one source index. Preserve their internal
        // offsets, including zero-advance components; only the cluster moves.
        glyph.position.x = cell_width * glyph.index as f32 + (glyph.position.x - origin);
    }
    LineLayout {
        font_size: natural.font_size,
        width: cell_width * natural.len as f32,
        ascent: natural.ascent,
        descent: natural.descent,
        runs,
        len: natural.len,
    }
}

#[cfg(test)]
mod tests;
