use std::mem;

use super::{Dimensions as _, Grid, Row};
use crate::index::{Column, Line};
use crate::term::cell::Cell;

impl Grid<Cell> {
    /// Seed only history; the fresh PTY owns all live-screen coordinates and modes.
    pub(crate) fn seed_scrollback(&mut self, columns: usize, rows: Vec<Vec<Cell>>) {
        let keep = rows.len().min(self.max_scroll_limit);
        if keep == 0 || self.history_size() != 0 {
            return;
        }
        let lines = self.screen_lines();
        let visible_columns = self.columns();
        let mut history = Grid::new(lines, columns, self.max_scroll_limit);
        history.increase_scroll_limit(keep);
        let skip = rows.len() - keep;
        for (index, cells) in rows.into_iter().skip(skip).enumerate() {
            let mut row = Row::new(columns);
            for (column, cell) in cells.into_iter().enumerate() {
                row[Column(column)] = cell;
            }
            history.raw[Line(index as i32 - keep as i32)] = row;
        }
        history.resize(true, lines, visible_columns);
        self.increase_scroll_limit(history.history_size());
        for line in history.topmost_line().0..0 {
            let line = Line(line);
            mem::swap(&mut self.raw[line], &mut history.raw[line]);
        }
    }
}
