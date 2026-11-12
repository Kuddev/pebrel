//! Mutable grid content and its transient resource ownership.

use super::*;
use crate::index::{Column, Line, Point};
use crate::inline_image::{ImageCell, ImageIdentity, ImageLayout, ImagePlacement, KittyPlacement};

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
    pub(crate) fn nebula_images_changed(&mut self) {
        self.reading.invalidate();
        self.mark_fully_damaged();
    }

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
        template.set_image(ImageCell { image: image.clone(), column: 0, row: 0, placement: None });
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
                    placement: None,
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
        placement: Arc<KittyPlacement>,
        keep_cursor: bool,
    ) {
        // Snapshots take an image-free fast path unless the grid knows it
        // carries images, so the flag has to follow the tiles in.
        self.grid.track_transient_content();
        let saved_point = self.grid.cursor.point;
        let saved_wrap = self.grid.cursor.input_needs_wrap;
        let start_column = saved_point.column.0;
        let start_line = saved_point.line;
        let screen_lines = self.screen_lines();
        for row in 0..placement.rows {
            if !keep_cursor && row != 0 {
                self.grid.cursor.point.column = Column(start_column);
                self.linefeed();
            }
            let line =
                if keep_cursor { start_line.0 + row as i32 } else { self.grid.cursor.point.line.0 };
            if line < 0 || line as usize >= screen_lines {
                break;
            }
            for column in 0..placement.columns {
                let target = start_column + column;
                if target >= self.columns() {
                    break;
                }
                self.grid.cursor.point = Point::new(Line(line), Column(target));
                self.write_at_cursor(' ');
                self.grid[Line(line)][Column(target)].set_image(ImageCell {
                    image: image.clone(),
                    column,
                    row,
                    placement: Some(placement.clone()),
                });
            }
        }
        if keep_cursor {
            self.grid.cursor.point = saved_point;
            self.grid.cursor.input_needs_wrap = saved_wrap;
        } else {
            let end = start_column + placement.columns;
            self.linefeed();
            self.grid.cursor.point.column = Column(end.min(self.columns() - 1));
            self.grid.cursor.input_needs_wrap = end >= self.columns();
        }
        self.mark_fully_damaged();
    }

    /// 删除所有可见 placement 时只扫描一次当前屏，不误删历史或另一屏的图。
    pub(crate) fn nebula_visible_kitty_placements(&self) -> std::collections::HashSet<u64> {
        let mut visible = std::collections::HashSet::new();
        for line in 0..self.screen_lines() {
            for column in 0..self.columns() {
                if let Some(placement) = self.grid[Line(line as i32)][Column(column)]
                    .image()
                    .and_then(|tile| tile.placement.as_ref())
                    .filter(|placement| placement.is_alive())
                {
                    visible.insert(placement.id);
                }
            }
        }
        visible
    }
}
