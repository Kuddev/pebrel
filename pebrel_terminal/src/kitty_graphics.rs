//! Bounded RGB/RGBA graphics storage and grid-backed placements.
//! The input stream owns this state; ordered markers apply mutations at VTE replay.
//! Bitmaps are decoded once per image. Crop/scale/placement lifetime lives with cells.

use base64::Engine as _;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};

use crate::event::{Event, EventListener, WindowSize};
use crate::grid::Dimensions;
use crate::inline_image::{
    ImageIdentity, ImageLayout, ImagePlacement, KittyPlacement, MAX_IMAGE_CELLS,
};
use crate::term::{Term, TermMode};

mod control;
use control::{Control, Reply, Result};

const MAX_PIXELS: usize = 16 * 1024 * 1024;
const MAX_ENCODED_BYTES: usize = 24 * 1024 * 1024;
const MAX_IMAGES: usize = 16;
const MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;
const ALT_KEY: u64 = 1 << 63;
const ANONYMOUS_KEY: u64 = 1 << 32;

struct Bitmap {
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    channels: u8,
    cost: usize,
    delete_when_unplaced: bool,
}

struct Transfer {
    key: u64,
    control: Control,
    encoded: Vec<u8>,
}

#[derive(Default)]
pub struct KittyGraphics {
    current: Option<Transfer>,
    images: HashMap<u64, Bitmap>,
    identities: HashMap<u64, Weak<ImageIdentity>>,
    placements: HashMap<(u64, u64), Weak<KittyPlacement>>,
    image_order: VecDeque<u64>,
    image_bytes: usize,
    published: HashSet<u64>,
    cell_size_queries: bool,
    viewport: Option<WindowSize>,
    anonymous: u64,
}

impl KittyGraphics {
    fn screen<U: EventListener>(terminal: &Term<U>) -> u64 {
        if terminal.mode().contains(TermMode::ALT_SCREEN) { ALT_KEY } else { 0 }
    }

    pub(crate) fn resize(&mut self, size: WindowSize) {
        self.viewport = Some(size);
    }

    pub(crate) fn reset(&mut self) {
        for identity in self.identities.values().filter_map(Weak::upgrade) {
            identity.invalidate();
        }
        let viewport = self.viewport;
        *self = Self { viewport, ..Self::default() };
    }

    fn send<U: EventListener>(terminal: &Term<U>, reply: Reply, result: Result<()>) {
        if let Some(message) = reply.message(result) {
            terminal.nebula_kitty_reply(message);
        }
    }

    pub(crate) fn rejected<U: EventListener>(terminal: &Term<U>, control: &[u8]) {
        Self::send(terminal, Reply::read(control), Err("ENOSPC"));
    }

    /// Capability queries have no grid/storage side effects and need not wait for DEC 2026.
    pub(crate) fn try_query<U: EventListener>(
        &self,
        terminal: &Term<U>,
        bytes: &[u8],
        data: &[u8],
    ) -> bool {
        if !control::single_query(bytes) {
            return false;
        }
        let reply = Reply::read(bytes);
        let result =
            Control::parse(bytes).and_then(|control| Self::decode(&control, data).map(|_| ()));
        Self::send(terminal, reply, result);
        true
    }

    pub fn graphic<U: EventListener>(&mut self, terminal: &mut Term<U>, bytes: &[u8], data: &[u8]) {
        if self.try_query(terminal, bytes, data) {
            return;
        }
        let control = match Control::parse(bytes) {
            Ok(control) => control,
            Err(error) => {
                if control::is_delete(bytes) {
                    self.current = None;
                }
                Self::send(terminal, Reply::read(bytes), Err(error));
                return;
            },
        };
        self.prune_dead();
        let screen = Self::screen(terminal);
        let continuation = self.current.as_ref().is_some_and(|transfer| {
            matches!(control.action, None | Some(b't' | b'T'))
                && control.width.is_none()
                && control.height.is_none()
                && control.reply.id.is_none_or(|id| transfer.control.reply.id == Some(id))
                && transfer.key & ALT_KEY == screen
        });
        if continuation {
            let mut transfer = self.current.take().unwrap();
            if let Some(quiet) = control.quiet {
                transfer.control.reply.quiet = quiet;
            }
            transfer.control.more = control.more;
            self.receive(terminal, transfer, data);
            return;
        }
        match control.action.unwrap_or(b't') {
            b't' | b'T' | b'q' => {
                // 新头终止旧半包，但旧的完整图片保留到新数据校验成功，避免加载中闪白。
                self.current = None;
                let reply = control.reply;
                if let Err(error) = control.dimensions() {
                    Self::send(terminal, reply, Err(error));
                    return;
                }
                let id = match reply.id {
                    Some(id) => u64::from(id),
                    None => {
                        self.anonymous = self.anonymous.wrapping_add(1) & (ANONYMOUS_KEY - 1);
                        ANONYMOUS_KEY | self.anonymous
                    },
                };
                self.receive(
                    terminal,
                    Transfer { key: screen | id, control, encoded: Vec::new() },
                    data,
                );
            },
            b'p' => {
                let result = control
                    .reply
                    .id
                    .ok_or("EINVAL")
                    .and_then(|id| self.place(terminal, screen | u64::from(id), &control));
                Self::send(terminal, control.reply, result);
            },
            b'd' => {
                self.current = None;
                self.delete(terminal, &control);
            },
            _ => {},
        }
    }

    fn decode(control: &Control, encoded: &[u8]) -> Result<Vec<u8>> {
        let (_, _, pixels) = control.dimensions()?;
        let needed = pixels * usize::from(control.channels);
        let maximum = needed.div_ceil(3).saturating_mul(4).min(MAX_ENCODED_BYTES);
        if encoded.len() > maximum {
            return Err("E2BIG");
        }
        let pixels =
            base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| "EINVAL")?;
        if pixels.len() != needed {
            return Err("EINVAL");
        }
        Ok(pixels)
    }

    fn receive<U: EventListener>(
        &mut self,
        terminal: &mut Term<U>,
        mut transfer: Transfer,
        data: &[u8],
    ) {
        let control = &transfer.control;
        let (width, height, pixels) = control.dimensions().unwrap();
        let maximum = (pixels * usize::from(control.channels))
            .div_ceil(3)
            .saturating_mul(4)
            .min(MAX_ENCODED_BYTES);
        if data.len() > maximum.saturating_sub(transfer.encoded.len()) {
            Self::send(terminal, control.reply, Err("E2BIG"));
            return;
        }
        let size = transfer.encoded.len() + data.len();
        if transfer.encoded.capacity() < size {
            let capacity = transfer.encoded.capacity().saturating_mul(2).max(size).min(maximum);
            transfer.encoded.reserve_exact(capacity - transfer.encoded.len());
        }
        transfer.encoded.extend_from_slice(data);
        if control.more {
            self.current = Some(transfer);
            return;
        }
        let decoded = match Self::decode(control, &transfer.encoded) {
            Ok(decoded) => decoded,
            Err(error) => {
                Self::send(terminal, control.reply, Err(error));
                return;
            },
        };
        if control.action == Some(b'q') {
            Self::send(terminal, control.reply, Ok(()));
            return;
        }
        self.forget(transfer.key);
        // 用最终 BGRA 成本约束原始仓库；RGB 不应在前端扩张后突破同一图片预算。
        let cost = pixels * 4;
        while self.images.len() >= MAX_IMAGES || self.image_bytes + cost > MAX_IMAGE_BYTES {
            let candidate = self
                .image_order
                .iter()
                .copied()
                .find(|key| !self.has_placements(*key))
                .or_else(|| self.image_order.front().copied());
            let Some(oldest) = candidate else { break };
            self.forget(oldest);
        }
        self.image_bytes += cost;
        self.image_order.push_back(transfer.key);
        self.images.insert(
            transfer.key,
            Bitmap {
                pixels: Arc::new(decoded),
                width,
                height,
                channels: control.channels,
                cost,
                delete_when_unplaced: false,
            },
        );
        terminal.nebula_images_changed();
        let result = if control.action == Some(b'T') {
            self.place(terminal, transfer.key, control)
        } else {
            Ok(())
        };
        Self::send(terminal, control.reply, result);
    }

    fn place<U: EventListener>(
        &mut self,
        terminal: &mut Term<U>,
        key: u64,
        control: &Control,
    ) -> Result<()> {
        if self.placements.len() >= MAX_IMAGE_CELLS {
            self.placements
                .retain(|_, placement| placement.upgrade().is_some_and(|p| p.is_alive()));
        }
        let bitmap = self.images.get(&key).ok_or("ENOENT")?;
        let viewport = self.viewport.unwrap_or(WindowSize {
            num_cols: terminal.columns() as u16,
            num_lines: terminal.screen_lines() as u16,
            cell_width: 9,
            cell_height: 20,
        });
        let resource = self
            .identities
            .get(&key)
            .and_then(Weak::upgrade)
            .filter(|id| id.is_alive())
            .unwrap_or_else(ImageIdentity::new);
        let identity = ImageIdentity::new();
        let placement = control.placement(
            bitmap.width,
            bitmap.height,
            viewport,
            resource.clone(),
            &identity,
        )?;
        let slot = control.reply.placement.map(u64::from).unwrap_or(ALT_KEY | identity.id);
        if !self.placements.contains_key(&(key, slot)) && self.placements.len() >= MAX_IMAGE_CELLS {
            return Err("ENOSPC");
        }
        if let Some(previous) = self
            .placements
            .insert((key, slot), Arc::downgrade(&placement))
            .and_then(|previous| previous.upgrade())
        {
            previous.invalidate();
        }
        self.identities.insert(key, Arc::downgrade(&resource));
        terminal.nebula_insert_kitty_block(&identity, placement, control.keep_cursor);
        // 再次放置允许重试已被前端预算淘汰的纹理；前端按 resource id 合并在途/已缓存解码。
        self.published.remove(&key);
        Ok(())
    }

    fn has_placements(&self, key: u64) -> bool {
        self.placements.iter().any(|((image, _), placement)| {
            *image == key && placement.upgrade().is_some_and(|placement| placement.is_alive())
        })
    }

    fn delete<U: EventListener>(&mut self, terminal: &mut Term<U>, control: &Control) {
        let screen = Self::screen(terminal);
        let selected = match control.selector.to_ascii_lowercase() {
            b'a' => None,
            b'i' => match control.reply.id {
                Some(id) => Some(screen | u64::from(id)),
                None => return,
            },
            _ => {
                Self::send(terminal, control.reply, Err("ENOTSUP"));
                return;
            },
        };
        let visible = (selected.is_none()).then(|| terminal.nebula_visible_kitty_placements());
        let free = control.selector.is_ascii_uppercase();
        let mut affected = HashSet::new();
        self.placements.retain(|(key, slot), weak| {
            let Some(placement) = weak.upgrade() else { return false };
            let matches = *key & ALT_KEY == screen
                && selected.is_none_or(|selected| *key == selected)
                && control.reply.placement.is_none_or(|id| *slot == u64::from(id))
                && visible.as_ref().is_none_or(|visible| visible.contains(&placement.id));
            if matches {
                placement.invalidate();
                affected.insert(*key);
            }
            !matches && placement.is_alive()
        });
        if let Some(key) = selected {
            affected.insert(key);
        }
        for key in affected {
            if free {
                if !self.has_placements(key) {
                    self.forget(key);
                } else if let Some(bitmap) = self.images.get_mut(&key) {
                    bitmap.delete_when_unplaced = true;
                }
            }
        }
        terminal.nebula_images_changed();
    }

    fn forget(&mut self, key: u64) {
        if let Some(identity) = self.identities.remove(&key).and_then(|id| id.upgrade()) {
            identity.invalidate();
        }
        self.placements.retain(|(image, _), placement| {
            if *image != key {
                return true;
            }
            if let Some(placement) = placement.upgrade() {
                placement.invalidate();
            }
            false
        });
        self.published.remove(&key);
        if let Some(bitmap) = self.images.remove(&key) {
            self.image_bytes -= bitmap.cost;
        }
        self.image_order.retain(|id| *id != key);
    }

    fn prune_dead(&mut self) {
        let dead: Vec<_> = self
            .identities
            .iter()
            .filter_map(|(key, identity)| {
                identity.upgrade().is_none_or(|identity| !identity.is_alive()).then_some(*key)
            })
            .collect();
        for key in dead {
            self.identities.remove(&key);
            self.published.remove(&key);
        }
        let retired: Vec<_> = self
            .images
            .iter()
            .filter_map(|(key, bitmap)| {
                (bitmap.delete_when_unplaced && !self.has_placements(*key)).then_some(*key)
            })
            .collect();
        for key in retired {
            self.forget(key);
        }
    }

    pub fn flush<U: EventListener>(&mut self, event_proxy: &U, _window_size: Option<WindowSize>) {
        self.prune_dead();
        for (&key, identity) in &self.identities {
            if self.published.contains(&key) {
                continue;
            }
            let (Some(identity), Some(bitmap)) = (identity.upgrade(), self.images.get(&key)) else {
                continue;
            };
            let Some(placement) = self
                .placements
                .iter()
                .filter(|((image, _), _)| *image == key)
                .filter_map(|(_, weak)| weak.upgrade())
                .find(|p| p.is_alive())
            else {
                continue;
            };
            let layout = ImageLayout {
                width: bitmap.width as f32,
                height: bitmap.height as f32,
                cell_width: bitmap.width as f32 / placement.columns as f32,
                cell_height: bitmap.height as f32 / placement.rows as f32,
                columns: placement.columns,
                rows: placement.rows,
            };
            self.published.insert(key);
            event_proxy.send_event(Event::KittyImage {
                data: Arc::downgrade(&bitmap.pixels),
                channels: bitmap.channels,
                placement: ImagePlacement {
                    id: identity.id,
                    lifetime: Arc::downgrade(&identity),
                    layout,
                },
            });
        }
    }

    pub fn request_cell_size(&mut self) {
        self.cell_size_queries = true;
    }

    pub fn answer_cell_size_queries<U: EventListener>(
        &mut self,
        size: Option<WindowSize>,
        event_proxy: &U,
    ) {
        let Some(size) = size else { return };
        if !self.cell_size_queries {
            return;
        }
        self.cell_size_queries = false;
        event_proxy.send_event(Event::PtyWrite(format!(
            "\x1b[6;{};{}t",
            size.cell_height.max(1),
            size.cell_width.max(1)
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::VoidListener;
    use crate::term::{Config, test::TermSize};

    fn terminal() -> Term<VoidListener> {
        Term::new(Config::default(), &TermSize::new(20, 8), VoidListener)
    }

    #[test]
    fn raw_image_store_has_a_count_budget_even_without_placements() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        for id in 1..=MAX_IMAGES + 1 {
            gfx.graphic(&mut term, format!("a=t,i={id},s=1,v=1,f=32").as_bytes(), b"AQIDBA==");
        }
        assert_eq!(gfx.images.len(), MAX_IMAGES);
        assert_eq!(gfx.image_bytes, MAX_IMAGES * 4);
        assert!(!gfx.images.contains_key(&1));
        assert!(gfx.images.values().all(|image| image.width == 1 && image.height == 1));
    }

    #[test]
    fn last_chunk_is_checked_before_joining_or_decoding_and_abort_releases_state() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=t,i=1,s=1,v=1,f=32,m=1", b"AQID");
        // 1x1 RGBA 最多 8 个 base64 字节；用 12 字节验证边界，不分配大图。
        gfx.graphic(&mut term, b"m=0", b"BA==AAAA");
        assert!(gfx.current.is_none());
        assert!(gfx.images.is_empty());
        assert!(gfx.placements.is_empty());
        assert!(gfx.current.is_none());
        for id in 1..=32 {
            gfx.graphic(&mut term, format!("a=t,i={id},s=1,v=1,m=1").as_bytes(), b"AQID");
        }
        assert_eq!(gfx.current.as_ref().unwrap().encoded.len(), 4);
        assert_eq!(gfx.current.as_ref().unwrap().control.width, Some(1));
        gfx.graphic(&mut term, b"a=d,d=A", b"");
        assert!(gfx.current.is_none());
        assert!(gfx.placements.is_empty());
    }

    #[test]
    fn cleared_grid_releases_placement_but_keeps_bounded_data_for_replacement() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=T,i=7,s=1,v=1,c=1,r=1", b"AQIDBA==");
        let owner = gfx.identities[&7].clone();
        let pixels = Arc::downgrade(&gfx.images[&7].pixels);
        let mut parser: vte::ansi::Processor = Default::default();
        parser.advance(&mut term, b"\x1b[2J\x1b[3J");
        assert_eq!(owner.strong_count(), 0, "only grid cells may keep image identities alive");
        gfx.flush(&VoidListener, None);
        assert!(gfx.identities.is_empty());
        assert!(!gfx.has_placements(7));
        assert!(pixels.upgrade().is_some());
        assert_eq!(gfx.image_bytes, 4);
        gfx.graphic(&mut term, b"a=p,i=7,c=1,r=1", b"");
        assert!(gfx.has_placements(7));
        gfx.graphic(&mut term, b"a=d,d=I,i=7", b"");
        assert!(pixels.upgrade().is_none());
        assert_eq!(gfx.image_bytes, 0);
    }

    #[test]
    fn headerless_new_ids_do_not_accumulate_partial_buffers() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        for id in 1..=MAX_IMAGES {
            gfx.graphic(&mut term, format!("a=t,i={id},s=1,v=1,f=32").as_bytes(), b"AQIDBA==");
        }
        for id in 1..=MAX_IMAGES {
            gfx.graphic(&mut term, format!("a=t,i={id},m=1").as_bytes(), b"AQID");
        }
        assert!(gfx.current.is_none());
        assert!(gfx.current.is_none());
        assert_eq!(gfx.image_bytes, MAX_IMAGES * 4);
    }

    #[test]
    fn lowercase_delete_keeps_pixels_and_uppercase_delete_releases_them() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=T,i=7,s=1,v=1,c=1,r=1", b"AQIDBA==");
        let old_placement = gfx.placements.values().find_map(Weak::upgrade).unwrap();
        gfx.graphic(&mut term, b"a=d,d=i,i=7", b"");
        assert!(!old_placement.is_alive());
        assert_eq!(gfx.image_bytes, 4);
        gfx.graphic(&mut term, b"a=p,i=7,c=1,r=1", b"");
        assert!(gfx.has_placements(7));
        gfx.graphic(&mut term, b"a=d,d=A", b"");
        assert!(gfx.images.is_empty());
        assert!(gfx.identities.is_empty());
        assert_eq!(gfx.image_bytes, 0);
    }

    #[test]
    fn deleting_an_unrelated_id_also_aborts_the_partial_upload() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=T,i=7,s=1,v=1,m=1", b"AQID");
        gfx.graphic(&mut term, b"a=d,d=i,i=9", b"");
        assert!(gfx.current.is_none());
        assert!(gfx.current.is_none());
        assert!(gfx.placements.is_empty());
        assert!(gfx.current.is_none());
        gfx.graphic(&mut term, b"m=0", b"BA==");
        assert!(gfx.images.is_empty());
    }

    #[test]
    fn chunked_transmit_and_place_preserves_the_first_headers_action() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=T,i=7,s=1,v=1,c=1,r=1,m=1", b"AQID");
        gfx.graphic(&mut term, b"m=0", b"BA==");
        assert_eq!(gfx.identities[&7].strong_count(), 1);
        assert!(gfx.current.is_none());
    }
}
