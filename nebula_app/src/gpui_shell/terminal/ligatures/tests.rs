use super::*;
use gpui::{FontId, GlyphId, ShapedGlyph, ShapedRun, point, px};
use nebula_terminal::vte::ansi::{Color, NamedColor};

fn cells(text: &str) -> Vec<SnapCell> {
    text.chars()
        .enumerate()
        .map(|(col, character)| SnapCell {
            col: col as u16,
            text: character.to_string(),
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            bold: false,
            dim: false,
            bg_dim: false,
            italic: false,
            underline: false,
            strikethrough: false,
        })
        .collect()
}

#[test]
fn shaping_stops_at_cell_styles_gaps_and_non_ascii_text() {
    assert_eq!(span_len(&cells("a->b"), |_, _| true), 4);
    for text in ["a b", "a中b", "a🙂b"] {
        assert_eq!(span_len(&cells(text), |_, _| true), 1);
    }
    for change in [
        |cell: &mut SnapCell| cell.bold = true,
        |cell: &mut SnapCell| cell.dim = true,
        |cell: &mut SnapCell| cell.italic = true,
        |cell: &mut SnapCell| cell.underline = true,
        |cell: &mut SnapCell| cell.strikethrough = true,
        |cell: &mut SnapCell| cell.fg = Color::Indexed(1),
        |cell: &mut SnapCell| cell.bg = Color::Indexed(2),
        |cell: &mut SnapCell| cell.col += 1,
        |cell: &mut SnapCell| cell.text.push('\u{301}'),
    ] {
        let mut cells = cells("a->b");
        change(&mut cells[2]);
        assert_eq!(span_len(&cells, |_, _| true), 2);
    }
    // The presentation adapter supplies the first cursor, selection or formula
    // boundary. Text after that boundary cannot join the preceding ligature.
    assert_eq!(span_len(&cells("a->b"), |_, offset| offset < 2), 2);
}

fn layout(indices_and_positions: &[(usize, f32)], len: usize) -> LineLayout {
    LineLayout {
        font_size: px(15.0),
        width: px(len as f32 * 9.6),
        ascent: px(12.0),
        descent: px(3.0),
        runs: vec![ShapedRun {
            font_id: FontId(0),
            glyphs: indices_and_positions
                .iter()
                .map(|&(index, x)| ShapedGlyph {
                    id: GlyphId(index as u32),
                    position: point(px(x), px(0.0)),
                    index,
                    is_emoji: false,
                })
                .collect(),
        }],
        len,
    }
}

fn positions(line: &LineLayout) -> Vec<f32> {
    line.runs.iter().flat_map(|run| &run.glyphs).map(|g| g.position.x.as_f32()).collect()
}

#[test]
fn ligature_components_move_together_and_the_next_cell_keeps_its_column() {
    // A two-cell ligature has two components at byte 1, then the next glyph
    // belongs to byte 3. Neither the glyph count nor natural advances are columns.
    let natural = layout(&[(0, 0.0), (1, 9.6), (1, 13.6), (3, 28.8)], 4);
    let aligned = grid_layout(&natural, px(9.0));
    assert_eq!(positions(&aligned), [0.0, 9.0, 13.0, 27.0]);
    assert_eq!(aligned.width, px(36.0));
    assert_eq!(positions(&natural), [0.0, 9.6, 13.6, 28.8]);
}

#[test]
fn monaco_fi_ligature_collapses_to_one_cell_and_must_be_expanded() {
    // "file": the fi glyph sits on byte 0 and only advances one cell, while
    // "l" is already indexed at byte 2. Snapping that index leaves byte 1 empty.
    let natural = layout(&[(0, 0.0), (2, 10.0), (3, 20.0)], 4);
    assert_eq!(super::collapsed_cluster_ranges(&natural, 10.0), vec![(0, 2)]);
    assert!(super::cluster_collapses(2, 10.0, 10.0));
}

#[test]
fn programming_ligature_uses_natural_advance_not_grid_spacing() {
    // Natural column advance 10, two-column cluster advance 20. font.offset.x = 3
    // at scale 1 makes the grid 13, and 20 < 13 * 1.55. That grid width must not
    // be the collapse metric. Negative spacing (grid 7) must not be either.
    assert!(!super::cluster_collapses(2, 20.0, 10.0));
    assert!(20.0 < 13.0 * 1.55);
    let natural = layout(&[(0, 0.0), (1, 10.0), (1, 14.0), (3, 30.0)], 4);
    assert!(super::collapsed_cluster_ranges(&natural, 10.0).is_empty());
    for grid in [7.0, 13.0] {
        let aligned = grid_layout(&natural, px(grid));
        let indices: Vec<_> = aligned.runs[0].glyphs.iter().map(|glyph| glyph.index).collect();
        assert_eq!(indices, [0, 1, 1, 3]);
        assert_eq!(positions(&aligned)[3], 3.0 * grid);
    }
}

#[test]
fn a_programming_ligature_that_covers_both_columns_stays_joined() {
    let natural = layout(&[(0, 0.0), (1, 9.6), (1, 13.6), (3, 28.8)], 4);
    assert!(super::collapsed_cluster_ranges(&natural, 9.6).is_empty());
    assert!(!super::cluster_collapses(2, 19.2, 9.6));
}

#[test]
fn a_trailing_one_cell_ligature_is_collapsed_against_the_shaped_width() {
    let mut natural = layout(&[(0, 0.0)], 2);
    natural.width = px(10.0);
    assert_eq!(super::collapsed_cluster_ranges(&natural, 10.0), vec![(0, 2)]);
}

#[test]
fn deleting_text_or_splitting_a_run_cannot_change_other_grid_origins() {
    let before = layout(&[(0, 0.0), (1, 9.6), (2, 19.2), (3, 28.8)], 4);
    let after = layout(&[(0, 0.0), (1, 9.6), (2, 19.2)], 3);
    for width in [8.0, 9.0, 13.0 / 1.5, 14.0 / 1.5] {
        let before = grid_layout(&before, px(width));
        let after = grid_layout(&after, px(width));
        assert_eq!(&positions(&before)[..3], positions(&after));
        for (col, position) in positions(&after).iter().enumerate() {
            assert_eq!(*position, col as f32 * width);
        }
        let suffix = grid_layout(&layout(&[(0, 0.0), (1, 9.6)], 2), px(width));
        assert_eq!(positions(&suffix)[0] + 2.0 * width, positions(&before)[2]);
    }
}
