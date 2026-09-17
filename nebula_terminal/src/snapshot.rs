//! Bounded, presentation-independent physical-cell snapshots for remote mirrors.
//!
//! Text extraction intentionally joins wrapped lines and discards style. A mirror
//! must instead preserve each cell's width and both colors, including blank cells.

use crate::event::EventListener;
use crate::grid::Dimensions;
use crate::index::{Column, Line};
use crate::term::cell::Flags;
use crate::term::{Term, TermMode};
use crate::vte::ansi::Color;

const MAX_CELLS: usize = 40_000;
const MAX_TEXT_BYTES: usize = 128 * 1024;

/// RGB is nonnegative 0xRRGGBB; -(index+1) selects the terminal palette.
/// Palette indices 256/257/258 denote default foreground/background/cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScreenCell(pub String, pub u8, pub i32, pub i32, pub u8);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScreenSnapshot {
    pub version: u8,
    pub columns: usize,
    pub rows: Vec<Vec<ScreenCell>>,
    pub cursor: [i32; 3],
    /// Only OSC overrides; unspecified entries use the viewer's terminal theme.
    pub palette: Vec<(usize, u32)>,
}

/// Capture buffer-bottom rows without following a desktop user's scroll position.
/// Oversized grids fail explicitly instead of silently cutting a physical row.
pub fn capture<T: EventListener>(term: &Term<T>, requested_rows: usize) -> Option<ScreenSnapshot> {
    let columns = term.columns();
    let count = requested_rows.min(term.total_lines());
    if columns == 0
        || count == 0
        || columns > 400
        || count > 200
        || columns.checked_mul(count)? > MAX_CELLS
    {
        return None;
    }
    let start = term.screen_lines() as i32 - count as i32;
    let mut rows = Vec::with_capacity(count);
    let mut text_bytes = 0;
    for y in start..term.screen_lines() as i32 {
        let row = &term.grid()[Line(y)];
        let mut cells = Vec::with_capacity(columns);
        let mut x = 0;
        while x < columns {
            let cell = &row[Column(x)];
            let wide = cell.flags.contains(Flags::WIDE_CHAR) && x + 1 < columns;
            let width = if wide { 2 } else { 1 };
            let mut text = String::new();
            // A spacer can be left at the beginning/end of a wrapped wide glyph.
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                text.push(' ');
            } else {
                text.push(if cell.c.is_control() { ' ' } else { cell.c });
                if let Some(extra) = cell.zerowidth() {
                    text.extend(extra.iter().copied().filter(|c| !c.is_control()));
                }
            }
            text_bytes += text.len();
            if text_bytes > MAX_TEXT_BYTES || text.len() > 256 {
                return None;
            }
            let mut fg = color(cell.fg);
            let mut bg = color(cell.bg);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let flags = u8::from(cell.flags.contains(Flags::BOLD))
                | (u8::from(cell.flags.contains(Flags::ITALIC)) << 1)
                | (u8::from(cell.flags.intersects(Flags::ALL_UNDERLINES)) << 2)
                | (u8::from(cell.flags.contains(Flags::STRIKEOUT)) << 3)
                | (u8::from(cell.flags.contains(Flags::DIM)) << 4)
                | (u8::from(cell.flags.contains(Flags::HIDDEN)) << 5);
            cells.push(ScreenCell(text, width, fg, bg, flags));
            x += usize::from(width);
        }
        rows.push(cells);
    }
    let cursor = term.grid().cursor.point;
    let cursor_y = cursor.line.0 - start;
    let visible =
        term.mode().contains(TermMode::SHOW_CURSOR) && (0..count as i32).contains(&cursor_y);
    let palette = (0..crate::term::color::COUNT)
        .filter_map(|index| term.colors()[index].map(|rgb| (index, rgb_number(rgb))))
        .collect();
    Some(ScreenSnapshot {
        version: 1,
        columns,
        rows,
        cursor: [cursor.column.0 as i32, cursor_y, i32::from(visible)],
        palette,
    })
}

fn rgb_number(rgb: crate::vte::ansi::Rgb) -> u32 {
    (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
}

fn color(value: Color) -> i32 {
    match value {
        Color::Spec(rgb) => rgb_number(rgb) as i32,
        Color::Indexed(index) => -(i32::from(index) + 1),
        Color::Named(index) => -(index as i32 + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::VoidListener;
    use crate::term::test::TermSize;
    use crate::vte::ansi::Processor;

    fn terminal(columns: usize, rows: usize, bytes: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Default::default(), &TermSize::new(columns, rows), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        term
    }

    #[test]
    fn preserves_palette_truecolor_background_and_block_characters() {
        let term =
            terminal(12, 2, "\x1b[31m▐\x1b[38;2;217;119;87m▛\x1b[48;5;25m \x1b[0m".as_bytes());
        let frame = capture(&term, 2).unwrap();
        assert_eq!(frame.rows[0][0], ScreenCell("▐".into(), 1, -2, -258, 0));
        assert_eq!(frame.rows[0][1].2, 0xd97757);
        assert_eq!(frame.rows[0][2].3, -26);
        assert_eq!(frame.rows[0][2].0, " ");
    }

    #[test]
    fn keeps_wide_combining_and_wrapped_rows_in_physical_cells() {
        let term = terminal(6, 3, "中e\u{301}───xy".as_bytes());
        let frame = capture(&term, 3).unwrap();
        assert_eq!(frame.rows[0][0].1, 2);
        assert_eq!(frame.rows[0][1].0, "e\u{301}");
        assert_eq!(frame.rows[1][0].0, "x");
        for row in frame.rows {
            assert_eq!(row.iter().map(|c| usize::from(c.1)).sum::<usize>(), 6);
        }
    }

    #[test]
    fn rejects_unbounded_grids_and_preserves_cursor_and_inverse() {
        assert!(capture(&terminal(401, 2, b""), 2).is_none());
        assert!(capture(&terminal(400, 200, b""), 200).is_none());
        let term = terminal(8, 2, b"\x1b[7;1mA\x1b[?25l");
        let frame = capture(&term, 2).unwrap();
        assert_eq!(frame.rows[0][0].2, -258);
        assert_eq!(frame.rows[0][0].3, -257);
        assert_eq!(frame.rows[0][0].4, 1);
        assert_eq!(frame.cursor, [1, 0, 0]);
    }

    #[test]
    fn osc_overrides_and_sgr_reset_keep_distinct_color_roles() {
        let term = terminal(
            8,
            2,
            b"\x1b]4;1;rgb:12/34/56\x07\x1b]10;rgb:ab/cd/ef\x07\x1b[31;44mA\x1b[0mB",
        );
        let frame = capture(&term, 2).unwrap();
        assert!(frame.palette.contains(&(1, 0x123456)));
        assert!(frame.palette.contains(&(256, 0xabcdef)));
        assert_eq!(frame.rows[0][0], ScreenCell("A".into(), 1, -2, -5, 0));
        assert_eq!(frame.rows[0][1], ScreenCell("B".into(), 1, -257, -258, 0));
    }

    #[test]
    fn scrolling_the_desktop_does_not_move_the_phone_tail() {
        let text = (0..16).map(|n| format!("\x1b[32mline-{n}\r\n")).collect::<String>();
        let mut term = terminal(12, 4, text.as_bytes());
        let tail = capture(&term, 6).unwrap();
        term.scroll_display(crate::grid::Scroll::Top);
        assert_eq!(capture(&term, 6).unwrap(), tail);
        assert!(
            tail.rows.iter().any(|row| row
                .iter()
                .map(|c| c.0.as_str())
                .collect::<String>()
                .contains("line-15"))
        );
    }

    #[test]
    fn alternate_screen_and_resize_keep_physical_width_and_color() {
        let mut term = terminal(8, 4, b"\x1b[31mprimary");
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, "\x1b[?1049h\x1b[H\x1b[38;2;217;119;87m▐▛███▜▌".as_bytes());
        let alt = capture(&term, 4).unwrap();
        assert_eq!(alt.rows[0][0].0, "▐");
        assert_eq!(alt.rows[0][0].2, 0xd97757);
        term.resize(TermSize::new(12, 5));
        let resized = capture(&term, 5).unwrap();
        assert_eq!(resized.columns, 12);
        for row in &resized.rows {
            assert_eq!(row.iter().map(|c| usize::from(c.1)).sum::<usize>(), 12);
        }
        parser.advance(&mut term, b"\x1b[?1049l");
        let primary = capture(&term, 5).unwrap();
        assert!(
            primary.rows.iter().any(|row| row
                .iter()
                .map(|c| c.0.as_str())
                .collect::<String>()
                .contains("primary"))
        );
        assert_eq!(primary.rows[0][0].2, -2);
    }
}
