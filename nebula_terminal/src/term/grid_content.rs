//! Mutable grid content and its transient resource ownership.

use super::*;
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
}
