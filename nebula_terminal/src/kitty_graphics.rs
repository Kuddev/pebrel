//! Kitty protocol (`ESC _ G … ESC \`).
//!
//! Two placement dialects reach terminals in the wild. A client that trusts the
//! terminal to do the geometry sends one `c`×`r` block; a client that cannot
//! read the cell size — yazi's `kgp_old` driver is the one that matters here —
//! moves the cursor to every single cell and sends a one-cell placement whose
//! `x`/`y`/`w`/`h` slice the bitmap. Both arrive as a run of `a=p` commands, so
//! the state machine keeps only the cursor: each placement writes its tile into
//! the cell the cursor currently occupies, and the union of those cells is the
//! image box. Nothing is derived from the application's pixel numbers, which is
//! what used to overflow the pane.
//!
//! Transfers are the expensive half: a 2.2 MiB bitmap arrives as ~730 APC
//! chunks. Nothing here may touch the disk, publish an event per chunk or per
//! placement, or re-encode the bitmap. A parsed placement is published once,
//! after the grid already holds its cells, so the frontend decodes off the UI
//! thread while the cells paint as ordinary grid content.
//!
//! Deletions matter for the same reason they are tedious: yazi reuses one
//! protocol image id for every preview and erases the previous image cell by cell.
//! Erasing once per id — blanking the cells that carry that identity — keeps a
//! scrolling file manager from accumulating dead bitmaps.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use base64::Engine as _;

use crate::event::{Event, EventListener, WindowSize};
use crate::inline_image::{ImageIdentity, ImageLayout, ImagePlacement};
use crate::term::Term;

/// Pixels one transfer may allocate (16 MiB at four bytes per pixel).
const MAX_PIXELS: usize = 16 * 1024 * 1024;
/// Base64 text held while a chunked transfer is still incomplete.
const MAX_ENCODED_BYTES: usize = 24 * 1024 * 1024;
/// `CSI 16 t` — the cell-size query, which vte drops as unhandled.
const CELL_SIZE_QUERY: &[u8] = b"\x1b[16t";

/// One decoded transfer.
struct Bitmap {
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    channels: u8,
}

/// Cells covered by the placements of one image, in absolute cursor
/// coordinates (`bottom`/`right` exclusive).
#[derive(Clone, Copy, PartialEq, Eq)]
struct GfxBox {
    top: usize,
    left: usize,
    bottom: usize,
    right: usize,
}

impl GfxBox {
    fn at(line: usize, column: usize, columns: usize, rows: usize) -> Self {
        Self { top: line, left: column, bottom: line + rows, right: column + columns }
    }

    fn union(&mut self, other: Self) {
        self.top = self.top.min(other.top);
        self.left = self.left.min(other.left);
        self.bottom = self.bottom.max(other.bottom);
        self.right = self.right.max(other.right);
    }

    fn columns(self) -> usize {
        self.right.saturating_sub(self.left)
    }

    fn rows(self) -> usize {
        self.bottom.saturating_sub(self.top)
    }
}

/// Kitty protocol state for one byte stream.
#[derive(Default)]
pub struct KittyGraphics {
    /// Protocol id of the transfer currently receiving chunks.
    current: Option<u32>,
    /// Partial base64 of a chunked transfer.
    transfers: HashMap<u32, Vec<u8>>,
    /// Pixel geometry declared by the transfer header: width, height, channels.
    sizes: HashMap<u32, (u32, u32, u8)>,
    images: HashMap<u32, Bitmap>,
    /// Grid-owned identity per protocol id: every placement of one image shares it,
    /// so erasing the image is a property of the grid, not of this state.
    identities: HashMap<u32, Arc<ImageIdentity>>,
    /// Image origin in absolute cells. Tile coordinates are stored relative to
    /// it, which is the frame the frontend samples the bitmap in.
    origins: HashMap<u32, (usize, usize)>,
    boxes: HashMap<u32, GfxBox>,
    /// Images whose placement already reached the frontend.
    published: HashSet<u32>,
    /// Unanswered `CSI 16 t` requests, coalesced: one answer serves them all.
    cell_size_queries: u32,
    /// Trailing bytes that may be the start of a query split across reads.
    query_tail: Vec<u8>,
}

impl KittyGraphics {
    /// Handle one Kitty protocol command. `control` is the raw `key=value`
    /// block, `payload` the raw base64 chunk.
    pub fn graphic<U: EventListener>(
        &mut self,
        terminal: &mut Term<U>,
        event_proxy: &U,
        control: &[u8],
        payload: &[u8],
    ) {
        let action = control_value(control, b'a').and_then(|value| value.first().copied());
        let id = control_value(control, b'i').and_then(parse_u32);
        match action {
            // A continuation chunk carries only `m`; the id is the one the
            // transfer started with.
            None => {
                let Some(id) = self.current else { return };
                // The continuation chunk carries its own `m`: only the last one
                // completes the transfer.
                let more = control_value(control, b'm') == Some(b"1");
                self.receive(id, payload, more);
            },
            Some(b't' | b'T') => {
                let Some(id) = id.or(self.current) else { return };
                self.current = Some(id);
                let more = control_value(control, b'm') == Some(b"1");
                // Only the header chunk carries `s`/`v`; a client that reuses the
                // id for the next picture relies on that to reset the transfer.
                // Without this, the previous picture's geometry decodes the new
                // pixels and the frame comes out as diagonal stripes.
                let starts = control_value(control, b's').is_some()
                    || control_value(control, b'v').is_some();
                if starts {
                    self.transfers.insert(id, Vec::new());
                    self.published.remove(&id);
                    self.declare(id, control);
                } else if !self.sizes.contains_key(&id) {
                    self.declare(id, control);
                }
                self.receive(id, payload, more);
                if !more && action == Some(b'T') {
                    let columns =
                        control_value(control, b'c').and_then(parse_usize).unwrap_or(1).max(1);
                    let rows =
                        control_value(control, b'r').and_then(parse_usize).unwrap_or(1).max(1);
                    self.place(terminal, id, columns, rows);
                }
            },
            Some(b'p') => {
                let Some(id) = id else { return };
                let columns =
                    control_value(control, b'c').and_then(parse_usize).unwrap_or(1).max(1);
                let rows = control_value(control, b'r').and_then(parse_usize).unwrap_or(1).max(1);
                self.place(terminal, id, columns, rows);
            },
            Some(b'd') => {
                let Some(id) = id else {
                    // `d=a` erases every image; ids are optional there.
                    self.clear_all(terminal);
                    return;
                };
                self.delete(terminal, id);
            },
            Some(b'q') => self.reply(event_proxy, control, id, surface_supported(control)),
            _ => (),
        }
    }

    /// Publish placements that reached the grid since the previous flush. One
    /// event per image: the frontend decodes once and paints the cells.
    pub fn flush<U: EventListener>(&mut self, event_proxy: &U, window_size: Option<WindowSize>) {
        let pending: Vec<u32> = self
            .boxes
            .keys()
            .copied()
            .filter(|id| !self.published.contains(id) && self.images.contains_key(id))
            .collect();
        for id in pending {
            let (Some(cells), Some(bitmap), Some(identity)) =
                (self.boxes.get(&id).copied(), self.images.get(&id), self.identities.get(&id))
            else {
                continue;
            };
            if cells.columns() == 0 || cells.rows() == 0 {
                continue;
            }
            let (cell_width, cell_height) = match window_size {
                Some(size) => {
                    (f32::from(size.cell_width.max(1)), f32::from(size.cell_height.max(1)))
                },
                // Until the pane reports its geometry, size the bitmap against
                // its own box: the application already chose that many cells.
                None => (
                    bitmap.width as f32 / cells.columns() as f32,
                    bitmap.height as f32 / cells.rows() as f32,
                ),
            };
            let layout = ImageLayout {
                width: bitmap.width as f32,
                height: bitmap.height as f32,
                cell_width,
                cell_height,
                columns: cells.columns(),
                rows: cells.rows(),
            };
            let placement =
                ImagePlacement { id: identity.id, lifetime: Arc::downgrade(identity), layout };
            self.published.insert(id);
            event_proxy.send_event(Event::KittyImage {
                data: bitmap.pixels.clone(),
                channels: bitmap.channels,
                placement,
            });
        }
    }

    /// Count `CSI 16 t` requests in the raw stream. The parser cannot see them
    /// (vte 0.15 ignores the sequence), and a client that cannot read the cell
    /// size falls back to stretching a preview across the whole pane.
    pub fn scan_cell_size_queries(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut probe = std::mem::take(&mut self.query_tail);
        probe.extend_from_slice(bytes);
        let len = CELL_SIZE_QUERY.len();
        if probe.len() >= len {
            let count = probe.windows(len).filter(|window| *window == CELL_SIZE_QUERY).count();
            self.cell_size_queries =
                self.cell_size_queries.saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
        }
        // Retain the longest suffix that could still become a split query.
        let keep = (len - 1).min(probe.len());
        let tail = &probe[probe.len() - keep..];
        if keep > 0 && CELL_SIZE_QUERY.starts_with(tail) {
            self.query_tail = tail.to_vec();
        }
    }

    /// Answer pending `CSI 16 t` requests with the real cell size. The reply is
    /// `CSI 6 ; height ; width t` — height first, as xterm defines it.
    pub fn answer_cell_size_queries<U: EventListener>(
        &mut self,
        window_size: Option<WindowSize>,
        event_proxy: &U,
    ) {
        let Some(size) = window_size else { return };
        if self.cell_size_queries == 0 {
            return;
        }
        self.cell_size_queries = 0;
        event_proxy.send_event(Event::PtyWrite(format!(
            "\x1b[6;{};{}t",
            size.cell_height.max(1),
            size.cell_width.max(1)
        )));
    }

    /// Record the pixel geometry declared by a transfer header.
    fn declare(&mut self, id: u32, control: &[u8]) {
        let width = control_value(control, b's').and_then(parse_u32);
        let height = control_value(control, b'v').and_then(parse_u32);
        let channels = match control_value(control, b'f').and_then(parse_u32) {
            // 24 = RGB, 32 = RGBA. Palette and PNG payloads would need the
            // frontend's image decoder, so they are refused, not guessed.
            Some(24) => Some(3),
            Some(32) => Some(4),
            _ => None,
        };
        match (width, height, channels) {
            (Some(width), Some(height), Some(channels)) => {
                self.sizes.insert(id, (width, height, channels));
            },
            _ => {
                self.sizes.remove(&id);
            },
        }
    }

    fn receive(&mut self, id: u32, payload: &[u8], more: bool) {
        if more {
            let encoded = self.transfers.entry(id).or_default();
            if encoded.len() + payload.len() > MAX_ENCODED_BYTES {
                encoded.clear();
                return;
            }
            encoded.extend_from_slice(payload);
            return;
        }
        let mut joined = self.transfers.remove(&id).unwrap_or_default();
        joined.extend_from_slice(payload);
        let Ok(pixels) = base64::engine::general_purpose::STANDARD.decode(&joined) else {
            return;
        };
        let Some(&(width, height, channels)) = self.sizes.get(&id) else { return };
        if width == 0 || height == 0 {
            return;
        }
        let needed = width as usize * height as usize * usize::from(channels);
        if needed > MAX_PIXELS * 4 || pixels.len() < needed {
            return;
        }
        self.images.insert(id, Bitmap { pixels: Arc::new(pixels), width, height, channels });
        self.published.remove(&id);
    }

    fn place<U: EventListener>(
        &mut self,
        terminal: &mut Term<U>,
        id: u32,
        columns: usize,
        rows: usize,
    ) {
        // A placement without a completed transfer has nothing to paint; the
        // application will retransmit, so dropping it is safe.
        if !self.images.contains_key(&id) {
            return;
        }
        let line = terminal.nebula_cursor_abs_line();
        let column = terminal.grid().cursor.point.column.0;
        let (origin_line, origin_column) = *self.origins.entry(id).or_insert((line, column));
        let identity = self.identities.entry(id).or_insert_with(ImageIdentity::new).clone();
        terminal.nebula_insert_kitty_block(
            &identity,
            column.saturating_sub(origin_column),
            line.saturating_sub(origin_line),
            columns,
            rows,
        );
        let cells = GfxBox::at(line, column, columns, rows);
        self.boxes.entry(id).and_modify(|existing| existing.union(cells)).or_insert(cells);
    }

    fn delete<U: EventListener>(&mut self, terminal: &mut Term<U>, id: u32) {
        if let Some(identity) = self.identities.remove(&id) {
            terminal.nebula_clear_image_cells(identity.id);
        }
        self.origins.remove(&id);
        self.boxes.remove(&id);
        self.published.remove(&id);
        self.transfers.remove(&id);
        // The pixels are gone with the placement: keeping them would let the
        // next transfer with the same id inherit a stale geometry.
        self.sizes.remove(&id);
        self.images.remove(&id);
        if self.current == Some(id) {
            self.current = None;
        }
    }

    fn clear_all<U: EventListener>(&mut self, terminal: &mut Term<U>) {
        let ids: Vec<u32> = self.identities.keys().copied().collect();
        for id in ids {
            self.delete(terminal, id);
        }
        self.images.clear();
    }

    fn reply<U: EventListener>(
        &self,
        event_proxy: &U,
        control: &[u8],
        id: Option<u32>,
        supported: bool,
    ) {
        // `q=2` asks for silence; every other query wants an answer so the
        // client can pick its transport.
        if control_value(control, b'q') == Some(b"2") {
            return;
        }
        let Some(id) = id else { return };
        let status = if supported { "OK" } else { "ENOTSUP" };
        event_proxy.send_event(Event::PtyWrite(format!("\x1b_Gi={id};{status}\x1b\\")));
    }
}

/// Whether the queried transport can serve this terminal: base64 transfers are
/// the only kind implemented, shared memory is not.
fn surface_supported(control: &[u8]) -> bool {
    control_value(control, b't') != Some(b"s")
}

/// Value of a single-letter key in a Kitty protocol control block.
fn control_value<'a>(control: &'a [u8], key: u8) -> Option<&'a [u8]> {
    control.split(|&b| b == b',' || b == b';').find_map(|pair| {
        (pair.first() == Some(&key) && pair.get(1) == Some(&b'=')).then(|| &pair[2..])
    })
}

fn parse_u32(value: &[u8]) -> Option<u32> {
    std::str::from_utf8(value).ok()?.trim().parse().ok()
}

fn parse_usize(value: &[u8]) -> Option<usize> {
    std::str::from_utf8(value).ok()?.trim().parse().ok()
}
