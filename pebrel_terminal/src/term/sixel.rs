//! Terminal-side graphics modes and pixel-to-grid placement.

use super::*;
use crate::event::WindowSize;
use crate::inline_image::{ImageCell, ImageIdentity, ImageLayout, ImagePlacement, MAX_IMAGE_CELLS};
use crate::sixel::SixelImage;

impl<T: EventListener> Term<T> {
    pub(super) fn report_device_attributes(&mut self, intermediate: Option<char>) {
        match intermediate {
            None => {
                // The pre-primed ConPTY bring-up query was already answered;
                // a second answer would reach the shell as typed input.
                if self.bringup_da1_pending {
                    self.bringup_da1_pending = false;
                    trace!("Suppressing duplicate DA1 response (pre-primed ConPTY handshake)");
                    return;
                }
                trace!("Reporting primary device attributes");
                self.event_proxy.send_event(Event::PtyWrite("\x1b[?62;4c".into()));
            },
            Some('>') => {
                trace!("Reporting secondary device attributes");
                let version = version_number(env!("CARGO_PKG_VERSION"));
                self.event_proxy.send_event(Event::PtyWrite(format!("\x1b[>0;{version};1c")));
            },
            _ => debug!("Unsupported device attributes intermediate"),
        }
    }

    pub(super) fn set_graphics_mode(&mut self, mode: PrivateMode, enabled: bool) -> bool {
        let flag = match mode {
            PrivateMode::Unknown(80) => TermMode::SIXEL_DISPLAY,
            PrivateMode::Unknown(8452) => TermMode::SIXEL_CURSOR_RIGHT,
            // Images have private palettes. Report this as permanently set.
            PrivateMode::Unknown(1070) => return true,
            _ => return false,
        };
        self.mode.set(flag, enabled);
        true
    }

    pub(super) fn report_graphics_mode(&self, mode: PrivateMode) -> bool {
        let status = match mode {
            PrivateMode::Unknown(80) => {
                if self.mode.contains(TermMode::SIXEL_DISPLAY) {
                    1
                } else {
                    2
                }
            },
            PrivateMode::Unknown(8452) => {
                if self.mode.contains(TermMode::SIXEL_CURSOR_RIGHT) {
                    1
                } else {
                    2
                }
            },
            PrivateMode::Unknown(1070) => 3,
            _ => return false,
        };
        self.event_proxy.send_event(Event::PtyWrite(format!("\x1b[?{};{status}$y", mode.raw())));
        true
    }

    pub(super) fn insert_sixel(
        &mut self,
        image: &SixelImage,
        viewport: WindowSize,
    ) -> Option<ImagePlacement> {
        let saved = self.grid.cursor.clone();
        let fixed = self.mode.contains(TermMode::SIXEL_DISPLAY);
        let start_column =
            if fixed { 0 } else { saved.point.column.0 + usize::from(saved.input_needs_wrap) };
        let cell_width = f32::from(viewport.cell_width.max(1));
        let cell_height = f32::from(viewport.cell_height.max(1));
        let width = image.width as f32;
        let height = image.height as f32 * image.aspect.0 as f32 / image.aspect.1 as f32;
        let columns = (width / cell_width).ceil() as usize;
        let rows = (height / cell_height).ceil() as usize;
        let visible_columns = columns.min(self.columns().saturating_sub(start_column));
        if visible_columns == 0 || rows == 0 || visible_columns.checked_mul(rows)? > MAX_IMAGE_CELLS
        {
            return None;
        }
        let layout = ImageLayout { width, height, cell_width, cell_height, columns, rows };
        self.reset_input_cluster();
        self.grid.track_transient_content();
        let identity = ImageIdentity::new();
        if fixed {
            self.grid.cursor.point = Point::new(Line(0), Column(0));
        }
        for row in 0..rows {
            if fixed && row >= self.screen_lines() {
                break;
            }
            if row != 0 {
                if fixed {
                    self.grid.cursor.point.line += 1;
                } else {
                    let next = self.grid.cursor.point.line + 1;
                    if next >= self.screen_lines() && next != self.scroll_region.end {
                        // Outside the scroll region, LF cannot advance beyond
                        // the screen. Clip instead of overwriting the last tile.
                        break;
                    }
                    self.linefeed();
                }
            }
            for column in 0..visible_columns {
                self.grid.cursor.point.column = Column(start_column + column);
                if !image.transparent {
                    self.write_at_cursor(' ');
                }
                self.grid.cursor_cell().set_image(ImageCell {
                    image: identity.clone(),
                    column,
                    row,
                    placement: None,
                });
            }
        }
        if fixed {
            self.grid.cursor = saved;
        } else {
            // Graphics finish on their last occupied text row. A producer's
            // following LF advances past the image; saved cursors stay valid.
            let column = if self.mode.contains(TermMode::SIXEL_CURSOR_RIGHT) {
                if start_column + columns >= self.columns() {
                    self.linefeed();
                    0
                } else {
                    start_column + columns
                }
            } else {
                start_column
            };
            self.grid.cursor.point.column = Column(column);
            self.grid.cursor.input_needs_wrap = false;
        }
        self.mark_fully_damaged();
        Some(ImagePlacement { id: identity.id, lifetime: Arc::downgrade(&identity), layout })
    }
}
