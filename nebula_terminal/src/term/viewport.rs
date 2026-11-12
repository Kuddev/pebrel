//! Viewport coordinates are presentation state, separate from VT grid mutation.
use super::{Term, TermMode, viewport_to_point_from};
use crate::grid::Dimensions as _;
use crate::index::{Line, Point};
use std::cmp;

impl<T> Term<T> {
    /// First grid row shown at line zero of a `lines`-tall visual viewport.
    ///
    /// While an interactive resize is between commit points the visual
    /// viewport can be smaller than the grid. The crop previews exactly the
    /// rows the deferred shrink will keep visible — the same anchor
    /// [`Grid::resize`]/[`Grid::resize_conpty`] will use — so the settled
    /// reflow lands without a visual jump. A user scrolled into history stays
    /// anchored to the row they are reading instead.
    #[inline]
    pub fn viewport_origin_for(&self, lines: usize) -> Line {
        if let Some(origin) = self.presentation_origin
            && !self.mode.intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE | TermMode::VI)
        {
            return origin.max(self.topmost_line()).min(self.bottommost_line());
        }
        let display_offset = self.grid.display_offset();
        if display_offset != 0 || lines >= self.grid.screen_lines() {
            return Line(-(display_offset as i32));
        }

        // The alternate screen is repainted by its application after the
        // commit, and its shrink only ever follows the cursor.
        let conpty = self.config.conpty_resize && !self.mode.contains(TermMode::ALT_SCREEN);
        Line(self.grid.shrink_scroll_amount(conpty, lines) as i32)
    }

    pub fn presentation_origin(&self) -> Option<Line> {
        self.presentation_origin
    }

    pub fn presentation_scroll_generation(&self) -> u64 {
        self.presentation_scroll_generation
    }

    pub fn set_presentation_origin(&mut self, origin: Option<Line>) {
        self.presentation_origin = origin;
    }

    /// Convert a point in a `lines`-tall visual viewport to grid space.
    ///
    /// The result is clamped to cells that exist in the grid: during a
    /// deferred resize the visual viewport can extend past the committed
    /// grid, and input must not index rows or columns that are not there yet.
    #[inline]
    pub fn visual_viewport_to_point(&self, lines: usize, point: Point<usize>) -> Point {
        let mut point = viewport_to_point_from(self.viewport_origin_for(lines), point);
        point.line = cmp::min(point.line, self.bottommost_line());
        point.column = cmp::min(point.column, self.grid.last_column());
        point
    }
}
