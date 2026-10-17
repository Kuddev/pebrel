//! Shape programming ligatures without giving the shaper control of the grid.
//!
//! Only contiguous ASCII graphic cells with identical styles share shaping.
//! Wide cells, combining sequences, whitespace and presentation boundaries keep
//! the existing per-cell path. Glyph cluster indices map back to fixed columns;
//! no `force_width` tolerance or natural advances accumulate along the row.
//!
//! Typographic pairs such as Monaco's `fi` and `fl` are the exception. Their
//! ligature is one glyph with a one-cell advance, while the next cluster index
//! skips a column. Snapping that index onto the grid leaves the second letter's
//! cell empty. Those clusters are shaped one character at a time instead.
//! Collapse is decided from the shaped column advance, not from the configured
//! grid width: `font.offset` and device-pixel rounding change the grid without
//! changing the font. Programming ligatures still advance every column they
//! cover, so they stay.

use std::sync::Arc;

use gpui::{
    FontId, LineLayout, Pixels, ShapedGlyph, ShapedLine, ShapedRun, SharedString, TextRun,
    WindowTextSystem, px,
};
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
                && cell.dim == first.dim
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
    let runs = [run];
    let mut shaped = text_system.shape_line(text, font_size, &runs, None);
    // No collapsed cluster: grid_layout makes the only glyph-run copy. The
    // TextRun stays in `runs` and is not cloned on this path.
    let layout = if let Some(expanded) =
        expand_collapsed_ligatures(&shaped, text_system, font_size, &runs[0])
    {
        grid_layout(&expanded, cell_width)
    } else {
        grid_layout(&shaped, cell_width)
    };
    // The cached natural layout is shared with other views. Replace only this
    // ShapedLine's Arc, preserving its text and decoration runs.
    *shaped = Arc::new(layout);
    shaped
}

/// A cluster collapsed when its advance is shorter than the columns it claims,
/// measured against the font's own column advance.
///
/// `0.45` leaves room for hinting. A programming ligature whose advance is about
/// `columns` natural columns stays joined. The configured grid width is not an
/// input: positive `font.offset.x` widens the grid and must not split `->`.
fn cluster_collapses(columns: usize, advance: f32, natural_advance: f32) -> bool {
    columns > 1 && natural_advance > 0.0 && advance < natural_advance * (columns as f32 - 0.45)
}

fn visit_clusters(natural: &LineLayout, mut visit: impl FnMut(usize, usize, f32)) {
    let mut glyphs = natural.runs.iter().flat_map(|run| &run.glyphs);
    let Some(first) = glyphs.next() else { return };
    let mut start = first.index;
    let mut origin = first.position.x.as_f32();
    for glyph in glyphs {
        if glyph.index == start {
            continue;
        }
        visit(start, glyph.index, glyph.position.x.as_f32() - origin);
        start = glyph.index;
        origin = glyph.position.x.as_f32();
    }
    visit(start, natural.len, natural.width.as_f32() - origin);
}

/// `(any multi-column cluster, advance of the first single-column cluster)`.
fn cluster_summary(natural: &LineLayout) -> (bool, Option<f32>) {
    let mut wide = false;
    let mut unit = None;
    visit_clusters(natural, |start, end, advance| {
        let columns = end.saturating_sub(start);
        if columns == 1 {
            if unit.is_none() && advance > 0.0 {
                unit = Some(advance);
            }
        } else if columns > 1 {
            wide = true;
        }
    });
    (wide, unit)
}

fn collapsed_cluster_ranges(natural: &LineLayout, natural_advance: f32) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    visit_clusters(natural, |start, end, advance| {
        if cluster_collapses(end.saturating_sub(start), advance, natural_advance) {
            ranges.push((start, end));
        }
    });
    ranges
}

fn expand_collapsed_ligatures(
    natural: &ShapedLine,
    text_system: &WindowTextSystem,
    font_size: Pixels,
    template: &TextRun,
) -> Option<LineLayout> {
    let (wide, mut unit) = cluster_summary(natural);
    if !wide {
        return None;
    }
    if unit.is_none() {
        unit = shaped_column_advance(text_system, font_size, template, &natural.text);
    }
    let ranges = collapsed_cluster_ranges(natural, unit?);
    if ranges.is_empty() {
        return None;
    }
    let text = natural.text.as_ref();

    let flat: Vec<(FontId, ShapedGlyph)> = natural
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().cloned().map(|glyph| (run.font_id, glyph)))
        .collect();
    let mut placed = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < flat.len() {
        let start = flat[index].1.index;
        if let Some(&(_, end)) = ranges.iter().find(|(range_start, _)| *range_start == start) {
            let end = end.min(text.len());
            for byte in start..end {
                let column = byte..byte + 1;
                if text.is_char_boundary(byte) && text.is_char_boundary(byte + 1) {
                    placed.extend(shape_one_column(
                        text_system,
                        font_size,
                        template,
                        &text[column],
                        byte,
                    ));
                }
            }
            while index < flat.len() && flat[index].1.index < end {
                index += 1;
            }
        } else {
            placed.push(flat[index].clone());
            index += 1;
        }
    }

    Some(LineLayout {
        font_size: natural.font_size,
        width: natural.width,
        ascent: natural.ascent,
        descent: natural.descent,
        runs: coalesce_runs(placed),
        len: natural.len,
    })
}

fn shaped_column_advance(
    text_system: &WindowTextSystem,
    font_size: Pixels,
    template: &TextRun,
    text: &str,
) -> Option<f32> {
    let end = text.chars().next()?.len_utf8();
    let sample = &text[..end];
    let mut run = template.clone();
    run.len = sample.len();
    let shaped = text_system.shape_line(sample.into(), font_size, &[run], None);
    let advance = shaped.width.as_f32();
    (advance > 0.0).then_some(advance)
}

fn shape_one_column(
    text_system: &WindowTextSystem,
    font_size: Pixels,
    template: &TextRun,
    text: &str,
    index: usize,
) -> Vec<(FontId, ShapedGlyph)> {
    let mut run = template.clone();
    run.len = text.len();
    let shaped = text_system.shape_line(text.into(), font_size, &[run], None);
    let mut glyphs = Vec::new();
    for run in &shaped.runs {
        let origin = run.glyphs.first().map(|glyph| glyph.position.x.as_f32()).unwrap_or(0.0);
        for glyph in &run.glyphs {
            let mut glyph = glyph.clone();
            glyph.position.x = px(glyph.position.x.as_f32() - origin);
            glyph.index = index;
            glyphs.push((run.font_id, glyph));
        }
    }
    glyphs
}

fn coalesce_runs(glyphs: Vec<(FontId, ShapedGlyph)>) -> Vec<ShapedRun> {
    let mut runs: Vec<ShapedRun> = Vec::new();
    for (font_id, glyph) in glyphs {
        if let Some(run) = runs.last_mut().filter(|run| run.font_id == font_id) {
            run.glyphs.push(glyph);
        } else {
            runs.push(ShapedRun { font_id, glyphs: vec![glyph] });
        }
    }
    runs
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
