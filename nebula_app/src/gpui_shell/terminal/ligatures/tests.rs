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
