//! Read-only terminal viewport assembled for the renderer.

use crate::grid::{Dimensions, GridIterator};
use crate::index::{Boundary, Line, Point};
use crate::selection::SelectionRange;
use crate::term::cell::{Cell, Flags};
use crate::term::color::Colors;
use crate::term::{Term, TermMode};
use crate::vte::ansi::CursorShape;

/// Terminal cursor rendering information.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct RenderableCursor {
    pub shape: CursorShape,
    pub point: Point,
}

impl RenderableCursor {
    fn new<T>(term: &Term<T>) -> Self {
        let vi_mode = term.mode().contains(TermMode::VI);
        let mut point = if vi_mode { term.vi_mode_cursor.point } else { term.grid.cursor.point };
        // Bound stale resize coordinates before indexing, preserving vi cursors
        // in retained history. This only adjusts the read-only render snapshot.
        let boundary = if vi_mode { Boundary::Grid } else { Boundary::Cursor };
        point = point.grid_clamp(&term.grid, boundary);
        if point.column > 0 && term.grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
            point.column -= 1;
        }

        let shape = if !vi_mode && !term.mode().contains(TermMode::SHOW_CURSOR) {
            CursorShape::Hidden
        } else {
            term.cursor_style().shape
        };

        Self { shape, point }
    }
}

/// All content required to render the current terminal view.
pub struct RenderableContent<'a> {
    pub display_iter: GridIterator<'a, Cell>,
    pub selection: Option<SelectionRange>,
    pub cursor: RenderableCursor,
    pub display_offset: usize,
    /// First grid row rendered at viewport line zero.
    pub viewport_origin: Line,
    pub colors: &'a Colors,
    pub mode: TermMode,
}

impl<'a> RenderableContent<'a> {
    pub(super) fn new<T>(term: &'a Term<T>) -> Self {
        Self::with_viewport(term, term.grid().screen_lines(), term.grid().columns())
    }

    pub(super) fn with_viewport<T>(term: &'a Term<T>, lines: usize, columns: usize) -> Self {
        let grid = term.grid();
        let display_offset = grid.display_offset();
        // Crop the committed grid to the rows the deferred resize will keep,
        // so rendering during the drag and the settled reflow agree.
        let viewport_origin = term.viewport_origin_for(lines);

        Self {
            display_iter: grid.display_iter_from(viewport_origin, lines, columns),
            display_offset,
            viewport_origin,
            cursor: RenderableCursor::new(term),
            selection: term.selection.as_ref().and_then(|selection| selection.to_range(term)),
            colors: &term.colors,
            mode: *term.mode(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::VoidListener;
    use crate::grid::Scroll;
    use crate::index::Column;
    use crate::term::Config;
    use crate::term::test::TermSize;

    #[test]
    fn stale_cursor_coordinates_are_bounded_before_rendering() {
        let mut term = Term::new(Config::default(), &TermSize::new(116, 29), VoidListener);
        term.resize(TermSize::new(80, 28));

        // Model a cursor snapshot still using coordinates discarded by resize.
        for (stale, expected) in [
            (Point::new(Line(-40), Column(0)), Point::new(Line(0), Column(0))),
            (Point::new(Line(-1), Column(0)), Point::new(Line(0), Column(0))),
            (Point::new(Line(28), Column(79)), Point::new(Line(27), Column(79))),
            (Point::new(Line(27), Column(115)), Point::new(Line(27), Column(79))),
        ] {
            for vi_mode in [false, true] {
                term.mode.set(TermMode::VI, vi_mode);
                term.grid.cursor.point = if vi_mode { Point::default() } else { stale };
                term.vi_mode_cursor.point = stale;

                let content = term.renderable_content();
                assert_eq!(content.cursor.point, expected, "{stale:?}, vi={vi_mode}");
                assert_eq!(content.cursor.shape, term.cursor_style().shape);
                assert_eq!(content.display_iter.count(), 80 * 28);

                // The smaller visual viewport used during a deferred resize must
                // also remain iterable, without moving the cursor into its crop.
                let content = term.renderable_content_with_viewport(27, 79);
                assert_eq!(content.cursor.point, expected, "{stale:?}, vi={vi_mode}");
                assert_eq!(content.display_iter.count(), 79 * 27);
                assert_eq!(term.vi_mode_cursor.point, stale);
                assert_eq!(
                    term.grid.cursor.point,
                    if vi_mode { Point::default() } else { stale },
                    "rendering must not mutate terminal state"
                );
            }
        }
    }

    #[test]
    fn deferred_viewport_bounds_stale_cursor_before_calculating_scroll() {
        for conpty_resize in [false, true] {
            let config = Config { conpty_resize, ..Config::default() };
            let mut term = Term::new(config, &TermSize::new(4, 3), VoidListener);
            term.grid[Point::new(Line(2), Column(0))].c = 'x';

            for (line, expected_origin) in [
                (-40, Line(0)),
                (-1, Line(0)),
                (0, Line(0)),
                (1, Line(i32::from(conpty_resize))),
                (2, Line(1)),
                (3, Line(1)),
                (i32::MAX, Line(1)),
            ] {
                term.grid.cursor.point = Point::new(Line(line), Column(0));
                assert_eq!(term.viewport_origin_for(2), expected_origin);
                let content = term.renderable_content_with_viewport(2, 4);
                assert_eq!(content.viewport_origin, expected_origin);
                assert_eq!(content.display_iter.count(), 8);
                assert_eq!(term.grid.cursor.point.line, Line(line));
            }
        }
    }

    #[test]
    fn vi_cursor_preserves_valid_history_and_bounds_discarded_history() {
        let mut term = Term::new(Config::default(), &TermSize::new(5, 3), VoidListener);
        term.grid.scroll_up(&(Line(0)..Line(3)), 3);
        term.scroll_display(Scroll::Top);
        term.mode.insert(TermMode::VI);
        let oldest = term.grid.topmost_line();
        assert!(oldest < Line(0));

        for point in [Point::new(oldest, Column(0)), Point::new(Line(-1), Column(4))] {
            term.vi_mode_cursor.point = point;
            assert_eq!(term.renderable_content().cursor.point, point);
        }

        term.vi_mode_cursor.point = Point::new(oldest - 1, Column(2));
        assert_eq!(term.renderable_content().cursor.point, Point::new(oldest, Column(0)));

        // A normal terminal cursor belongs to the live screen, even with history.
        term.mode.remove(TermMode::VI);
        term.grid.cursor.point = Point::new(Line(-1), Column(2));
        assert_eq!(term.renderable_content().cursor.point, Point::default());
    }

    #[test]
    fn wide_character_cursor_uses_the_leading_cell_after_bounding() {
        let mut term = Term::new(Config::default(), &TermSize::new(4, 2), VoidListener);
        let leading = Point::new(Line(1), Column(2));
        let spacer = Point::new(Line(1), Column(3));
        term.grid[leading].c = '界';
        term.grid[leading].flags.insert(Flags::WIDE_CHAR);
        term.grid[spacer].flags.insert(Flags::WIDE_CHAR_SPACER);

        for point in [spacer, Point::new(Line(1), Column(8))] {
            for vi_mode in [false, true] {
                term.mode.set(TermMode::VI, vi_mode);
                term.grid.cursor.point = point;
                term.vi_mode_cursor.point = point;
                assert_eq!(term.renderable_content().cursor.point, leading);
            }
        }
    }

    #[test]
    fn orphan_wide_character_spacer_at_column_zero_does_not_underflow() {
        let mut term = Term::new(Config::default(), &TermSize::new(1, 1), VoidListener);
        term.grid[Point::default()].flags.insert(Flags::WIDE_CHAR_SPACER);
        assert_eq!(term.renderable_content().cursor.point, Point::default());
    }

    #[test]
    fn bounding_preserves_hidden_cursor_and_vi_cursor_visibility() {
        let mut term = Term::new(Config::default(), &TermSize::new(4, 2), VoidListener);
        term.mode.remove(TermMode::SHOW_CURSOR);
        term.grid.cursor.point = Point::new(Line(3), Column(8));
        assert_eq!(term.renderable_content().cursor.shape, CursorShape::Hidden);

        term.mode.insert(TermMode::VI);
        term.vi_mode_cursor.point = Point::new(Line(-40), Column(0));
        assert_eq!(term.renderable_content().cursor.shape, term.cursor_style().shape);
    }
}
