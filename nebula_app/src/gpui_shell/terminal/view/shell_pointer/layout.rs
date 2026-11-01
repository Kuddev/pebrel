//! A verified native buffer mapped onto actual terminal cells, never prompt text guesses.
use nebula_terminal::Term;
use nebula_terminal::event::EventListener;
use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Line, Point, Side};
use nebula_terminal::term::cell::{Cell, Flags};
use unicode_segmentation::UnicodeSegmentation;

/// Capture the cells the user actually clicked. Readline clears and redraws its
/// line around a bind-x reply, so the reply-time grid is not a stable reference.
pub(super) struct Screen {
    start: Point,
    cursor: Point,
    columns: usize,
    rows: usize,
    cells: Vec<Cell>,
}

impl Screen {
    pub(super) fn capture<T: EventListener>(term: &Term<T>) -> Option<Self> {
        let start = term.nebula_prompt_input_point()?;
        if start.line < Line(0) || term.grid().display_offset() != 0 {
            return None;
        }
        let columns = term.columns();
        let rows = term.screen_lines();
        if columns == 0 || start.line.0 as usize >= rows || start.column.0 > columns {
            return None;
        }
        // column == columns is the API's empty, pending-wrap insertion point.
        let count = (rows - start.line.0 as usize)
            .checked_mul(columns)?
            .checked_sub(start.column.0)?
            .min(4096);
        let cells = (0..count)
            .map(|offset| {
                let index = start.column.0 + offset;
                term.grid()
                    [Point::new(start.line + (index / columns) as i32, Column(index % columns))]
                .clone()
            })
            .collect();
        let mut cursor = term.grid().cursor.point;
        if term.grid().cursor.input_needs_wrap {
            cursor.column += 1;
        }
        Some(Self { start, cursor, columns, rows, cells })
    }

    fn cell(&self, point: Point) -> Option<&Cell> {
        let line = usize::try_from(point.line.0 - self.start.line.0).ok()?;
        let index = (line * self.columns + point.column.0).checked_sub(self.start.column.0)?;
        self.cells.get(index)
    }

    pub(super) fn matches<T: EventListener>(&self, term: &Term<T>, end: Point) -> bool {
        if term.nebula_prompt_input_point() != Some(self.start)
            || term.columns() != self.columns
            || term.screen_lines() != self.rows
            || term.grid().display_offset() != 0
        {
            return false;
        }
        let Some(count) = usize::try_from(end.line.0 - self.start.line.0)
            .ok()
            .and_then(|lines| lines.checked_mul(self.columns))
            .and_then(|cells| cells.checked_add(end.column.0))
            .and_then(|cells| cells.checked_sub(self.start.column.0))
        else {
            return false;
        };
        if count > self.cells.len() {
            return false;
        }
        let flags = Flags::WRAPLINE
            | Flags::WIDE_CHAR
            | Flags::WIDE_CHAR_SPACER
            | Flags::LEADING_WIDE_CHAR_SPACER
            | Flags::HIDDEN;
        self.cells.iter().take(count).enumerate().all(|(offset, old)| {
            let index = self.start.column.0 + offset;
            let current = &term.grid()[Point::new(
                self.start.line + (index / self.columns) as i32,
                Column(index % self.columns),
            )];
            old.c == current.c
                && old.zerowidth() == current.zerowidth()
                && old.flags & flags == current.flags & flags
        })
    }
}

struct Glyph {
    start: Point,
    end: Point,
    before: usize,
    after: usize,
}

pub(super) struct Layout {
    start: Point,
    pub(super) end: Point,
    length: usize,
    glyphs: Vec<Glyph>,
}

impl Layout {
    pub(super) fn read(screen: &Screen, text: &str, cursor: usize) -> Option<Self> {
        if text.len() > 4096 || text.chars().any(char::is_control) || !text.is_char_boundary(cursor)
        {
            return None;
        }
        let start = screen.start;
        let columns = screen.columns;
        let mut point = start;
        let mut count = 0;
        let mut caret = (cursor == 0).then_some(start);
        let mut glyphs = Vec::new();
        for (byte, grapheme) in text.grapheme_indices(true) {
            if point.column.0 == columns {
                if !screen
                    .cell(Point::new(point.line, Column(columns - 1)))?
                    .flags
                    .contains(Flags::WRAPLINE)
                {
                    return None;
                }
                point = Point::new(point.line + 1, Column(0));
            }
            if point.line >= Line(screen.rows as i32) {
                return None;
            }
            if screen.cell(point)?.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                if point.column.0 + 1 != columns
                    || !screen.cell(point)?.flags.contains(Flags::WRAPLINE)
                {
                    return None;
                }
                point = Point::new(point.line + 1, Column(0));
                if point.line >= Line(screen.rows as i32) {
                    return None;
                }
            }
            let cell = screen.cell(point)?;
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::HIDDEN) {
                return None;
            }
            if !std::iter::once(cell.c)
                .chain(cell.zerowidth().into_iter().flatten().copied())
                .eq(grapheme.chars())
            {
                return None;
            }
            let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
            if point.column.0 + width > columns
                || (width == 2
                    && !screen
                        .cell(Point::new(point.line, point.column + 1))?
                        .flags
                        .contains(Flags::WIDE_CHAR_SPACER))
            {
                return None;
            }
            if byte == cursor {
                caret = Some(point);
            }
            let end = Point::new(point.line, point.column + width);
            let after = count + grapheme.chars().count();
            glyphs.push(Glyph { start: point, end, before: count, after });
            count = after;
            point = end;
            if byte + grapheme.len() == cursor {
                caret = Some(point);
            }
        }
        let actual = screen.cursor;
        let caret = caret?;
        // A cursor explicitly placed on the next row is equivalent to a verified
        // soft-wrap boundary. No other visual discrepancy is accepted.
        if caret != actual
            && !(caret.column.0 == columns
                && actual == Point::new(caret.line + 1, Column(0))
                && screen
                    .cell(Point::new(caret.line, Column(columns - 1)))?
                    .flags
                    .contains(Flags::WRAPLINE))
        {
            return None;
        }
        Some(Self { start, end: point, length: count, glyphs })
    }

    pub(super) fn offset(&self, point: Point, side: Side) -> Option<usize> {
        if point < self.start || point > self.end {
            return None;
        }
        if point == self.end {
            return Some(self.length);
        }
        let glyph = self.glyphs.iter().find(|glyph| {
            point.line == glyph.start.line
                && point.column >= glyph.start.column
                && point.column < glyph.end.column
        })?;
        let half = point.column.0 * 2 + usize::from(side == Side::Right);
        Some(if half < glyph.start.column.0 + glyph.end.column.0 {
            glyph.before
        } else {
            glyph.after
        })
    }

    pub(super) fn position(&self, offset: usize) -> Option<(Point, Side)> {
        if let Some(glyph) = self.glyphs.iter().find(|glyph| glyph.before == offset) {
            return Some((glyph.start, Side::Left));
        }
        if offset == self.length {
            let glyph = self.glyphs.last()?;
            return Some((Point::new(glyph.end.line, glyph.end.column - 1), Side::Right));
        }
        None
    }
}
