use std::hash::{BuildHasher, RandomState};
use std::sync::{Arc, Weak};

use super::*;
use crate::event::WindowSize;
use crate::inline_image::ImageOptions;

#[derive(Default)]
pub(super) struct PendingImages {
    entries: Vec<Weak<PendingImage>>,
}

pub(crate) struct PendingImage {
    marker: String,
    data: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    options: ImageOptions,
    viewport: WindowSize,
}

impl<T: EventListener> Term<T> {
    /// VTE has no extension-OSC callback. A private, one-shot title token
    /// places the image at the parser's actual replay point, including inside
    /// DEC synchronized updates. It is consumed before any title side effect.
    pub(crate) fn nebula_queue_inline_image(
        &mut self,
        data: Vec<u8>,
        width: u32,
        height: u32,
        options: ImageOptions,
        viewport: WindowSize,
    ) -> Option<(String, Arc<PendingImage>)> {
        let pending = &mut self.pending_images;
        pending.entries.retain(|image| image.strong_count() != 0);
        let bytes: usize =
            pending.entries.iter().filter_map(Weak::upgrade).map(|image| image.data.len()).sum();
        if pending.entries.len() >= 16 || bytes + data.len() > 32 * 1024 * 1024 {
            return None;
        }
        // A fresh keyed std hasher makes collision with a real PTY title
        // impractical. The exact token must also exist in this Term's queue.
        let marker = format!("pebrel-image-{:016x}", RandomState::new().hash_one(bytes));
        let escape = format!("\x1b]2;{marker}\x07");
        let image = Arc::new(PendingImage {
            marker,
            data: Arc::new(data),
            width,
            height,
            options,
            viewport,
        });
        pending.entries.push(Arc::downgrade(&image));
        Some((escape, image))
    }

    pub(super) fn nebula_dispatch_inline_image(&mut self, title: &str) -> bool {
        let Some(index) = self
            .pending_images
            .entries
            .iter()
            .position(|image| image.upgrade().is_some_and(|image| image.marker == title))
        else {
            return false;
        };
        let Some(image) = self.pending_images.entries.remove(index).upgrade() else {
            return true;
        };
        let viewport = WindowSize {
            num_cols: self.columns() as u16,
            num_lines: self.screen_lines() as u16,
            ..image.viewport
        };
        // The grid stores the right-margin sentinel as last column + pending
        // wrap. Preserve that one-past-end position when sizing an image so
        // a full text line cannot have its last character overwritten.
        let start_column =
            self.grid.cursor.point.column.0 + usize::from(self.grid.cursor.input_needs_wrap);
        if let Some(layout) =
            image.options.layout(image.width, image.height, viewport, start_column)
        {
            let placement = self.nebula_insert_inline_image(layout);
            self.event_proxy.send_event(Event::InlineImage { data: image.data.clone(), placement });
        }
        true
    }
}
