use std::mem::size_of;

use super::{Cell, Column, Dimensions, Line, Term, TermMode};
use crate::grid::GridCell as _;

/// A compact, in-memory copy of terminal output for a replacement PTY.
/// Each row stores just the prefix through its last non-default cell.
pub struct Scrollback {
    columns: usize,
    rows: Vec<Vec<Cell>>,
    truncated: bool,
}

impl Scrollback {
    pub fn was_truncated(&self) -> bool {
        self.truncated
    }
}

impl<T> Term<T> {
    /// The start of the live prompt is the last row excluded from the snapshot.
    fn scrollback_cutoff(&self) -> Option<i32> {
        if self.reading.prompt_edited {
            return None;
        }
        let prompt = self.nebula_prompt_input_point()?;
        if self.mode.intersects(TermMode::ALT_SCREEN | TermMode::VI) {
            return None;
        }
        let input_cursor = &self.grid.cursor;
        let insertion_column = if input_cursor.input_needs_wrap {
            self.columns()
        } else {
            input_cursor.point.column.0
        };
        if input_cursor.point.line != prompt.line || insertion_column != prompt.column.0 {
            return None;
        }
        // Moving the cursor back to the start of an unsubmitted buffer does not
        // make it empty. Refuse any visible input to the right of the boundary.
        if self.grid[prompt.line][Column(prompt.column.0.min(self.columns()))..]
            .iter()
            .any(|cell| cell.c != ' ' || cell.zerowidth().is_some())
        {
            return None;
        }
        let top = self.grid.topmost_line().0;
        let last_prompt = self.reading.prompt_marks.back().copied()?;
        let before_prompt = i32::try_from(
            last_prompt as i64 - self.grid.scrolled_out() as i64 - self.grid.history_size() as i64,
        )
        .ok()?;
        if before_prompt > prompt.line.0 || before_prompt < top {
            return None;
        }
        Some(before_prompt)
    }

    pub fn at_unedited_prompt(&self) -> bool {
        self.scrollback_cutoff().is_some()
    }

    /// Capture the most recent output, excluding the live prompt and transient
    /// image/reader data. Exceeding the budget omits older rows.
    pub fn capture_scrollback(&self, max_bytes: usize) -> Option<Scrollback> {
        let before_prompt = self.scrollback_cutoff()?;
        let top = self.grid.topmost_line().0;
        let mut rows = Vec::new();
        let mut used = 0usize;
        let mut truncated = false;
        let blank = Cell::default();
        for line in (top..before_prompt).rev() {
            let row = &self.grid[Line(line)][..];
            let length = row.iter().rposition(|cell| *cell != blank).map_or(0, |i| i + 1);
            let mut bytes = size_of::<Vec<Cell>>() + length * size_of::<Cell>();
            for cell in &row[..length] {
                bytes = bytes.saturating_add(
                    cell.zerowidth().map_or(0, |chars| size_of::<char>() * chars.len()),
                );
                if let Some(link) = cell.hyperlink() {
                    bytes = bytes.saturating_add(link.id().len() + link.uri().len());
                }
            }
            if bytes > max_bytes.saturating_sub(used) {
                truncated = true;
                break;
            }
            used += bytes;
            let mut cells = row[..length].to_vec();
            for cell in &mut cells {
                cell.discard();
            }
            rows.push(cells);
        }
        rows.reverse();
        Some(Scrollback { columns: self.columns(), rows, truncated })
    }

    pub(in crate::term) fn install_scrollback(&mut self, scrollback: Scrollback) {
        self.grid.seed_scrollback(scrollback.columns, scrollback.rows);
        self.mark_fully_damaged();
    }
}

#[cfg(test)]
mod tests {
    use crate::event::VoidListener;
    use crate::index::Point;
    use crate::term::{Config, test::TermSize};
    use crate::vte::ansi::{Processor, StdSyncHandler};

    use super::*;

    #[test]
    fn snapshot_preserves_output_and_leaves_a_fresh_pty_screen() {
        let mut source = Term::new(Config::default(), &TermSize::new(28, 4), VoidListener);
        let mut parser = Processor::<StdSyncHandler>::default();
        parser.advance(&mut source, "an older line\r\n\x1b[31m中文 🙂\x1b[0m\r\n".as_bytes());
        source.nebula_add_prompt_mark();
        parser.advance(&mut source, b"shell> ");
        source.nebula_mark_prompt_input();
        let snapshot = source.capture_scrollback(1024 * 1024).unwrap();
        let mut resumed = Term::new(Config::default(), &TermSize::new(28, 5), VoidListener);
        resumed.seed_scrollback(snapshot);
        resumed.nebula_add_prompt_mark();
        parser.advance(&mut resumed, b"new shell> ");

        assert_eq!(resumed.history_size(), 2);
        assert_eq!(resumed.grid.cursor.point, Point::new(Line(0), Column(11)));
        let history = resumed
            .bounds_to_string(Point::new(Line(-2), Column(0)), Point::new(Line(-1), Column(27)));
        assert_eq!(history, "an older line\n中文 🙂");
        assert_ne!(resumed.grid[Line(-1)][Column(0)].fg, Cell::default().fg);
    }

    fn two_saved_lines() -> Scrollback {
        let mut source = Term::new(Config::default(), &TermSize::new(12, 3), VoidListener);
        let mut parser = Processor::<StdSyncHandler>::default();
        parser.advance(&mut source, b"old first\r\nold second\r\n");
        source.nebula_add_prompt_mark();
        source.nebula_mark_prompt_input();
        source.capture_scrollback(2048).unwrap()
    }

    #[test]
    fn conpty_startup_erase_restores_rows_once() {
        for split in 0..=8 {
            let mut term = Term::new(Config::default(), &TermSize::new(12, 3), VoidListener);
            term.pty_startup.scrollback = Some(two_saved_lines());
            let mut parser = Processor::<StdSyncHandler>::default();
            parser.advance(&mut term, b"\x1b[?1049h\x1b[3J\x1b[?1049l");
            let clears = b"\x1b[2J\x1b[3J";
            parser.advance(&mut term, &clears[..split]);
            parser.advance(&mut term, &clears[split..]);
            term.nebula_add_prompt_mark();
            assert_eq!(term.history_size(), 2);
            assert_eq!(
                term.bounds_to_string(
                    Point::new(Line(-2), Column(0)),
                    Point::new(Line(-1), Column(11))
                ),
                "old first\nold second"
            );
            parser.advance(&mut term, b"\x1b[3J");
            assert_eq!(term.history_size(), 0);
        }
    }

    #[test]
    fn first_prompt_prepends_only_rows_that_fit_before_startup_output() {
        let config = Config { scrolling_history: 2, ..Default::default() };
        let mut term = Term::new(config, &TermSize::new(12, 2), VoidListener);
        term.pty_startup.scrollback = Some(two_saved_lines());
        let mut parser = Processor::<StdSyncHandler>::default();
        parser.advance(&mut term, b"banner one\r\nbanner two\r\n");
        assert_eq!(term.history_size(), 1);
        term.nebula_add_prompt_mark();
        assert_eq!(term.history_size(), 2);
        assert_eq!(
            term.bounds_to_string(Point::new(Line(-2), Column(0)), Point::new(Line(0), Column(11))),
            "old second\nbanner one\nbanner two"
        );
        assert_eq!(term.grid.cursor.point, Point::new(Line(1), Column(0)));
        parser.advance(&mut term, b"\x1b[3J");
        assert_eq!(term.history_size(), 0);
    }

    #[test]
    fn empty_budget_and_unsubmitted_input_do_not_copy_terminal_state() {
        let mut source = Term::new(Config::default(), &TermSize::new(28, 4), VoidListener);
        let mut parser = Processor::<StdSyncHandler>::default();
        parser.advance(&mut source, b"visible history\r\n");
        source.nebula_add_prompt_mark();
        parser.advance(&mut source, b"shell> ");
        source.nebula_mark_prompt_input();
        source.note_prompt_input(false);
        // Windows line editors can keep an edited buffer while drawing a new prompt.
        source.nebula_add_prompt_mark();
        source.nebula_mark_prompt_input();
        assert!(source.capture_scrollback(1024 * 1024).is_none());
        source.note_prompt_input(true);
        let snapshot = source.capture_scrollback(0).unwrap();
        assert!(snapshot.rows.is_empty());
        assert!(snapshot.was_truncated());
        parser.advance(&mut source, b"draft\x1b[5D");
        assert!(source.capture_scrollback(1024 * 1024).is_none());
    }
}
