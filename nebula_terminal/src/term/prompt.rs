//! Semantic prompt marks and input boundaries owned by the terminal grid.
use super::cell::Flags;
use super::{Term, TermMode};
use crate::event::EventListener;
use crate::grid::{Dimensions, Scroll};
use crate::index::{Column, Line, Point};

impl<T> Term<T> {
    /// The cursor row in the grid's absolute line numbering (see
    /// [`Grid::scrolled_out`]): stable across scrollback growth, so overlays
    /// (prompt marks, inline images) can anchor to it.
    pub fn nebula_cursor_abs_line(&self) -> usize {
        self.grid.scrolled_out()
            + self.grid.history_size()
            + self.grid.cursor.point.line.0.max(0) as usize
    }

    /// Record a shell prompt row (OSC 133;A) at the current cursor line.
    ///
    /// Called by the PTY reader between `parser.advance` slices, so the cursor
    /// sits exactly on the fresh prompt row. Marks only make sense on the
    /// primary screen — the alternate screen has no scrollback to jump.
    pub fn nebula_add_prompt_mark(&mut self) {
        if self.mode.contains(TermMode::ALT_SCREEN) {
            return;
        }
        let abs = self.nebula_cursor_abs_line();

        // A screen redraw (clear, resize) can re-emit a mark for the same or
        // an earlier row; drop those so the deque stays strictly increasing.
        while self.reading.prompt_marks.back().is_some_and(|&m| m >= abs) {
            self.reading.prompt_marks.pop_back();
        }
        // Prune marks whose rows have scrolled out of history entirely.
        let floor = self.grid.scrolled_out();
        while self.reading.prompt_marks.front().is_some_and(|&m| m < floor) {
            self.reading.prompt_marks.pop_front();
        }

        self.reading.prompt_marks.push_back(abs);
        self.reading.prompt_active = true;
        self.reading.prompt_input = None;
    }

    pub fn nebula_end_prompt(&mut self) {
        self.reading.prompt_active = false;
        self.reading.prompt_input = None;
    }

    pub fn nebula_prompt_active(&self) -> bool {
        self.reading.prompt_active && !self.mode.contains(TermMode::ALT_SCREEN)
    }

    /// Capture OSC 133;B between parser slices, before input is echoed.
    pub fn nebula_mark_prompt_input(&mut self) {
        if self.nebula_prompt_active() {
            let mut line = self.nebula_cursor_abs_line();
            let mut column = self.grid.cursor.point.column;
            if self.grid.cursor.input_needs_wrap {
                line += 1;
                column = Column(0);
            }
            self.reading.prompt_input = Some((line, column));
        }
    }

    /// Resolve the boundary after scrollback growth; reflow/reset discard it.
    /// A column equal to the grid width denotes an empty pending-wrap boundary.
    pub fn nebula_prompt_input_point(&self) -> Option<Point> {
        if !self.nebula_prompt_active() {
            return None;
        }
        let (line, column) = self.reading.prompt_input?;
        if line < self.grid.scrolled_out() || column.0 >= self.columns() {
            return None;
        }
        // Until input is echoed, a full-width prompt's next insertion point is
        // still on the filled row. This also covers the bottom row, where the
        // eventual wrap will scroll the grid before displaying the input.
        if self.grid.cursor.input_needs_wrap
            && line == self.nebula_cursor_abs_line() + 1
            && column == Column(0)
        {
            return Some(Point::new(self.grid.cursor.point.line, Column(self.columns())));
        }
        let relative =
            line as i64 - self.grid.scrolled_out() as i64 - self.grid.history_size() as i64;
        let point = Point::new(Line(i32::try_from(relative).ok()?), column);
        (point.line >= self.grid.topmost_line() && point.line <= self.grid.bottommost_line())
            .then_some(point)
    }

    /// Characters from the cursor to `target` inside the current prompt input,
    /// negative to the left, so a click can move the shell cursor with arrow
    /// keys. Targets past the typed text land after it; points before the
    /// input or outside the cursor's logical line yield `None`.
    pub fn nebula_prompt_cursor_offset(&self, target: Point) -> Option<isize> {
        let start = self.nebula_prompt_input_point()?;
        let cursor = self.grid.cursor.point;
        let end = self.line_search_right(cursor);
        if start.column.0 >= self.columns() || cursor < start || target < start || target > end {
            return None;
        }
        let next = |point: Point| {
            if point.column < self.last_column() {
                Point::new(point.line, point.column + 1)
            } else {
                Point::new(point.line + 1, Column(0))
            }
        };
        let mut text_end = start;
        let mut point = start;
        while point <= end {
            let cell = &self.grid[point];
            if cell.c != ' ' || cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                text_end = next(point);
            }
            point = next(point);
        }
        let mut target = target.min(text_end);
        if self.grid[target].flags.contains(Flags::WIDE_CHAR_SPACER) {
            target.column -= 1;
        }
        let (mut point, until) = (cursor.min(target), cursor.max(target));
        let mut characters = 0;
        while point < until {
            let flags = self.grid[point].flags;
            if !flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                characters += 1;
            }
            point = next(point);
        }
        Some(if target < cursor { -characters } else { characters })
    }

    /// Scroll the viewport to the previous (`up`) or next shell prompt mark.
    ///
    /// Returns whether the viewport moved, so callers know to redraw.
    pub fn nebula_prompt_jump(&mut self, up: bool) -> bool
    where
        T: EventListener,
    {
        if self.mode.contains(TermMode::ALT_SCREEN) || self.reading.prompt_marks.is_empty() {
            return false;
        }

        let scrolled_out = self.grid.scrolled_out();
        let history = self.grid.history_size();
        // Absolute line currently shown at the top of the viewport.
        let top_abs = scrolled_out + history - self.grid.display_offset();

        let target = if up {
            self.reading.prompt_marks.iter().rev().find(|&&m| m < top_abs)
        } else {
            self.reading.prompt_marks.iter().find(|&&m| m > top_abs)
        };
        let Some(&mark) = target else { return false };

        // Put the mark's row at the viewport top: offset = history - relative
        // row. Marks on the visible screen clamp to 0 (bottom), long-gone
        // marks clamp to the scrollback top.
        let offset = (scrolled_out + history).saturating_sub(mark).min(history);
        let delta = offset as i32 - self.grid.display_offset() as i32;
        if delta == 0 {
            return false;
        }
        self.scroll_display(Scroll::Delta(delta));
        true
    }
}
