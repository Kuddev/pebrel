//! Mutable grid content and its transient resource ownership.

use super::*;
use crate::index::{Column, Line, Point};
use crate::inline_image::{ImageCell, ImageIdentity, ImageLayout, ImagePlacement};

impl<T> Term<T> {
    /// Mutable access to the raw grid data structure.
    pub fn grid_mut(&mut self) -> &mut Grid<Cell> {
        self.reset_input_cluster();
        self.reading.invalidate();
        self.grid.track_transient_content()
    }

    /// Internal marker writes need cached-row cleanup, but cannot introduce images.
    pub(crate) fn inline_content_grid_mut(&mut self) -> &mut Grid<Cell> {
        self.reset_input_cluster();
        self.reading.invalidate();
        self.grid.track_inline_content()
    }
}

impl<T: EventListener> Term<T> {
    /// Place the image exactly like rows of terminal cells. OSC 1337 advances
    /// between image rows only; a trailing newline belongs to the application.
    pub fn nebula_insert_inline_image(&mut self, layout: ImageLayout) -> ImagePlacement {
        self.reset_input_cluster();
        if self.grid.cursor.input_needs_wrap {
            self.wrapline();
        }
        self.grid.track_transient_content();
        let image = ImageIdentity::new();
        // Prepare text attributes once; tiles clone this image template so a
        // styled image does not allocate a separate text owner for every cell.
        let mut template = self.grid.cursor.template.clone();
        template.set_image(ImageCell { image: image.clone(), column: 0, row: 0 });
        let start_column = self.grid.cursor.point.column.0;
        let end_column = (start_column + layout.columns).min(self.columns());
        for row in 0..layout.rows {
            if row != 0 {
                self.linefeed();
            }
            for column in start_column..end_column {
                self.grid.cursor.point.column = Column(column);
                self.write_at_cursor(' ');
                self.grid.cursor_cell().extra = template.extra.clone();
                self.grid.cursor_cell().set_image(ImageCell {
                    image: image.clone(),
                    column: column - start_column,
                    row,
                });
            }
        }
        self.grid.cursor.point.column = Column(end_column.min(self.columns() - 1));
        self.grid.cursor.input_needs_wrap = end_column == self.columns();
        self.mark_fully_damaged();
        ImagePlacement { id: image.id, lifetime: Arc::downgrade(&image), layout }
    }

    /// Write one tile of an externally placed image at the cursor without
    /// moving it. The kitty protocol addresses cells directly (`C=1` means
    /// "leave the cursor alone"), so text cursor state must survive the write.
    pub(crate) fn nebula_insert_kitty_block(
        &mut self,
        image: &Arc<ImageIdentity>,
        tile_column: usize,
        tile_row: usize,
        columns: usize,
        rows: usize,
    ) {
        // Snapshots take an image-free fast path unless the grid knows it
        // carries images, so the flag has to follow the tiles in.
        self.grid.track_transient_content();
        let saved_point = self.grid.cursor.point;
        let saved_wrap = self.grid.cursor.input_needs_wrap;
        let start_column = saved_point.column.0;
        let start_line = saved_point.line;
        let screen_lines = self.screen_lines();
        for row in 0..rows {
            let line = start_line.0 + row as i32;
            if line < 0 || line as usize >= screen_lines {
                break;
            }
            for column in 0..columns {
                let target = start_column + column;
                if target >= self.columns() {
                    break;
                }
                self.grid.cursor.point = Point::new(Line(line), Column(target));
                self.write_at_cursor(' ');
                self.grid[Line(line)][Column(target)].set_image(ImageCell {
                    image: image.clone(),
                    column: tile_column + column,
                    row: tile_row + row,
                });
            }
        }
        self.grid.cursor.point = saved_point;
        self.grid.cursor.input_needs_wrap = saved_wrap;
        self.mark_fully_damaged();
    }

    /// Blank every cell carrying `id`. Protocol clients erase placement by
    /// placement, so the whole screen is scanned once per image instead of once
    /// per erased cell.
    pub(crate) fn nebula_clear_image_cells(&mut self, id: u64) {
        let saved_point = self.grid.cursor.point;
        let saved_wrap = self.grid.cursor.input_needs_wrap;
        let mut cleared = false;
        for line in 0..self.screen_lines() {
            for column in 0..self.columns() {
                let carries_image = self.grid[Line(line as i32)][Column(column)]
                    .image()
                    .is_some_and(|tile| tile.image.id == id);
                if !carries_image {
                    continue;
                }
                self.grid.cursor.point = Point::new(Line(line as i32), Column(column));
                self.write_at_cursor(' ');
                cleared = true;
            }
        }
        self.grid.cursor.point = saved_point;
        self.grid.cursor.input_needs_wrap = saved_wrap;
        if cleared {
            self.mark_fully_damaged();
        }
    }
}
