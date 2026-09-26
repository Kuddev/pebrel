//! 清屏与滚动历史清除的网格、选区及阅读位置合同。
use super::{Boundary, Column, Dimensions, EventListener, Line, Term, TermMode, ansi, cmp};

impl<T: EventListener> Term<T> {
    pub(super) fn clear_screen_contents(&mut self, mode: ansi::ClearMode) {
        self.observe_redraw_clear(&mode);
        log::trace!("Clearing screen: {mode:?}");
        let bg = self.grid.cursor.template.bg;
        let screen_lines = self.screen_lines();

        match mode {
            ansi::ClearMode::Above => {
                let cursor = self.grid.cursor.point;
                if cursor.line > 1 {
                    self.grid.reset_region(..cursor.line);
                }
                let end = cmp::min(cursor.column + 1, Column(self.columns()));
                for cell in &mut self.grid[cursor.line][..end] {
                    *cell = bg.into();
                }
                let range = Line(0)..=cursor.line;
                self.selection = self.selection.take().filter(|s| !s.intersects_range(range));
            },
            ansi::ClearMode::Below => {
                let cursor = self.grid.cursor.point;
                for cell in &mut self.grid[cursor.line][cursor.column..] {
                    *cell = bg.into();
                }
                if (cursor.line.0 as usize) < screen_lines - 1 {
                    self.grid.reset_region((cursor.line + 1)..);
                }
                let range = cursor.line..Line(screen_lines as i32);
                self.selection = self.selection.take().filter(|s| !s.intersects_range(range));
            },
            ansi::ClearMode::All => {
                if self.mode.contains(TermMode::ALT_SCREEN) {
                    self.grid.reset_region(..);
                } else {
                    let old_offset = self.grid.display_offset();
                    self.grid.clear_viewport();
                    let lines = self.grid.display_offset().saturating_sub(old_offset);
                    self.vi_mode_cursor.point.line =
                        (self.vi_mode_cursor.point.line - lines).grid_clamp(self, Boundary::Grid);
                }
                self.selection = None;
            },
            ansi::ClearMode::Saved if self.history_size() > 0 => {
                self.grid.clear_history();
                self.vi_mode_cursor.point.line =
                    self.vi_mode_cursor.point.line.grid_clamp(self, Boundary::Cursor);
                self.selection = self.selection.take().filter(|s| !s.intersects_range(..Line(0)));
            },
            ansi::ClearMode::Saved => (),
        }
        self.mark_fully_damaged();
    }
}
