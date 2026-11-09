use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::{Arc, Weak};

use super::*;
use crate::event::WindowSize;
use crate::inline_image::ImageOptions;
use crate::kitty_graphics::KittyGraphics;

#[derive(Default)]
pub(super) struct PendingImages {
    entries: HashMap<u64, (Weak<PendingImage>, usize, bool)>,
    bytes: usize,
    inline_count: usize,
    sequence: u64,
    nonce: RandomState,
}

pub(crate) struct PendingImage {
    payload: Payload,
}

enum Payload {
    Inline {
        data: Arc<Vec<u8>>,
        width: u32,
        height: u32,
        options: ImageOptions,
        viewport: WindowSize,
    },
    Graphics {
        state: Weak<parking_lot::Mutex<KittyGraphics>>,
        control: Vec<u8>,
        data: Vec<u8>,
    },
    ResetGraphics(Weak<parking_lot::Mutex<KittyGraphics>>),
}

impl PendingImages {
    fn queue(&mut self, payload: Payload) -> Result<(String, Arc<PendingImage>), Payload> {
        let (bytes, inline) = match &payload {
            Payload::Inline { data, .. } => (data.len(), true),
            Payload::Graphics { control, data, .. } => (control.len() + data.len(), false),
            Payload::ResetGraphics(_) => (0, false),
        };
        // 接近预算时才清扫失效弱引用；正常重放按 token O(1) 移除，避免分块的平方成本。
        if self.entries.len() >= 65536
            || self.bytes + bytes > 32 * 1024 * 1024
            || inline && self.inline_count >= 16
        {
            let mut held = self.bytes;
            let mut count = self.inline_count;
            self.entries.retain(|_, (entry, size, is_inline)| {
                let live = entry.strong_count() != 0;
                if !live {
                    held -= *size;
                    count -= usize::from(*is_inline);
                }
                live
            });
            self.bytes = held;
            self.inline_count = count;
        }
        if self.entries.len() >= 65536
            || self.bytes + bytes > 32 * 1024 * 1024
            || inline && self.inline_count >= 16
        {
            return Err(payload);
        }
        self.sequence = self.sequence.wrapping_add(1);
        let id = self.nonce.hash_one(self.sequence);
        let escape = format!("\x1b]2;pebrel-image-{id:016x}\x07");
        let image = Arc::new(PendingImage { payload });
        self.entries.insert(id, (Arc::downgrade(&image), bytes, inline));
        self.bytes += bytes;
        self.inline_count += usize::from(inline);
        Ok((escape, image))
    }
}

impl<T: EventListener> Term<T> {
    /// Private title tokens execute at VTE's replay point, including DEC 2026.
    /// The stream owns payloads; retaining a disconnected Term cannot retain uploads.
    pub(crate) fn nebula_queue_inline_image(
        &mut self,
        data: Vec<u8>,
        width: u32,
        height: u32,
        options: ImageOptions,
        viewport: WindowSize,
    ) -> Option<(String, Arc<PendingImage>)> {
        self.pending_images
            .queue(Payload::Inline { data: Arc::new(data), width, height, options, viewport })
            .ok()
    }

    pub(crate) fn nebula_queue_kitty(
        &mut self,
        state: Weak<parking_lot::Mutex<KittyGraphics>>,
        control: Vec<u8>,
        data: Vec<u8>,
    ) -> Option<(String, Arc<PendingImage>)> {
        match self.pending_images.queue(Payload::Graphics { state, control, data }) {
            Ok(command) => Some(command),
            Err(Payload::Graphics { control, .. }) => {
                KittyGraphics::rejected(self, &control);
                None
            },
            _ => unreachable!(),
        }
    }

    pub(crate) fn nebula_queue_kitty_reset(
        &mut self,
        state: Weak<parking_lot::Mutex<KittyGraphics>>,
    ) -> Option<(String, Arc<PendingImage>)> {
        match self.pending_images.queue(Payload::ResetGraphics(state)) {
            Ok(command) => Some(command),
            Err(Payload::ResetGraphics(state)) => {
                if let Some(state) = state.upgrade() {
                    state.lock().reset();
                }
                None
            },
            _ => unreachable!(),
        }
    }

    pub(crate) fn nebula_kitty_reply(&self, reply: String) {
        self.event_proxy.send_event(Event::PtyWrite(reply));
    }

    pub(crate) fn nebula_flush_kitty(
        &self,
        state: &mut KittyGraphics,
        viewport: Option<WindowSize>,
    ) {
        state.flush(&self.event_proxy, viewport);
    }

    pub(super) fn nebula_dispatch_inline_image(&mut self, title: &str) -> bool {
        let Some(id) = title
            .strip_prefix("pebrel-image-")
            .and_then(|value| u64::from_str_radix(value, 16).ok())
        else {
            return false;
        };
        let Some((image, bytes, inline)) = self.pending_images.entries.remove(&id) else {
            return false;
        };
        self.pending_images.bytes -= bytes;
        self.pending_images.inline_count -= usize::from(inline);
        let Some(image) = image.upgrade() else { return true };
        match &image.payload {
            Payload::ResetGraphics(state) => {
                if let Some(state) = state.upgrade() {
                    state.lock().reset();
                }
            },
            Payload::Graphics { state, control, data } => {
                if let Some(state) = state.upgrade() {
                    state.lock().graphic(self, control, data);
                }
            },
            Payload::Inline { data, width, height, options, viewport } => {
                let viewport = WindowSize {
                    num_cols: self.columns() as u16,
                    num_lines: self.screen_lines() as u16,
                    ..*viewport
                };
                let start_column = self.grid.cursor.point.column.0
                    + usize::from(self.grid.cursor.input_needs_wrap);
                if let Some(layout) = options.layout(*width, *height, viewport, start_column) {
                    let placement = self.nebula_insert_inline_image(layout);
                    self.event_proxy
                        .send_event(Event::InlineImage { data: data.clone(), placement });
                }
            },
        }
        true
    }
}
