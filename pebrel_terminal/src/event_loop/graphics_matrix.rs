//! Protocol matrix: one assertion per wire behaviour, driven through the real parser.
//!
//! Every case here is a row of the compatibility table: raw and container
//! transfers, chunking, cropping, placement reuse, delete selectors, payload
//! tolerance, capability queries and the documented limits. A regression in any
//! of them names itself, instead of showing up as "some reader renders weird".
//!
//! Nothing here depends on a reader application or a screenshot: the checks read
//! the terminal's own events, replies and grid occupancy.

use super::*;
use crate::inline_image::{ImageFormat, ImagePlacement};
use crate::term::{Config, test::TermSize};

#[derive(Clone, Default)]
struct Gfx(std::sync::Arc<std::sync::Mutex<Vec<Event>>>);

impl Gfx {
    fn replies(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::PtyWrite(reply) => Some(reply.clone()),
                _ => None,
            })
            .collect()
    }

    fn images(&self) -> Vec<(ImagePlacement, u8)> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::KittyImage { channels, placement, .. } => {
                    Some((placement.clone(), *channels))
                },
                _ => None,
            })
            .collect()
    }
}

impl EventListener for Gfx {
    fn send_event(&self, event: Event) {
        if matches!(event, Event::KittyImage { .. } | Event::PtyWrite(_)) {
            self.0.lock().unwrap().push(event);
        }
    }
}

/// APC record as it travels on the wire.
fn command(control: &str, payload: &str) -> Vec<u8> {
    if payload.is_empty() {
        format!("\x1b_G{control}\x1b\\").into_bytes()
    } else {
        format!("\x1b_G{control};{payload}\x1b\\").into_bytes()
    }
}

fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Solid frame of `width * height` RGB pixels; cheap and easy to reason about.
fn rgb(width: u32, height: u32, colour: [u8; 3]) -> Vec<u8> {
    colour.iter().copied().cycle().take((width * height * 3) as usize).collect()
}

/// A 2x2 grey-square PNG, the smallest container a client can send.
fn png(width: u32, height: u32) -> Vec<u8> {
    fn chunk(tag: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(tag);
        out.extend_from_slice(body);
        let crc = crc32(tag, body);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.push(0u8);
        raw.extend(std::iter::repeat_n(0x80u8, (width * 3) as usize));
    }
    let mut header = width.to_be_bytes().to_vec();
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &header));
    out.extend(chunk(b"IDAT", &zlib(&raw)));
    out.extend(chunk(b"IEND", b""));
    out
}

/// Stored-deflate wrapper: enough zlib framing for an `IDAT` a decoder can inflate.
fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut remaining = raw;
    while !remaining.is_empty() {
        let take = remaining.len().min(0xFFFF);
        let last = if take == remaining.len() { 1 } else { 0 };
        out.push(last);
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!(take as u16)).to_le_bytes());
        out.extend_from_slice(&remaining[..take]);
        remaining = &remaining[take..];
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn crc32(tag: &[u8], body: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (index, entry) in table.iter_mut().enumerate() {
        let mut value = index as u32;
        for _ in 0..8 {
            value = if value & 1 != 0 { 0xEDB8_8320 ^ (value >> 1) } else { value >> 1 };
        }
        *entry = value;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for byte in tag.iter().chain(body.iter()) {
        crc = table[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(raw: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in raw {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn terminal() -> (Term<Gfx>, StreamProcessor, Gfx) {
    let listener = Gfx::default();
    let terminal = Term::new(Config::default(), &TermSize::new(40, 12), listener.clone());
    (terminal, StreamProcessor::default(), listener)
}

fn feed(stream: &mut StreamProcessor, terminal: &mut Term<Gfx>, listener: &Gfx, bytes: &[u8]) {
    stream.feed(terminal, listener, bytes);
}

fn cell_image(terminal: &Term<Gfx>, row: i32, column: usize) -> bool {
    terminal.grid()[Line(row)][Column(column)].image().is_some()
}

/// The cell-level placement: this is where the crop and the owned cell area live.
fn image_cells(terminal: &Term<Gfx>) -> usize {
    let mut count = 0;
    for row in 0..terminal.screen_lines() {
        for column in 0..terminal.columns() {
            if terminal.grid()[Line(row as i32)][Column(column)].image().is_some() {
                count += 1;
            }
        }
    }
    count
}

fn placement_at(
    terminal: &Term<Gfx>,
    row: i32,
    column: usize,
) -> std::sync::Arc<crate::inline_image::KittyPlacement> {
    terminal.grid()[Line(row)][Column(column)]
        .image()
        .and_then(|cell| cell.placement.clone())
        .expect("the cell carries a placement")
}

// --- transfers -------------------------------------------------------------

#[test]
fn matrix_raw_rgb_placement_reports_its_source_and_cells() {
    let (mut terminal, mut stream, listener) = terminal();
    let payload = base64(&rgb(60, 40, [220, 40, 40]));
    feed(
        &mut stream,
        &mut terminal,
        &listener,
        &command("a=T,f=24,s=60,v=40,i=11,c=20,r=10,C=1", &payload),
    );

    let images = listener.images();
    assert_eq!(images.len(), 1, "one publication per placement");
    let (placement, channels) = &images[0];
    assert_eq!(*channels, 3);
    assert_eq!(placement.format, ImageFormat::Raw);
    assert_eq!((placement.layout.width, placement.layout.height), (60.0, 40.0));
    assert_eq!((placement.layout.columns, placement.layout.rows), (20, 10));
    assert!(cell_image(&terminal, 0, 0), "the placement owns its first cell");
}

#[test]
fn matrix_rgba_transfer_reports_four_channels() {
    let (mut terminal, mut stream, listener) = terminal();
    let payload = base64(&vec![9u8; 4 * 4 * 4]);
    feed(
        &mut stream,
        &mut terminal,
        &listener,
        &command("a=T,f=32,s=4,v=4,i=12,c=2,r=2,C=1", &payload),
    );
    let images = listener.images();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].1, 4);
}

#[test]
fn matrix_png_container_is_read_from_its_own_header() {
    let (mut terminal, mut stream, listener) = terminal();
    let payload = base64(&png(6, 3));
    feed(&mut stream, &mut terminal, &listener, &command("a=T,f=100,i=13,c=6,r=3,C=1", &payload));
    let images = listener.images();
    assert_eq!(images.len(), 1, "a container arrives without s/v");
    let (placement, _) = &images[0];
    assert_eq!(placement.format, ImageFormat::Png);
    assert_eq!((placement.layout.width, placement.layout.height), (6.0, 3.0));
}

#[test]
fn matrix_payload_above_one_chunk_is_joined() {
    let (mut terminal, mut stream, listener) = terminal();
    let payload = base64(&rgb(80, 40, [40, 120, 220]));
    let pieces: Vec<&str> =
        payload.as_bytes().chunks(4096).map(|piece| std::str::from_utf8(piece).unwrap()).collect();
    assert!(pieces.len() > 1, "the payload must span several chunks");
    for (index, piece) in pieces.iter().enumerate() {
        let more = usize::from(index + 1 < pieces.len());
        let control = if index == 0 {
            format!("a=T,f=24,s=80,v=40,i=14,c=20,r=10,C=1,m={more}")
        } else {
            format!("m={more}")
        };
        feed(&mut stream, &mut terminal, &listener, &command(&control, piece));
    }
    let images = listener.images();
    assert_eq!(images.len(), 1, "chunks belong to one image");
    assert_eq!(images[0].0.layout.width, 80.0);
}

#[test]
fn matrix_crop_keeps_the_requested_window() {
    let (mut terminal, mut stream, listener) = terminal();
    let payload = base64(&rgb(60, 80, [10, 200, 10]));
    feed(
        &mut stream,
        &mut terminal,
        &listener,
        &command("a=T,f=24,s=60,v=80,i=15,c=10,r=5,C=1,x=10,y=20,w=30,h=40", &payload),
    );
    assert_eq!(listener.images().len(), 1);
    let placement = placement_at(&terminal, 0, 0);
    assert_eq!((placement.source_x, placement.source_y), (10.0, 20.0));
    assert_eq!((placement.source_width, placement.source_height), (30.0, 40.0));
}

#[test]
fn matrix_png_query_checks_the_container_without_retaining_an_image() {
    let (mut terminal, mut stream, listener) = terminal();
    let payload = base64(&png(2, 2));
    feed(&mut stream, &mut terminal, &listener, &command("a=q,i=11,f=100", &payload));
    assert!(listener.replies().iter().any(|reply| reply == "\x1b_Gi=11;OK\x1b\\"));
    assert!(listener.images().is_empty());
    assert_eq!(image_cells(&terminal), 0);
}

// --- payload tolerance -----------------------------------------------------

#[test]
fn matrix_unpadded_base64_is_accepted() {
    let (mut terminal, mut stream, listener) = terminal();
    // Four bytes are canonical 'AQIDBA=='; strip the padding a lenient encoder drops.
    feed(
        &mut stream,
        &mut terminal,
        &listener,
        &command("a=T,f=32,s=1,v=1,i=30,c=2,r=1,C=1", "AQIDBA"),
    );
    assert_eq!(listener.images().len(), 1);
    assert!(!listener.replies().iter().any(|reply| reply.contains("EINVAL")));
}

#[test]
fn matrix_payload_with_line_breaks_is_accepted() {
    let (mut terminal, mut stream, listener) = terminal();
    let wrapped = base64(&rgb(8, 4, [50, 200, 50]))
        .as_bytes()
        .chunks(37)
        .map(|piece| std::str::from_utf8(piece).unwrap().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    feed(
        &mut stream,
        &mut terminal,
        &listener,
        &command("a=T,f=24,s=8,v=4,i=31,c=4,r=2,C=1", &wrapped),
    );
    assert_eq!(listener.images().len(), 1, "whitespace inside the payload is stripped, not fatal");
}

#[test]
fn matrix_pixel_budget_is_enforced() {
    let (mut terminal, mut stream, listener) = terminal();
    feed(&mut stream, &mut terminal, &listener, &command("a=t,f=24,s=5000,v=5000,i=32", "AAAA"));
    assert!(listener.replies().iter().any(|reply| reply.contains("E2BIG")));
}
