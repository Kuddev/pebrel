//! Legacy shell image decoding and pane cache ownership.

use super::{ActionContext, EventProxy, Notifier, input, warn};
use nebula_terminal::inline_image::{ImageData, ImagePlacement};

impl input::Processor<EventProxy, ActionContext<'_, Notifier, EventProxy>> {
    pub(super) fn handle_inline_image(&mut self, data: ImageData, placement: ImagePlacement) {
        // Decode off the PTY thread (here, on the UI loop) and
        // anchor the pixels to the pane. Textures upload lazily
        // on first draw.
        match crate::renderer::image::decode_inline_image(&data, &self.ctx.display.colors) {
            Ok((px_w, px_h, rgba)) => {
                let images = &mut self.ctx.nebula_state.inline_images;
                images.push(crate::display::NebulaInlineImage {
                    placement,
                    rgba: std::sync::Arc::new(rgba),
                    px_w,
                    px_h,
                });
                // VRAM/heap guard against imgcat runaway loops.
                if images.len() > 16 {
                    images.remove(0);
                }
                *self.ctx.dirty = true;
            },
            Err(err) => {
                warn!("inline image decode failed: {err}");
            },
        }
    }
}
