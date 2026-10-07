//! 终端模型的只读投影：纯文本尾部与有界彩色网格共用目标和生命周期。
use super::{ApiError, MAX_READ_BYTES, RuntimeTaskState};
use nebula_terminal::event::EventListener;
use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Line, Point};
use nebula_terminal::term::{Term, cell::Flags};
use nebula_terminal::vte::ansi::{Color, CursorShape, Rgb};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimePaneRead {
    pub window_id: u64,
    pub pane_id: u64,
    pub text: String,
    pub requested_lines: usize,
    pub returned_lines: usize,
    pub history_available: usize,
    pub truncated: bool,
    pub task_state: RuntimeTaskState,
    pub exited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<RuntimeTerminalScreen>,
}

/// 单一模式避免把“不读取 screen”与“读取 viewport”组合成矛盾状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMode {
    Live,
    Viewport,
}

/// Screen v1 uses logical cells, not ANSI replay; wide spacers never become extra glyphs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeTerminalScreen {
    pub version: u8,
    pub columns: usize,
    pub rows: Vec<Vec<(String, u8, i32, i32, u8)>>,
    pub palette: Vec<(usize, u32)>,
    pub cursor: (usize, usize, u8),
    #[serde(default)]
    pub wrapped: Vec<bool>,
}

pub(crate) fn capture_terminal_screen<T: EventListener>(
    term: &Term<T>,
    palette_color: impl Fn(usize) -> Rgb,
) -> Result<RuntimeTerminalScreen, ApiError> {
    capture_terminal_grid(term, palette_color, 0)
}

/// 手机显式跟随被控制的桌面视口；普通 Agent 读取仍固定在实时网格。
pub(crate) fn capture_terminal_viewport<T: EventListener>(
    term: &Term<T>,
    palette_color: impl Fn(usize) -> Rgb,
) -> Result<RuntimeTerminalScreen, ApiError> {
    capture_terminal_grid(term, palette_color, term.grid().display_offset())
}

fn capture_terminal_grid<T: EventListener>(
    term: &Term<T>,
    palette_color: impl Fn(usize) -> Rgb,
    display_offset: usize,
) -> Result<RuntimeTerminalScreen, ApiError> {
    let columns = term.columns();
    let lines = term.screen_lines();
    if !(1..=400).contains(&columns) || !(1..=200).contains(&lines) || columns * lines > 40_000 {
        return Err(ApiError::new(
            "screen_too_large",
            "screen v1 requires at most 400 columns, 200 rows and 40000 cells; resize the desktop pane",
        ));
    }
    let mut rows = Vec::with_capacity(lines);
    let mut text_bytes = 0;
    for y in 0..lines {
        let mut row = Vec::with_capacity(columns);
        let mut x = 0;
        while x < columns {
            let cell = &term.grid()[Point::new(Line(y as i32 - display_offset as i32), Column(x))];
            let spacer =
                cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let width = if !spacer && cell.flags.contains(Flags::WIDE_CHAR) && x + 1 < columns {
                2
            } else {
                1
            };
            let mut glyph =
                if spacer || cell.c.is_control() { " ".to_owned() } else { cell.c.to_string() };
            if !spacer && let Some(extra) = cell.zerowidth() {
                glyph.extend(extra.iter().copied().filter(|ch| !ch.is_control()));
            }
            text_bytes += glyph.len();
            if glyph.len() > 256 || text_bytes > 128 * 1024 {
                return Err(ApiError::new(
                    "screen_too_large",
                    "screen glyphs exceed the screen v1 text budget",
                ));
            }
            let mut foreground = wire_color(cell.fg, cell.flags.contains(Flags::BOLD));
            let mut background = wire_color(cell.bg, false);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            let flags = u8::from(cell.flags.contains(Flags::BOLD))
                | (u8::from(cell.flags.contains(Flags::ITALIC)) << 1)
                | (u8::from(cell.flags.intersects(Flags::ALL_UNDERLINES)) << 2)
                | (u8::from(cell.flags.contains(Flags::STRIKEOUT)) << 3)
                | (u8::from(cell.flags.contains(Flags::DIM)) << 4)
                | (u8::from(cell.flags.contains(Flags::HIDDEN)) << 5);
            row.push((glyph, width as u8, foreground, background, flags));
            x += width;
        }
        rows.push(row);
    }
    let cursor = term.renderable_content().cursor;
    let cursor_y = i64::from(cursor.point.line.0) + display_offset as i64;
    let visible = cursor.shape != CursorShape::Hidden && cursor_y >= 0 && cursor_y < lines as i64;
    Ok(RuntimeTerminalScreen {
        version: 1,
        columns,
        rows,
        wrapped: (0..lines)
            .map(|y| {
                term.grid()[Point::new(Line(y as i32 - display_offset as i32), Column(columns - 1))]
                    .flags
                    .contains(Flags::WRAPLINE)
            })
            .collect(),
        palette: (0..nebula_terminal::term::color::COUNT)
            .map(|index| (index, packed_rgb(palette_color(index))))
            .collect(),
        cursor: (
            cursor.point.column.0.min(columns - 1),
            cursor_y.clamp(0, lines as i64 - 1) as usize,
            u8::from(visible),
        ),
    })
}

fn packed_rgb(color: Rgb) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

fn wire_color(color: Color, bold: bool) -> i32 {
    let index = match color {
        Color::Spec(rgb) => return packed_rgb(rgb) as i32,
        Color::Named(named) => named as usize,
        Color::Indexed(index) => index as usize,
    };
    let index = if bold && index < 8 { index + 8 } else { index };
    -(index as i32 + 1)
}

/// Read the logical tail of the terminal model. The range is anchored at the
/// buffer bottom, never at `display_offset`, so a user scrolling through
/// history cannot change what an external agent observes.
pub(crate) fn capture_terminal_tail<T: EventListener>(
    term: &Term<T>,
    window_id: u64,
    pane_id: u64,
    requested_lines: usize,
    task_state: RuntimeTaskState,
    exited: bool,
    exit_reason: Option<String>,
) -> RuntimePaneRead {
    let columns = term.columns();
    let screen_lines = term.screen_lines();
    let total_lines = term.total_lines();
    let history_available = total_lines.saturating_sub(screen_lines);
    if columns == 0 || screen_lines == 0 || total_lines == 0 {
        return RuntimePaneRead {
            window_id,
            pane_id,
            text: String::new(),
            requested_lines,
            returned_lines: 0,
            history_available,
            truncated: false,
            task_state,
            exited,
            exit_reason,
            screen: None,
        };
    }

    let mut returned_lines = requested_lines.min(total_lines);
    let end = Point::new(Line(screen_lines as i32 - 1), Column(columns - 1));
    let capture = |lines: usize| {
        let start_line = screen_lines as i64 - lines as i64;
        let start = Point::new(Line(start_line.max(-(history_available as i64)) as i32), Column(0));
        term.bounds_to_string(start, end)
    };
    let mut text = capture(returned_lines);

    // Reduce by whole terminal rows first, preserving exact returned_lines.
    // Only a pathological single row can fall through to UTF-8 byte slicing.
    while text.len() > MAX_READ_BYTES && returned_lines > 1 {
        let estimated = ((returned_lines as u128 * MAX_READ_BYTES as u128) / text.len() as u128)
            .clamp(1, (returned_lines - 1) as u128) as usize;
        returned_lines = estimated;
        text = capture(returned_lines);
    }
    let mut byte_truncated = false;
    if text.len() > MAX_READ_BYTES {
        let mut start = text.len() - MAX_READ_BYTES;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text = text[start..].to_owned();
        byte_truncated = true;
    }

    RuntimePaneRead {
        window_id,
        pane_id,
        text,
        requested_lines,
        returned_lines,
        history_available,
        truncated: returned_lines < total_lines || byte_truncated,
        task_state,
        exited,
        exit_reason,
        screen: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_terminal::event::VoidListener;
    use nebula_terminal::grid::Scroll;
    use nebula_terminal::term::test::TermSize;
    use nebula_terminal::vte::ansi::{NamedColor, Processor};

    fn terminal(columns: usize, lines: usize, text: &str) -> Term<VoidListener> {
        let mut term = Term::new(Default::default(), &TermSize::new(columns, lines), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, text.as_bytes());
        term
    }

    fn capture(term: &Term<VoidListener>) -> RuntimeTerminalScreen {
        capture_terminal_screen(term, |index| {
            term.colors()[index].unwrap_or(Rgb { r: 1, g: 2, b: 3 })
        })
        .unwrap()
    }

    #[test]
    fn screen_preserves_wide_combining_ansi_truecolor_and_styles() {
        let term =
            terminal(12, 2, "\x1b[1;3;4;9;31m中e\u{301}\x1b[0;38;2;12;34;56mX\x1b[7mY\x1b[0m\t");
        let screen = capture(&term);
        let row = &screen.rows[0];
        assert_eq!(row[0], ("中".into(), 2, -10, -258, 15));
        assert_eq!(row[1].0, "e\u{301}");
        assert_eq!(row[2].2, 0x0c2238);
        assert_eq!((row[3].2, row[3].3), (-258, 0x0c2238));
        assert_eq!(row.iter().map(|cell| usize::from(cell.1)).sum::<usize>(), 12);
        assert!(row.iter().all(|cell| cell.0.chars().all(|ch| !ch.is_control())));
        assert_eq!(screen.palette.len(), 269);
    }

    #[test]
    fn screen_preserves_palette_overrides_and_cursor_visibility_without_reading_scrollback() {
        let mut term = terminal(12, 2, "old\r\nsecond\r\nlatest\x1b]10;#123456\x07");
        let screen = capture(&term);
        assert_eq!(screen.palette[NamedColor::Foreground as usize].1, 0x123456);
        assert_eq!(screen.cursor, (6, 1, 1));
        term.scroll_display(Scroll::Top);
        assert_eq!(capture(&term), screen);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, b"\x1b[?25l");
        assert_eq!(capture(&term).cursor.2, 0);
    }

    #[test]
    fn controlled_viewport_reads_history_without_changing_live_grid_or_tail() {
        let mut term = terminal(12, 2, "old\r\nsecond\r\nlatest");
        let live = capture(&term);
        let tail = capture_terminal_tail(&term, 1, 2, 3, RuntimeTaskState::Idle, false, None);
        term.scroll_display(Scroll::Top);
        let viewport = capture_terminal_viewport(&term, |_| Rgb { r: 0, g: 0, b: 0 }).unwrap();
        assert_eq!(viewport.rows[0][0].0, "o");
        assert_eq!(viewport.rows[1][0].0, "s");
        assert_eq!(viewport.cursor.2, 0);
        assert!(viewport.cursor.1 < viewport.rows.len());
        assert_eq!(capture(&term), live);
        assert_eq!(
            capture_terminal_tail(&term, 1, 2, 3, RuntimeTaskState::Idle, false, None),
            tail
        );
    }

    #[test]
    fn scroll_request_bounds_and_viewport_opt_in_are_explicit() {
        use crate::runtime_api::{ApiRequest, RuntimeCommand};
        for lines in [-32, -1, 1, 32] {
            let request = ApiRequest::new(
                "fixture".into(),
                "pane.scroll",
                serde_json::json!({"window_id":1,"pane_id":2,"lines":lines,"column":79,"row":23}),
            );
            assert!(matches!(
                RuntimeCommand::from_request(&request).unwrap(),
                RuntimeCommand::ScrollPane { .. }
            ));
        }
        for params in [
            serde_json::json!({"pane_id":2,"lines":0,"column":0,"row":0}),
            serde_json::json!({"pane_id":2,"lines":33,"column":0,"row":0}),
            serde_json::json!({"pane_id":2,"lines":1,"column":400,"row":0}),
            serde_json::json!({"pane_id":2,"lines":1,"column":0,"row":200}),
        ] {
            assert!(
                RuntimeCommand::from_request(&ApiRequest::new(
                    "fixture".into(),
                    "pane.scroll",
                    params
                ))
                .is_err()
            );
        }
        let invalid = ApiRequest::new(
            "fixture".into(),
            "pane.read",
            serde_json::json!({"pane_id":2,"screen_viewport":true}),
        );
        assert!(RuntimeCommand::from_request(&invalid).is_err());
    }

    #[test]
    fn screen_limits_are_explicit_and_plain_text_remains_compatible() {
        let term = terminal(401, 2, "latest");
        assert_eq!(
            capture_terminal_screen(&term, |_| Rgb { r: 0, g: 0, b: 0 }).unwrap_err().code,
            "screen_too_large"
        );
        let tail = capture_terminal_tail(&term, 1, 2, 2, RuntimeTaskState::Idle, false, None);
        let json = serde_json::to_value(tail).unwrap();
        assert!(json.get("screen").is_none());
        assert!(json["text"].as_str().unwrap().contains("latest"));
    }

    #[test]
    fn screen_request_is_opt_in_and_advertised() {
        use crate::runtime_api::{ApiRequest, RuntimeCommand};
        for screen in [true, false] {
            let request = ApiRequest::new(
                "fixture".into(),
                "pane.read",
                serde_json::json!({"pane_id": 2, "screen":screen}),
            );
            assert!(
                matches!(RuntimeCommand::from_request(&request).unwrap(), RuntimeCommand::ReadPane { screen: value, .. } if value.is_some() == screen)
            );
        }
        assert!(
            super::super::transport::runtime_description()["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| feature == "pane.read.screen.v1")
        );
    }

    #[test]
    fn screen_reports_physical_soft_wrap_without_turning_hard_breaks_into_continuations() {
        let term = terminal(6, 3, "abcdefgh\r\nnext");
        let screen = capture(&term);
        assert_eq!(screen.wrapped, vec![true, false, false]);
    }
}
