//! Bounded RGB/RGBA graphics storage and grid-backed placements.
//! The input stream owns this state; ordered markers apply mutations at VTE replay.
//! Bitmaps are decoded once per image. Crop/scale/placement lifetime lives with cells.

use base64::Engine as _;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};

use crate::event::{Event, EventListener, WindowSize};
use crate::grid::Dimensions;
use crate::inline_image::{
    ImageFormat, ImageIdentity, ImageLayout, ImagePlacement, KittyPlacement, MAX_IMAGE_CELLS,
};
use crate::term::{Term, TermMode};

mod control;
pub(crate) use control::is_feature_query;
use control::{Control, Reply, Result};

const MAX_PIXELS: usize = 16 * 1024 * 1024;
/// Maximum decoded body of one direct graphics transfer. Consumers use the
/// same bound for PNG containers; the wire representation is base64 below.
pub const MAX_TRANSFER_BYTES: usize = 48 * 1024 * 1024;
const MAX_ENCODED_BYTES: usize = MAX_TRANSFER_BYTES.div_ceil(3) * 4;
const MAX_IMAGES: usize = 16;
const MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;
const ALT_KEY: u64 = 1 << 63;
const ANONYMOUS_KEY: u64 = 1 << 32;

/// Accepts base64 with or without the canonical `=` padding; clients that split
/// a stream by hand or strip the tail never round-trip through it exactly.
const BASE64: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
);

struct Bitmap {
    number: Option<u32>,
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    channels: u8,
    format: ImageFormat,
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
    assigned_id: u32,
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
        Self::send_features(terminal, reply, None, result);
    }

    fn send_features<U: EventListener>(
        terminal: &Term<U>,
        reply: Reply,
        features: Option<&str>,
        result: Result<()>,
    ) {
        if let Some(message) = reply.message(features, result) {
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
        let control = match Control::parse(bytes) {
            Ok(control) => control,
            Err(error) => {
                Self::send(terminal, reply, Err(error));
                return true;
            },
        };
        if control.feature.is_some() {
            self.answer_features(terminal, &control);
            return true;
        }
        let result = match control.format {
            ImageFormat::Raw => Self::decode(&control, data).map(|_| ()),
            ImageFormat::Png => Self::decode_png(data).map(|_| ()),
        };
        Self::send(terminal, reply, result);
        true
    }

    /// Replies to an `a=q,e=<action>` probe: a supported action echoes the query
    /// keys it honours, an unsupported one reports `ENOTSUP` without naming `e`.
    fn answer_features<U: EventListener>(&self, terminal: &Term<U>, control: &Control) {
        let reply = Reply { placement: None, ..control.reply };
        match control.queried_action() {
            Some(action) => {
                let features = control.features(action);
                Self::send_features(terminal, reply, Some(&features), Ok(()));
            },
            None => Self::send(terminal, reply, Err("ENOTSUP")),
        }
    }

    pub fn graphic<U: EventListener>(&mut self, terminal: &mut Term<U>, bytes: &[u8], data: &[u8]) {
        if self.try_query(terminal, bytes, data) {
            return;
        }
        let mut control = match Control::parse(bytes) {
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
                && !control.format_specified
                && control
                    .reply
                    .number
                    .is_none_or(|number| transfer.control.reply.number == Some(number))
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
                // A container carries its own geometry; asking it for `s`/`v`
                // would refuse every PNG a client sends.
                if control.format != ImageFormat::Png {
                    if let Err(error) = control.dimensions() {
                        Self::send(terminal, reply, Err(error));
                        return;
                    }
                }
                let id = match reply.id {
                    Some(id) => u64::from(id),
                    None if reply.number.is_some() => {
                        let id = match self.allocate_id() {
                            Ok(id) => id,
                            Err(error) => {
                                Self::send(terminal, reply, Err(error));
                                return;
                            },
                        };
                        control.reply.id = Some(id);
                        u64::from(id)
                    },
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
                let key = self.reference_key(screen, control.reply);
                if let Ok(key) = key {
                    control.reply.id = Some(key as u32);
                }
                let result = key.and_then(|key| self.place(terminal, key, &control));
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
        let pixels = BASE64.decode(encoded).map_err(|_| "EINVAL")?;
        if pixels.len() != needed {
            return Err("EINVAL");
        }
        Ok(pixels)
    }

    /// Decodes a PNG transfer and reads its geometry from the mandatory `IHDR`
    /// chunk, which is all the terminal needs in order to place it on the grid.
    fn decode_png(encoded: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
        if encoded.len() > MAX_ENCODED_BYTES {
            return Err("E2BIG");
        }
        let png = BASE64.decode(encoded).map_err(|_| "EINVAL")?;
        let (width, height) = Self::png_size(&png)?;
        Ok((png, width, height))
    }

    fn png_size(png: &[u8]) -> Result<(u32, u32)> {
        const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        if png.len() < 33
            || png[..8] != SIGNATURE
            || png[8..12] != 13_u32.to_be_bytes()
            || &png[12..16] != b"IHDR"
            || png[26] != 0
            || png[27] != 0
            || png[28] > 1
        {
            return Err("EINVAL");
        }
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        let depth = png[24];
        let channels = match (png[25], depth) {
            (0, 1 | 2 | 4 | 8 | 16) => 1,
            (2, 8 | 16) => 3,
            (3, 1 | 2 | 4 | 8) => 4,
            (4, 8 | 16) => 2,
            (6, 8 | 16) => 4,
            _ => return Err("EINVAL"),
        };
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 {
            return Err("EINVAL");
        }
        // The decoder may retain 16-bit source samples before RGBA8 conversion.
        let bytes_per_pixel = (channels * u64::from(depth.div_ceil(8))).max(4);
        if pixels > MAX_PIXELS as u64 || pixels * bytes_per_pixel > MAX_PIXELS as u64 * 4 {
            return Err("E2BIG");
        }
        // A truecolor tRNS chunk makes the decoder expand RGB16 to RGBA16.
        // Only inspect chunk headers when that expansion could change admission.
        if depth == 16 && png[25] == 2 && pixels * 8 > MAX_PIXELS as u64 * 4 {
            let mut chunks = &png[33..];
            while chunks.len() >= 12 {
                let kind = &chunks[4..8];
                if kind == b"tRNS" {
                    return Err("E2BIG");
                }
                if kind == b"IDAT" || kind == b"IEND" {
                    break;
                }
                let size = u32::from_be_bytes(chunks[..4].try_into().unwrap()) as usize;
                chunks = chunks.get(12..).and_then(|rest| rest.get(size..)).ok_or("EINVAL")?;
            }
        }
        Ok((width, height))
    }

    fn receive<U: EventListener>(
        &mut self,
        terminal: &mut Term<U>,
        mut transfer: Transfer,
        data: &[u8],
    ) {
        let control = &transfer.control;
        let maximum = match control.format {
            // A container is bounded by the staging budget alone; its pixel
            // budget is checked once the header names a size.
            ImageFormat::Png => MAX_ENCODED_BYTES,
            ImageFormat::Raw => {
                let (_, _, pixels) = control.dimensions().unwrap();
                (pixels * usize::from(control.channels))
                    .div_ceil(3)
                    .saturating_mul(4)
                    .min(MAX_ENCODED_BYTES)
            },
        };
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
        let (decoded, width, height) = match control.format {
            ImageFormat::Png => match Self::decode_png(&transfer.encoded) {
                Ok((png, width, height)) => (png, width, height),
                Err(error) => {
                    Self::send(terminal, control.reply, Err(error));
                    return;
                },
            },
            ImageFormat::Raw => match Self::decode(control, &transfer.encoded) {
                Ok(decoded) => {
                    let (width, height, _) = control.dimensions().unwrap();
                    (decoded, width, height)
                },
                Err(error) => {
                    Self::send(terminal, control.reply, Err(error));
                    return;
                },
            },
        };
        if control.action == Some(b'q') {
            Self::send(terminal, control.reply, Ok(()));
            return;
        }
        self.forget(transfer.key);
        // Each retained resource must fit both its source bytes and its eventual
        // BGRA bitmap. Compressed pages cannot hide their decoded cache cost.
        let cost = decoded.len().max(width as usize * height as usize * 4);
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
                number: control.reply.number,
                pixels: Arc::new(decoded),
                width,
                height,
                channels: control.channels,
                format: control.format,
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

    fn allocate_id(&mut self) -> Result<u32> {
        for _ in 0..=MAX_IMAGES {
            self.assigned_id = self.assigned_id.wrapping_add(1).max(1);
            if !self.images.keys().any(|key| *key & !ALT_KEY == u64::from(self.assigned_id)) {
                return Ok(self.assigned_id);
            }
        }
        Err("ENOSPC")
    }

    fn reference_key(&self, screen: u64, reply: Reply) -> Result<u64> {
        if let Some(id) = reply.id {
            return Ok(screen | u64::from(id));
        }
        let number = reply.number.ok_or("EINVAL")?;
        self.image_order
            .iter()
            .rev()
            .copied()
            .find(|key| {
                *key & ALT_KEY == screen
                    && self.images.get(key).is_some_and(|image| image.number == Some(number))
            })
            .ok_or("ENOENT")
    }

    fn delete<U: EventListener>(&mut self, terminal: &mut Term<U>, control: &Control) {
        let screen = Self::screen(terminal);
        let selector = control.selector.to_ascii_lowercase();
        let mut reply = control.reply;
        let selected: Result<Option<HashSet<u64>>> = match selector {
            b'i' if reply.id.is_some() => {
                Ok(Some(HashSet::from([screen | u64::from(reply.id.unwrap())])))
            },
            b'n' if reply.number.is_some() => self.reference_key(screen, reply).map(|key| {
                reply.id = Some(key as u32);
                Some(HashSet::from([key]))
            }),
            b'r' if control.x <= control.y => Ok(Some(
                self.images
                    .keys()
                    .copied()
                    .filter(|key| {
                        *key & ALT_KEY == screen
                            && *key >= screen | u64::from(control.x)
                            && *key <= screen | u64::from(control.y)
                    })
                    .collect(),
            )),
            b'a' | b'c' | b'p' | b'x' | b'y' => Ok(None),
            b'i' | b'n' | b'r' => Err("EINVAL"),
            _ => Err("ENOTSUP"),
        };
        let selected = match selected {
            Ok(keys) => keys,
            Err(error) => {
                Self::send(terminal, reply, Err(error));
                return;
            },
        };
        let coordinate = |value: u32| value.checked_sub(1).map(|n| n as usize).ok_or("EINVAL");
        let cells: Result<Option<HashSet<u64>>> = match selector {
            b'a' => Ok(Some(terminal.nebula_visible_kitty_placements())),
            b'c' => {
                let (column, row) = terminal.nebula_cursor_cell();
                Ok(Some(terminal.nebula_kitty_placements_at(Some(column), Some(row))))
            },
            b'p' => coordinate(control.x).and_then(|column| {
                coordinate(control.y)
                    .map(|row| Some(terminal.nebula_kitty_placements_at(Some(column), Some(row))))
            }),
            b'x' => coordinate(control.x)
                .map(|column| Some(terminal.nebula_kitty_placements_at(Some(column), None))),
            b'y' => coordinate(control.y)
                .map(|row| Some(terminal.nebula_kitty_placements_at(None, Some(row)))),
            _ => Ok(None),
        };
        let cells = match cells {
            Ok(cells) => cells,
            Err(error) => {
                Self::send(terminal, reply, Err(error));
                return;
            },
        };
        let free = control.selector.is_ascii_uppercase();
        let by_placement = matches!(selector, b'i' | b'n') && reply.placement.is_some();
        let mut affected =
            if by_placement { HashSet::new() } else { selected.clone().unwrap_or_default() };
        self.placements.retain(|(key, slot), weak| {
            let Some(placement) = weak.upgrade() else { return false };
            let matches = *key & ALT_KEY == screen
                && selected.as_ref().is_none_or(|keys| keys.contains(key))
                && cells.as_ref().is_none_or(|ids| ids.contains(&placement.id))
                && (!matches!(selector, b'i' | b'n')
                    || reply.placement.is_none_or(|id| *slot == u64::from(id)));
            if matches {
                placement.invalidate();
                affected.insert(*key);
            }
            !matches && placement.is_alive()
        });
        if free {
            for key in affected {
                if !self.has_placements(key) {
                    self.forget(key);
                } else if let Some(bitmap) = self.images.get_mut(&key) {
                    bitmap.delete_when_unplaced = true;
                }
            }
        }
        terminal.nebula_images_changed();
        Self::send(terminal, reply, Ok(()));
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
                    format: bitmap.format,
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
    fn signed_placement_layer_does_not_select_compression() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        // A page reader sends a signed placement layer together with its crop.
        gfx.graphic(
            &mut term,
            b"a=T,t=d,f=24,s=2,v=1,i=1,p=1,C=1,q=1,x=0,y=0,w=2,h=1,z=-1",
            b"AAAAAAAA",
        );
        assert!(gfx.current.is_none());
        assert_eq!(gfx.images[&1].width, 2);
        assert_eq!(gfx.images[&1].channels, 3);
        assert!(gfx.has_placements(1));
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

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);
        png
    }

    /// The wire carries the container base64-encoded, like every other payload.
    fn encoded_png(width: u32, height: u32) -> Vec<u8> {
        use base64::Engine as _;
        BASE64.encode(png(width, height)).into_bytes()
    }

    #[test]
    fn a_container_transfer_is_read_from_its_own_header() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        let payload = encoded_png(3, 2);
        gfx.graphic(&mut term, b"a=T,f=100,i=5,c=1,r=1", &payload);
        assert!(gfx.current.is_none());
        let image = &gfx.images[&5];
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(image.format, ImageFormat::Png);
        assert_eq!(image.cost, png(3, 2).len());
        assert!(gfx.has_placements(5));
    }

    #[test]
    fn a_container_that_is_not_a_png_is_refused() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=T,f=100,i=5,c=1,r=1", b"not a png at all");
        assert!(gfx.images.is_empty());
        assert!(gfx.placements.is_empty());
        assert_eq!(gfx.image_bytes, 0);
    }

    #[test]
    fn png_geometry_reservations_include_the_decoded_bitmap_cost() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        // Header-only fixture: reservation happens before the background PNG
        // decoder validates IDAT and CRCs. Real PNG decoding is covered there.
        let payload = encoded_png(4096, 4096);
        for id in 1..=3 {
            let control = format!("a=T,f=100,i={id},c=1,r=1");
            gfx.graphic(&mut term, control.as_bytes(), &payload);
            assert_eq!(gfx.images[&id].cost, 64 * 1024 * 1024);
            assert!(gfx.image_bytes <= MAX_IMAGE_BYTES);
        }
        assert_eq!(gfx.images.len(), 2);
        assert!(!gfx.images.contains_key(&1));
    }

    #[test]
    fn a_payload_without_canonical_padding_is_still_decoded() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        // 1x1 RGBA needs 4 bytes; "AQIDBA" is the unpadded spelling of them.
        gfx.graphic(&mut term, b"a=T,i=3,s=1,v=1,f=32", b"AQIDBA");
        assert_eq!(gfx.images[&3].pixels.len(), 4);
    }

    #[test]
    fn a_feature_query_reports_only_the_keys_this_terminal_honours() {
        let control = control::Control::parse(b"a=q,i=41,e=T,U=1,C=0,r=1").unwrap();
        assert_eq!(control.feature, Some(b'T'));
        assert_eq!(control.queried_action(), Some(b'T'));
        assert_eq!(control.features(b'T'), "e=T,r=1");

        // A field this terminal does not implement stays out of the answer.
        let control = control::Control::parse(b"a=q,i=42,e=f,r=1").unwrap();
        assert_eq!(control.features(b'f'), "e=f");

        // Answers are single key/value pairs per comma, never a fake value.
        let control = control::Control::parse(b"a=q,i=43,e=p,c=2,r=2,x=0,y=0").unwrap();
        assert_eq!(control.features(b'p'), "e=p,c=2,r=2");
    }

    #[test]
    fn a_feature_query_for_an_unimplemented_action_is_refused() {
        let control = control::Control::parse(b"a=q,i=42,e=f,r=1").unwrap();
        assert_eq!(control.queried_action(), None);
    }

    #[test]
    fn a_feature_query_on_a_transfer_action_is_refused() {
        // The marker is only defined for queries; `f=100` would otherwise be
        // silently downgraded to RGBA by the lenient branch.
        assert!(control::Control::parse(b"a=T,e=T,f=100").is_err());
        assert!(control::Control::parse(b"a=T,e=T").is_err());
        assert!(control::Control::parse(b"a=q,i=1,e=T").is_ok());
        assert!(control::Control::parse(b"a=q,I=7,e=T").is_err());
    }

    #[test]
    fn a_png_payload_is_accepted_without_a_pixel_size_in_the_header() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=T,f=100,i=6,c=1,r=1", &encoded_png(4, 3));
        assert_eq!(gfx.images[&6].width, 4);
        assert_eq!(gfx.images[&6].height, 3);
    }

    #[test]
    fn delete_by_placement_leaves_the_other_placements_alone() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=T,i=7,s=1,v=1,c=1,r=1,p=1", b"AQIDBA==");
        gfx.graphic(&mut term, b"a=p,i=7,c=1,r=1,p=2", b"");
        assert_eq!(gfx.placements.len(), 2);
        gfx.graphic(&mut term, b"a=d,d=i,i=7,p=1", b"");
        assert_eq!(gfx.placements.len(), 1);
        assert!(gfx.placements.keys().all(|(image, placement)| *image == 7 && *placement == 2));
    }

    #[test]
    fn delete_by_id_range_removes_the_placements_inside_it() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        for id in 8..=10 {
            gfx.graphic(&mut term, format!("a=T,i={id},s=1,v=1,c=1,r=1").as_bytes(), b"AQIDBA==");
        }
        assert_eq!(gfx.placements.len(), 3);
        gfx.graphic(&mut term, b"a=d,d=r,x=8,y=9", b"");
        assert_eq!(gfx.placements.len(), 1);
        assert!(gfx.placements.keys().any(|(image, _)| *image == 10));
        assert_eq!(gfx.image_bytes, 4 * 3);
    }

    #[test]
    fn delete_by_cursor_cell_removes_only_the_placement_under_it() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        // `C=1` keeps the cursor on the image, which is the cell `d=c` names.
        gfx.graphic(&mut term, b"a=T,i=7,s=1,v=1,c=1,r=1,C=1", b"AQIDBA==");
        assert!(gfx.has_placements(7));
        gfx.graphic(&mut term, b"a=d,d=c", b"");
        assert!(!gfx.has_placements(7));
        assert_eq!(gfx.image_bytes, 4);
    }

    #[test]
    fn numbered_images_allocate_unique_ids_and_delete_only_the_newest_match() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=t,i=1,s=1,v=1", b"AQIDBA==");
        let mut ids = Vec::new();
        for number in [7, 9, 7] {
            let command = format!("a=T,I={number},s=1,v=1,c=1,r=1");
            gfx.graphic(&mut term, command.as_bytes(), b"AQIDBA==");
            ids.push(*gfx.image_order.back().unwrap());
        }
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 3);
        assert!(ids.iter().all(|id| *id != 1 && *id <= u64::from(u32::MAX)));
        gfx.graphic(&mut term, b"a=d,d=n,I=7", b"");
        assert!(!gfx.has_placements(ids[2]));
        assert!(gfx.has_placements(ids[0]) && gfx.has_placements(ids[1]));
        assert!(gfx.images.contains_key(&ids[2]), "lowercase deletion retains source data");
        gfx.graphic(&mut term, b"a=d,d=N,I=7", b"");
        assert!(!gfx.images.contains_key(&ids[2]));
        gfx.graphic(&mut term, b"a=p,I=7,c=1,r=1", b"");
        assert!(gfx.has_placements(ids[0]));
        let before = gfx.images.len();
        gfx.graphic(&mut term, b"a=t,i=1,I=7,s=1,v=1", b"AQIDBA==");
        assert_eq!(gfx.images.len(), before);
        assert_eq!(gfx.images[&1].pixels.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn absent_placement_id_does_not_release_or_schedule_its_image_for_deletion() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        gfx.graphic(&mut term, b"a=t,I=7,s=1,v=1", b"AQIDBA==");
        let key = *gfx.image_order.back().unwrap();
        gfx.graphic(&mut term, b"a=d,d=N,I=7,p=5", b"");
        assert!(gfx.images.contains_key(&key));
        gfx.graphic(&mut term, b"a=p,I=7,p=4,c=1,r=1", b"");
        gfx.graphic(&mut term, b"a=d,d=N,I=7,p=5", b"");
        gfx.graphic(&mut term, b"a=d,d=n,I=7,p=4", b"");
        gfx.prune_dead();
        assert!(!gfx.has_placements(key) && gfx.images.contains_key(&key));
    }

    #[test]
    fn uppercase_range_deletion_also_releases_unplaced_resources() {
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        for id in 8..=10 {
            gfx.graphic(&mut term, format!("a=t,i={id},s=1,v=1").as_bytes(), b"AQIDBA==");
        }
        gfx.graphic(&mut term, b"a=d,d=R,x=9,y=8", b"");
        assert_eq!(gfx.images.len(), 3, "a reversed range must not delete another range");
        gfx.graphic(&mut term, b"a=d,d=R,x=8,y=9", b"");
        assert_eq!(gfx.images.keys().copied().collect::<Vec<_>>(), vec![10]);
        assert_eq!(gfx.image_bytes, 4);
    }

    #[test]
    fn transparent_rgb16_accounts_for_the_expanded_alpha_channel() {
        let mut header = png(4096, 2160);
        header[24] = 16;
        assert_eq!(KittyGraphics::png_size(&header), Ok((4096, 2160)));
        // Header-only fixtures exercise admission before full decoding on the worker.
        let chunks =
            [b"\0\0\0\x01tEXtX\0\0\0\0".as_slice(), b"\0\0\0\x06tRNS\0\0\0\0\0\0\0\0\0\0"].concat();
        header.splice(33..33, chunks);
        assert_eq!(KittyGraphics::png_size(&header), Err("E2BIG"));
        header[16..20].copy_from_slice(&2048_u32.to_be_bytes());
        assert_eq!(KittyGraphics::png_size(&header), Ok((2048, 2160)));
    }

    #[test]
    fn png_source_sample_cost_and_replacement_headers_are_checked() {
        use base64::Engine as _;
        let mut rgba16 = png(4096, 2160);
        rgba16[24] = 16;
        rgba16[25] = 6;
        assert_eq!(
            KittyGraphics::decode_png(&BASE64.encode(&rgba16).into_bytes()).unwrap_err(),
            "E2BIG"
        );
        let mut gfx = KittyGraphics::default();
        let mut term = terminal();
        let incomplete = encoded_png(2, 2);
        gfx.graphic(&mut term, b"a=t,f=100,i=5,m=1", &incomplete[..12]);
        assert!(gfx.current.is_some());
        gfx.graphic(&mut term, b"a=t,f=100,i=5", &encoded_png(3, 2));
        assert!(gfx.current.is_none());
        assert_eq!((gfx.images[&5].width, gfx.images[&5].height), (3, 2));
    }

    #[test]
    fn feature_queries_omit_defaults_and_fields_unused_by_the_requested_action() {
        for (query, expected) in [
            ("a=q,i=1,e=T,C=0,U=0,t=d,f=32,x=0,y=0,X=0,Y=0", "e=T"),
            ("a=q,i=1,e=t,C=1,c=20,r=10,f=100,d=i", "e=t,f=100"),
            ("a=q,i=1,e=p,C=1,f=100,x=1,d=i", "e=p,C=1,x=1"),
            ("a=q,i=1,e=d,C=1,f=100,d=p,x=2,y=3", "e=d,x=2,y=3,d=p"),
            ("a=q,i=1,e=d,d=n,x=2,y=3", "e=d,d=n"),
            ("a=q,i=1,e=p,p=9,Z=7,C=2", "e=p,p=9"),
            ("a=q,i=1,e=t,p=9", "e=t"),
        ] {
            let control = Control::parse(query.as_bytes()).unwrap();
            assert_eq!(control.features(control.queried_action().unwrap()), expected);
        }
        assert!(Control::parse(b"a=t,I=0,s=1,v=1").is_err());
        assert!(Control::parse(b"a=t,i=1,I=1,s=1,v=1").is_err());
    }

    #[test]
    fn delete_sources_never_reach_the_grid() {
        // Image identities reach cells off screen; only the selectors that name
        // visible cells may be filtered by them.
        for selector in [b'c', b'p', b'x', b'y'] {
            let mut gfx = KittyGraphics::default();
            let mut term = terminal();
            gfx.graphic(&mut term, b"a=T,i=7,s=1,v=1,c=1,r=1,C=1", b"AQIDBA==");
            gfx.graphic(&mut term, format!("a=d,d={},x=1,y=1", selector as char).as_bytes(), b"");
            assert!(!gfx.has_placements(7), "selector {:?}", selector as char);
        }
    }
}
