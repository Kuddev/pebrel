//! Bounded decode/cache state for terminal image protocols.
//!
//! Encoded bytes arrive through the PTY, but decoding is serialized on the
//! background executor. This prevents a burst of compressed image bombs from
//! expanding concurrently on the UI thread or exhausting process memory.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Weak};

use gpui::{Bounds, ContentMask, Corners, Pixels, RenderImage, Window, point, px, size};
use image::{Frame, ImageFormat};
use nebula_terminal::inline_image::{ImageIdentity, ImageLayout, ImagePlacement, ImageRun};

const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_ENCODED_BYTES: usize = 12 * 1024 * 1024;
const MAX_IMAGE_DECODED_BYTES: usize = 64 * 1024 * 1024;
// A scanned page decodes to a few tens of megabytes of BGRA, and a reader
// keeps several pages in flight (a pager uploads the pages around the cursor
// up front). Keeping only ten pages' worth evicted the page that was on screen
// as soon as the reader paged ahead, which showed up as a page that never came
// back until the reader changed what it placed.
const MAX_CACHED_IMAGES: usize = 32;
const MAX_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_QUEUED_IMAGES: usize = 16;
const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;

pub(super) struct PendingInlineImage {
    sequence: u64,
    data: Arc<Vec<u8>>,
    placement: ImagePlacement,
    /// Raw pixel channels when the transfer carried pixels rather than an
    /// encoded container: 3 = RGB, 4 = RGBA. Kitty protocol sends raw pixels,
    /// so re-guessing a container format there would reject every image.
    channels: Option<u8>,
}

#[derive(Clone)]
pub(super) struct InlineImage {
    pub image: Arc<RenderImage>,
    pub placement: ImagePlacement,
    decoded_bytes: usize,
}

#[derive(Default)]
pub(super) struct InlineImageStore {
    queued: VecDeque<PendingInlineImage>,
    queued_bytes: usize,
    decoding: bool,
    next_sequence: u64,
    images: VecDeque<(u64, InlineImage)>,
    decoded_bytes: usize,
    /// Pixel buffer behind the placement of each live image. A placement that
    /// only moved its crop carries the same buffer, so the decoded texture is
    /// reused instead of decoded again on every scroll step.
    sources: HashMap<u64, (Weak<ImageIdentity>, Arc<Vec<u8>>)>,
}

impl InlineImageStore {
    pub fn enqueue(
        &mut self,
        data: Arc<Vec<u8>>,
        placement: ImagePlacement,
    ) -> Result<(), &'static str> {
        if data.len() > MAX_IMAGE_ENCODED_BYTES {
            return Err("encoded terminal image exceeds 12 MiB");
        }
        if !placement.layout.width.is_finite()
            || !placement.layout.height.is_finite()
            || placement.layout.width <= 0.0
            || placement.layout.height <= 0.0
        {
            return Err("terminal image has an invalid display size");
        }
        if !placement.is_alive() {
            return Ok(());
        }
        if self.queued.len() >= MAX_QUEUED_IMAGES {
            return Err("too many terminal images are waiting to decode");
        }
        let next_bytes = self
            .queued_bytes
            .checked_add(data.len())
            .ok_or("terminal image queue size overflow")?;
        if next_bytes > MAX_QUEUED_BYTES {
            return Err("terminal image decode queue exceeds 32 MiB");
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.queued.push_back(PendingInlineImage { sequence, data, placement, channels: None });
        self.queued_bytes = next_bytes;
        Ok(())
    }

    /// Queue a Kitty protocol transfer, whose payload is raw pixels rather than
    /// an encoded container. `channels` is the byte count per pixel (3 = RGB,
    /// 4 = RGBA); the pixel geometry rides along in `placement.layout`.
    pub fn enqueue_pixels(
        &mut self,
        data: Arc<Vec<u8>>,
        channels: u8,
        placement: ImagePlacement,
    ) -> Result<(), &'static str> {
        if !matches!(channels, 3 | 4) {
            return Err("image transfer has an unsupported channel count");
        }

        if data.len() > MAX_IMAGE_DECODED_BYTES {
            return Err("image transfer exceeds the decoded image budget");
        }
        if !placement.layout.width.is_finite()
            || !placement.layout.height.is_finite()
            || placement.layout.width <= 0.0
            || placement.layout.height <= 0.0
        {
            return Err("image transfer has an invalid display size");
        }
        let pixels = placement.layout.width as u64 * placement.layout.height as u64;
        if pixels > MAX_IMAGE_PIXELS {
            return Err("image transfer exceeds the pixel budget");
        }
        if (pixels * u64::from(channels)) > data.len() as u64 {
            return Err("image transfer is shorter than its declared size");
        }
        if !placement.is_alive() {
            return Ok(());
        }
        // Re-placing a transfer the store already holds — a pager scrolling
        // through a page sends a new crop of the same bitmap — must not decode
        // the pixels a second time. Sharing the buffer is what identifies it:
        // the terminal hands out the same allocation every time it republishes
        // one bitmap, and a fresh one for a retransfer.
        let reuse =
            self.sources.get(&placement.id).map_or(false, |(_, source)| Arc::ptr_eq(source, &data));
        if reuse {
            if let Some(pending) =
                self.queued.iter_mut().find(|pending| pending.placement.id == placement.id)
            {
                pending.placement = placement;
                return Ok(());
            }
            if let Some((_, image)) =
                self.images.iter_mut().find(|(_, image)| image.placement.id == placement.id)
            {
                image.placement = placement;
                return Ok(());
            }
        }
        self.sources.insert(placement.id, (placement.lifetime.clone(), data.clone()));
        if self.queued.len() >= MAX_QUEUED_IMAGES {
            return Err("too many terminal images are waiting to decode");
        }
        let next_bytes = self
            .queued_bytes
            .checked_add(data.len())
            .ok_or("terminal image queue size overflow")?;
        if next_bytes > MAX_QUEUED_BYTES {
            return Err("terminal image decode queue exceeds 32 MiB");
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.queued.push_back(PendingInlineImage {
            sequence,
            data,
            placement,
            channels: Some(channels),
        });
        self.queued_bytes = next_bytes;
        Ok(())
    }

    pub fn start_next(&mut self) -> Option<PendingInlineImage> {
        if self.decoding {
            return None;
        }
        while let Some(pending) = self.queued.pop_front() {
            self.queued_bytes = self.queued_bytes.saturating_sub(pending.data.len());
            if pending.placement.is_alive() {
                self.decoding = true;
                return Some(pending);
            }
        }
        None
    }

    pub fn finish(&mut self, result: Result<(u64, InlineImage), String>) -> Result<(), String> {
        self.decoding = false;
        let (sequence, image) = result?;
        if !image.placement.is_alive() {
            return Ok(());
        }
        if image.decoded_bytes > MAX_IMAGE_DECODED_BYTES {
            return Err("decoded terminal image exceeds 64 MiB".to_owned());
        }

        while self.images.len() >= MAX_CACHED_IMAGES
            || self.decoded_bytes.saturating_add(image.decoded_bytes) > MAX_CACHE_BYTES
        {
            let Some((_, evicted)) = self.images.pop_front() else { break };
            self.decoded_bytes = self.decoded_bytes.saturating_sub(evicted.decoded_bytes);
        }
        self.decoded_bytes += image.decoded_bytes;
        self.images.push_back((sequence, image));
        Ok(())
    }

    /// Grid erasure invalidates both queued jobs and decoded cache entries.
    pub fn frame_images(&mut self) -> Vec<InlineImage> {
        self.images.retain(|(_, image)| image.placement.is_alive());
        self.sources.retain(|_, (lifetime, _)| lifetime.strong_count() != 0);
        self.decoded_bytes = self.images.iter().map(|(_, image)| image.decoded_bytes).sum();
        self.images.iter().map(|(_, image)| image.clone()).collect()
    }
}

pub(super) fn decode(pending: PendingInlineImage) -> Result<(u64, InlineImage), String> {
    let (render_image, decoded_bytes) = match pending.channels {
        Some(channels) => decode_pixels(&pending.data, channels, pending.placement.layout)?,
        None => decode_bytes(pending.data.as_slice())?,
    };
    Ok((
        pending.sequence,
        InlineImage { image: render_image, placement: pending.placement, decoded_bytes },
    ))
}

/// Turn a image transfer into a texture. The protocol transfers pixels in RGB or
/// RGBA order, so nothing is guessed from the bytes; only the channel swap GPUI
/// expects is applied.
fn decode_pixels(
    data: &[u8],
    channels: u8,
    layout: ImageLayout,
) -> Result<(Arc<RenderImage>, usize), String> {
    let width = layout.width.round() as usize;
    let height = layout.height.round() as usize;
    let Some(pixels) = width.checked_mul(height) else {
        return Err("image transfer has an impossible size".to_owned());
    };
    if width == 0 || height == 0 || pixels as u64 > MAX_IMAGE_PIXELS {
        return Err("image transfer has an impossible size".to_owned());
    }
    let stride = usize::from(channels);
    let Some(needed) = pixels.checked_mul(stride) else {
        return Err("image transfer has an impossible size".to_owned());
    };
    let Some(source) = data.get(..needed) else {
        return Err("image transfer is shorter than its declared size".to_owned());
    };

    let bgra = pixels_to_bgra(source, channels, pixels)?;
    let decoded_bytes = bgra.len();
    let Some(buffer) = image::RgbaImage::from_raw(width as u32, height as u32, bgra) else {
        return Err("image transfer has an impossible size".to_owned());
    };
    Ok((Arc::new(RenderImage::new([Frame::new(buffer)])), decoded_bytes))
}

/// Repack `pixels` worth of RGB/RGBA source bytes into the BGRA order GPUI
/// samples. Transparent-capable transfers keep their alpha; RGB ones become
/// opaque. Kept separate from texture creation so the byte layout is testable
/// without a window.
fn pixels_to_bgra(source: &[u8], channels: u8, pixels: usize) -> Result<Vec<u8>, String> {
    let stride = usize::from(channels);
    let Some(needed) = pixels.checked_mul(stride) else {
        return Err("image transfer has an impossible size".to_owned());
    };
    let Some(source) = source.get(..needed) else {
        return Err("image transfer is shorter than its declared size".to_owned());
    };

    let mut bgra = vec![0u8; pixels * 4];
    for (index, pixel) in source.chunks_exact(stride).enumerate() {
        let target = &mut bgra[index * 4..(index + 1) * 4];
        target[0] = pixel[2];
        target[1] = pixel[1];
        target[2] = pixel[0];
        target[3] = if stride == 4 { pixel[3] } else { 0xff };
    }
    Ok(bgra)
}

/// Paint only cells present in this frame's grid snapshot. Sampling always
/// uses the full image bounds, so erased/reflowed fragments are never stretched.
pub(super) fn paint(
    images: &[InlineImage],
    runs: &[ImageRun],
    bounds: Bounds<Pixels>,
    cell_width: Pixels,
    cell_height: Pixels,
    row_y: &dyn Fn(f32) -> Pixels,
    window: &mut Window,
) {
    let mut painted = 0usize;
    let mut empty = 0usize;
    let mut unmatched = 0usize;
    let mut first_painted: Option<String> = None;
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        for run in runs {
            let Some(inline) = images.iter().find(|image| image.placement.id == run.id) else {
                unmatched += 1;
                continue;
            };
            let (image_bounds, clip) = image_geometry(
                inline.placement.layout,
                run,
                bounds.origin,
                cell_width,
                cell_height,
                row_y,
            );
            if clip.size.width <= px(0.0) || clip.size.height <= px(0.0) {
                empty += 1;
                continue;
            }
            painted += 1;
            if first_painted.is_none() {
                first_painted = Some(format!(
                    "id={} row={} col={} src_row={} src_col={} texture={:?} cell=({},{}) layout=({}x{} cell {}x{}) img=({:?},{:?}) clip=({:?},{:?})",
                    run.id,
                    run.row,
                    run.column,
                    run.source_row,
                    run.source_column,
                    inline.image.size(0),
                    cell_width,
                    cell_height,
                    inline.placement.layout.width,
                    inline.placement.layout.height,
                    inline.placement.layout.cell_width,
                    inline.placement.layout.cell_height,
                    image_bounds.origin,
                    image_bounds.size,
                    clip.origin,
                    clip.size,
                ));
            }
            // Keep full texture coordinates: GPUI rounds sub-image UVs to
            // source pixels, which loses rows when a tiny image is enlarged.
            window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                let _ = window.paint_image(
                    image_bounds,
                    image_bounds,
                    Corners::all(px(0.0)),
                    inline.image.clone(),
                    0,
                    false,
                );
            });
        }
    });
    log_paint_summary(
        runs.len(),
        images.len(),
        painted,
        empty,
        unmatched,
        first_painted.as_deref(),
    );
}

/// A frame with no image runs is the common case, so only a change in what the
/// graphics path did is worth a line in the debug log.
fn log_paint_summary(
    runs: usize,
    images: usize,
    painted: usize,
    empty: usize,
    unmatched: usize,
    first: Option<&str>,
) {
    use std::sync::{Mutex, OnceLock};

    if runs == 0 && images == 0 {
        return;
    }
    static LAST: OnceLock<Mutex<String>> = OnceLock::new();
    let summary = format!(
        "gfx paint runs={runs} images={images} painted={painted} empty={empty} unmatched={unmatched} first={}",
        first.unwrap_or("-")
    );
    let seen = LAST.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut last) = seen.lock()
        && *last != summary
    {
        *last = summary.clone();
        crate::logging::debug_log(summary);
    }
}

pub(super) fn image_geometry(
    layout: ImageLayout,
    run: &ImageRun,
    origin: gpui::Point<Pixels>,
    cell_width: Pixels,
    cell_height: Pixels,
    row_y: &dyn Fn(f32) -> Pixels,
) -> (Bounds<Pixels>, Bounds<Pixels>) {
    let tile_origin =
        point(origin.x + cell_width * run.column as f32, origin.y + row_y(run.row as f32));
    // A placement owns exactly the cells it declared (`c`/`r`), and the source
    // region it asked for fills that box: scaling the bitmap by its own pixels
    // instead lets a sender with different cell metrics (a reader computing
    // 23px cells while the pane reports 14) paint a page several cells wider
    // than the block it asked for, and a sender that crops to the part of a
    // page it wants visible (a pager, one `y`/`h` per scroll step) ends up with
    // a slice occupying a fraction of the box — which shows as a blank page.
    let box_size = size(cell_width * layout.columns as f32, cell_height * layout.rows as f32);
    let source_width = layout.source.width.max(1) as f32;
    let source_height = layout.source.height.max(1) as f32;
    // A scaled bitmap (a high-DPI pane paints it smaller than its pixel size)
    // must shift by the scaled offset, or the slice lands outside the tile and
    // only its last few pixels survive the clip.
    let scale_x = box_size.width / source_width;
    let scale_y = box_size.height / source_height;
    let image_bounds = Bounds::new(
        point(
            tile_origin.x
                - (scale_x * layout.source.x as f32 + cell_width * run.source_column as f32),
            tile_origin.y
                - (scale_y * layout.source.y as f32 + cell_height * run.source_row as f32),
        ),
        size(layout.width * scale_x, layout.height * scale_y),
    );
    // The slice is anchored to the placement's own origin, not to the row being
    // painted: a run covers one row of cells, so anchoring it per run would let
    // every row show another row's worth of the picture below the slice.
    let slice = Bounds::new(
        point(
            image_bounds.origin.x + scale_x * layout.source.x as f32,
            image_bounds.origin.y + scale_y * layout.source.y as f32,
        ),
        box_size,
    );
    let clip = Bounds::new(tile_origin, size(cell_width * run.columns as f32, cell_height))
        .intersect(&slice);
    (image_bounds, clip)
}

pub(super) fn decode_bytes(data: &[u8]) -> Result<(Arc<RenderImage>, usize), String> {
    let format = image::guess_format(data)
        .map_err(|error| format!("unsupported terminal image: {error}"))?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif) {
        return Err(format!("unsupported terminal image format: {format:?}"));
    }

    let mut bgra = decode_rgba(data, format, MAX_IMAGE_ENCODED_BYTES)?;
    let decoded_bytes = bgra.len();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let render_image = Arc::new(RenderImage::new([Frame::new(bgra)]));
    Ok((render_image, decoded_bytes))
}

/// Clipboard bitmaps may be uncompressed; normalization shares the terminal
/// decoder's pixel/allocation limits while keeping its protocol formats intact.
pub(super) fn clipboard_png(data: &[u8]) -> Result<Vec<u8>, String> {
    let format = image::guess_format(data)
        .map_err(|error| format!("unsupported clipboard image: {error}"))?;
    if !matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::WebP
    ) {
        return Err(format!("unsupported clipboard image format: {format:?}"));
    }
    let rgba = decode_rgba(data, format, MAX_IMAGE_DECODED_BYTES)?;
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|error| format!("could not encode clipboard image: {error}"))?;
    let png = output.into_inner();
    if png.len() > MAX_IMAGE_ENCODED_BYTES {
        return Err("clipboard PNG exceeds 12 MiB".to_owned());
    }
    Ok(png)
}

fn decode_rgba(
    data: &[u8],
    format: ImageFormat,
    encoded_limit: usize,
) -> Result<image::RgbaImage, String> {
    if data.len() > encoded_limit {
        return Err("encoded image exceeds its size limit".to_owned());
    }

    let (encoded_width, encoded_height) =
        image::ImageReader::with_format(std::io::Cursor::new(data), format)
            .into_dimensions()
            .map_err(|error| format!("failed to read terminal image dimensions: {error}"))?;
    let pixels = u64::from(encoded_width)
        .checked_mul(u64::from(encoded_height))
        .ok_or_else(|| "terminal image dimensions overflow".to_owned())?;
    if pixels == 0 || pixels > MAX_IMAGE_PIXELS {
        return Err("terminal image exceeds 16 megapixels".to_owned());
    }
    let decoded_bytes = usize::try_from(pixels.checked_mul(4).unwrap_or(u64::MAX))
        .map_err(|_| "terminal image allocation size overflow".to_owned())?;
    if decoded_bytes > MAX_IMAGE_DECODED_BYTES {
        return Err("decoded terminal image exceeds 64 MiB".to_owned());
    }

    // Decode only the first frame and cap allocations before decoding.
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_IMAGE_DECODED_BYTES as u64);
    reader.limits(limits);
    let decoded =
        reader.decode().map_err(|error| format!("failed to decode terminal image: {error}"))?;
    let (actual_width, actual_height) = (decoded.width(), decoded.height());
    if actual_width != encoded_width || actual_height != encoded_height {
        return Err("terminal image dimensions changed while decoding".to_owned());
    }

    Ok(decoded.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_terminal::inline_image::{ImageIdentity, ImageLayout, SourceRect};

    fn placement(image: &Arc<ImageIdentity>) -> ImagePlacement {
        ImagePlacement {
            id: image.id,
            lifetime: Arc::downgrade(image),
            layout: ImageLayout {
                width: 2.0,
                height: 1.0,
                cell_width: 1.0,
                cell_height: 1.0,
                columns: 2,
                rows: 1,
                source: SourceRect::full(2, 1),
            },
        }
    }

    fn pending(data: Vec<u8>) -> PendingInlineImage {
        PendingInlineImage {
            sequence: 7,
            data: Arc::new(data),
            placement: placement(&ImageIdentity::new()),
            channels: None,
        }
    }

    fn pending_pixels(data: Vec<u8>, channels: u8) -> PendingInlineImage {
        PendingInlineImage {
            sequence: 8,
            data: Arc::new(data),
            placement: placement(&ImageIdentity::new()),
            channels: Some(channels),
        }
    }

    #[test]
    fn raw_rgb_pixels_are_repacked_into_opaque_bgra() {
        let bgra = pixels_to_bgra(&[10, 20, 30, 40, 50, 60], 3, 2).unwrap();
        assert_eq!(bgra, vec![30, 20, 10, 0xff, 60, 50, 40, 0xff]);
    }

    #[test]
    fn raw_rgba_pixels_keep_their_alpha_when_repacked() {
        let bgra = pixels_to_bgra(&[10, 20, 30, 40], 4, 1).unwrap();
        assert_eq!(bgra, vec![30, 20, 10, 40]);
    }

    #[test]
    fn raw_pixels_shorter_than_the_declared_size_are_rejected() {
        let error = pixels_to_bgra(&[1, 2, 3], 3, 2).unwrap_err();
        assert!(error.contains("shorter"), "{error}");
    }

    #[test]
    fn raw_transfer_lands_in_the_cache_with_the_placement_geometry() {
        let mut store = InlineImageStore::default();
        let image = ImageIdentity::new();
        let placement = placement(&image);
        store.enqueue_pixels(Arc::new(vec![1, 2, 3, 4, 5, 6]), 3, placement).unwrap();
        let pending = store.start_next().expect("queued transfer should start");
        let (_, decoded) = decode(pending).unwrap();
        assert_eq!(decoded.decoded_bytes, 8);
        assert_eq!(decoded.placement.layout.columns, 2);
    }

    #[test]
    fn raw_transfer_with_an_unknown_channel_count_is_dropped() {
        let mut store = InlineImageStore::default();
        let image = ImageIdentity::new();
        let error = store.enqueue_pixels(Arc::new(vec![0; 8]), 2, placement(&image)).unwrap_err();
        assert!(error.contains("channel"), "{error}");
    }

    fn encoded_png() -> Vec<u8> {
        let pixels = image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255]));
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels).write_to(&mut output, ImageFormat::Png).unwrap();
        output.into_inner()
    }

    #[test]
    fn queue_rejects_unbounded_encoded_backlog() {
        let mut store = InlineImageStore::default();
        let chunk = Arc::new(vec![0; 3 * 1024 * 1024]);
        let image = ImageIdentity::new();
        for _ in 0..10 {
            store.enqueue(chunk.clone(), placement(&image)).unwrap();
        }
        assert!(store.enqueue(chunk, placement(&image)).is_err());
    }

    #[test]
    fn decode_png_produces_one_bgra_frame() {
        let (sequence, decoded) = decode(pending(encoded_png())).unwrap();
        assert_eq!(sequence, 7);
        assert_eq!(decoded.image.frame_count(), 1);
        assert_eq!(decoded.image.as_bytes(0), Some([30, 20, 10, 255, 30, 20, 10, 255].as_slice()));
    }

    #[test]
    fn decode_gif_keeps_only_the_first_frame() {
        use image::codecs::gif::{GifEncoder, Repeat};

        let mut encoded = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut encoded);
            encoder.set_repeat(Repeat::Infinite).unwrap();
            encoder
                .encode_frame(Frame::new(image::RgbaImage::from_pixel(
                    2,
                    1,
                    image::Rgba([255, 0, 0, 255]),
                )))
                .unwrap();
            encoder
                .encode_frame(Frame::new(image::RgbaImage::from_pixel(
                    2,
                    1,
                    image::Rgba([0, 255, 0, 255]),
                )))
                .unwrap();
        }

        let (_, decoded) = decode(pending(encoded)).unwrap();
        assert_eq!(decoded.image.frame_count(), 1);
        assert_eq!(decoded.image.as_bytes(0), Some([0, 0, 255, 255, 0, 0, 255, 255].as_slice()));
    }

    #[test]
    fn decode_rejects_mp4_bytes() {
        let Err(error) = decode(pending(b"\0\0\0\x18ftypmp42not-an-image".to_vec())) else {
            panic!("MP4 input was accepted as a terminal image");
        };
        assert!(error.contains("unsupported terminal image"));
    }

    #[test]
    fn clipboard_bitmaps_become_png_without_extending_terminal_protocol_formats() {
        let pixels = image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255]));
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels.clone())
            .write_to(&mut output, ImageFormat::Bmp)
            .unwrap();
        let bmp = output.into_inner();
        assert!(decode_bytes(&bmp).is_err());
        let png = clipboard_png(&bmp).unwrap();
        assert_eq!(image::guess_format(&png).unwrap(), ImageFormat::Png);
        assert_eq!(image::load_from_memory(&png).unwrap().into_rgba8(), pixels);
    }

    #[test]
    fn clipboard_rejects_invalid_bytes_and_pixel_bombs_before_staging() {
        assert!(clipboard_png(b"not an image").is_err());
        let mut png = encoded_png();
        png[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(clipboard_png(&png).is_err());
    }

    #[test]
    fn cache_is_bounded_by_count() {
        let mut store = InlineImageStore::default();
        let mut owners = Vec::new();
        for sequence in 0..MAX_CACHED_IMAGES + 3 {
            owners.push(ImageIdentity::new());
            let rgba = image::RgbaImage::new(1, 1);
            store
                .finish(Ok((
                    sequence as u64,
                    InlineImage {
                        image: Arc::new(RenderImage::new([Frame::new(rgba)])),
                        placement: placement(owners.last().unwrap()),
                        decoded_bytes: 4,
                    },
                )))
                .unwrap();
        }
        assert_eq!(store.frame_images().len(), MAX_CACHED_IMAGES);
    }

    #[test]
    fn cache_drops_an_image_after_its_last_grid_cell_is_removed() {
        let mut store = InlineImageStore::default();
        let owner = ImageIdentity::new();
        store
            .finish(Ok((
                0,
                InlineImage {
                    image: Arc::new(RenderImage::new([Frame::new(image::RgbaImage::new(1, 1))])),
                    placement: placement(&owner),
                    decoded_bytes: 4,
                },
            )))
            .unwrap();

        assert_eq!(store.frame_images().len(), 1);
        drop(owner);
        assert!(store.frame_images().is_empty());
        assert_eq!(store.decoded_bytes, 0);
    }

    #[test]
    fn clearing_while_decode_runs_cannot_resurrect_image() {
        let owner = ImageIdentity::new();
        let mut store = InlineImageStore::default();
        store.enqueue(Arc::new(encoded_png()), placement(&owner)).unwrap();
        let pending = store.start_next().unwrap();
        drop(owner);
        store.finish(decode(pending)).unwrap();
        assert!(store.frame_images().is_empty());
        assert_eq!(store.decoded_bytes, 0);
        assert!(!store.decoding);
    }

    #[test]
    fn obsolete_queued_jobs_release_bytes_without_decoding() {
        let owner = ImageIdentity::new();
        let mut store = InlineImageStore::default();
        store.enqueue(Arc::new(encoded_png()), placement(&owner)).unwrap();
        drop(owner);
        assert!(store.start_next().is_none());
        assert_eq!(store.queued_bytes, 0);
    }

    /// A placement shows a sub-region of the bitmap by scaling that region into
    /// the cells it declared, so the slice's top-left lands where the tile
    /// starts. The clip is still the tile the grid allocated: the page cannot
    /// paint outside the cells it owns.
    #[test]
    fn cropped_placement_moves_the_bitmap_under_its_tile() {
        let layout = ImageLayout {
            width: 8.0,
            height: 4.0,
            cell_width: 1.0,
            cell_height: 1.0,
            columns: 2,
            rows: 2,
            source: SourceRect { x: 0, y: 2, width: 8, height: 2 },
        };
        let run =
            ImageRun { id: 0, row: 1, column: 3, source_row: 0, source_column: 0, columns: 2 };
        let row_y = |row: f32| px(row);
        let (image, clip) =
            image_geometry(layout, &run, point(px(0.0), px(0.0)), px(1.0), px(1.0), &row_y);
        // The two-row slice fills the two-row box, so the bitmap is drawn at two
        // points per bitmap row and shifted up by the slice offset.
        assert_eq!(image.origin, point(px(3.0), px(-1.0)));
        assert_eq!(image.size, size(px(2.0), px(4.0)));
        assert_eq!(clip.origin, point(px(3.0), px(1.0)));
        assert_eq!(clip.size, size(px(2.0), px(1.0)));

        // A run that is itself a slice of the image (a wide row chunked into
        // runs) offsets along x by the cells it skipped.
        let sliced = ImageRun { source_column: 2, ..run };
        let (shifted, _) =
            image_geometry(layout, &sliced, point(px(0.0), px(0.0)), px(1.0), px(1.0), &row_y);
        assert_eq!(shifted.origin.x, px(3.0 - 2.0));

        // A one-row slice fills the box on its own: the row below it is part of
        // the declared cells, so it shows the slice too rather than nothing.
        let shallow =
            ImageLayout { source: SourceRect { x: 0, y: 2, width: 8, height: 1 }, ..layout };
        let tall = ImageRun { row: 0, columns: 8, ..run };
        let (_, clipped) =
            image_geometry(shallow, &tall, point(px(0.0), px(0.0)), px(1.0), px(1.0), &row_y);
        assert_eq!(clipped.size, size(px(2.0), px(1.0)));
    }

    /// A reader that crops to the part of the page it wants visible (one `y`/`h`
    /// per scroll step) expects that crop to cover the block it placed, not a
    /// fraction of it. Scaling by the whole bitmap instead left the slice a few
    /// rows tall inside a tall box, which is what a user saw as a blank page.
    #[test]
    fn a_crop_fills_every_row_of_its_cell_box() {
        let layout = ImageLayout {
            width: 240.0,
            height: 160.0,
            cell_width: 12.0,
            cell_height: 20.0,
            columns: 20,
            rows: 8,
            source: SourceRect { x: 0, y: 0, width: 240, height: 20 },
        };
        let row_y = |row: f32| px(row * 20.0);
        let first =
            ImageRun { id: 0, row: 0, column: 0, source_row: 0, source_column: 0, columns: 20 };
        let origin = point(px(0.0), px(0.0));
        let (image, top) = image_geometry(layout, &first, origin, px(12.0), px(20.0), &row_y);
        assert_eq!(top.size, size(px(240.0), px(20.0)));
        // Twenty bitmap rows stretched over eight cell rows of 20px, so the
        // whole 160-row bitmap is drawn 1280 points tall and clipped to the box.
        assert_eq!(image.size, size(px(240.0), px(1280.0)));
        let last = ImageRun { row: 7, source_row: 7, ..first };
        let (_, bottom) = image_geometry(layout, &last, origin, px(12.0), px(20.0), &row_y);
        assert_eq!(bottom.size, size(px(240.0), px(20.0)));
    }

    /// A placement is scaled into the cells it declared, so a slice of the
    /// bitmap has to be shifted by the scaled offset. Shifting by the raw pixel
    /// offset put the slice above the tile and left only its last pixels
    /// visible.
    #[test]
    fn a_slice_lands_inside_the_cells_its_placement_owns() {
        let layout = ImageLayout {
            width: 240.0,
            height: 160.0,
            cell_width: 14.0,
            cell_height: 36.0,
            columns: 22,
            rows: 8,
            source: SourceRect { x: 0, y: 80, width: 240, height: 80 },
        };
        let row_y = |row: f32| px(row * 18.0);
        let run =
            ImageRun { id: 0, row: 0, column: 0, source_row: 0, source_column: 0, columns: 22 };
        let origin = point(px(0.0), px(0.0));
        let (image, first) = image_geometry(layout, &run, origin, px(7.0), px(18.0), &row_y);
        // 22 cells of 7px wide, 8 rows of 18px tall: the 240x80 slice fills it.
        assert_eq!(image.size, size(px(154.0), px(288.0)));
        assert_eq!(first.origin.y, px(0.0), "the slice starts at the tile top");
        assert_eq!(first.size.height, px(18.0), "the whole first row belongs to it");
        let fourth = ImageRun { row: 3, source_row: 3, ..run };
        let (_, tail) = image_geometry(layout, &fourth, origin, px(7.0), px(18.0), &row_y);
        assert_eq!(tail.size.height, px(18.0), "row 3 is inside the slice");
        let last = ImageRun { row: 7, source_row: 7, ..run };
        let (_, bottom) = image_geometry(layout, &last, origin, px(7.0), px(18.0), &row_y);
        assert_eq!(bottom.size.height, px(18.0), "the slice fills the last row too");
    }

    /// A pager scrolls a page by re-placing the same transfer with a new slice.
    /// The pixels are the same allocation, so the store must retarget the queued
    /// job instead of decoding the bitmap again — a page scrolls many times per
    /// second and every decode blocks the frame.
    #[test]
    fn replaying_a_crop_reuses_the_queued_transfer() {
        let mut store = InlineImageStore::default();
        let owner = ImageIdentity::new();
        let data = Arc::new(vec![1u8, 2, 3, 4, 5, 6]);
        let first = placement(&owner);
        store.enqueue_pixels(data.clone(), 3, first.clone()).unwrap();

        let mut moved = first.clone();
        moved.layout.source = SourceRect { x: 1, y: 0, width: 1, height: 1 };
        store.enqueue_pixels(data, 3, moved.clone()).unwrap();
        assert_eq!(store.queued.len(), 1);
        assert_eq!(store.queued[0].placement.layout.source, moved.layout.source);

        // A retransfer hands out a new buffer, and that one has to be decoded.
        let replaced = ImageIdentity::new();
        store.enqueue_pixels(Arc::new(vec![7u8; 6]), 3, placement(&replaced)).unwrap();
        assert_eq!(store.queued.len(), 2);
    }
}
