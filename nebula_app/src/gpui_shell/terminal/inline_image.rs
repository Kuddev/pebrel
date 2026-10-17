//! Bounded decode/cache state for terminal image protocols.
//!
//! Encoded bytes arrive through the PTY, but decoding is serialized on the
//! background executor. This prevents a burst of compressed image bombs from
//! expanding concurrently on the UI thread or exhausting process memory.

use std::collections::VecDeque;
use std::sync::Arc;

use gpui::{Bounds, ContentMask, Corners, Pixels, RenderImage, Window, point, px, size};
use image::{Frame, ImageFormat};
use nebula_terminal::inline_image::{ImagePlacement, ImageRun};

const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_ENCODED_BYTES: usize = 12 * 1024 * 1024;
const MAX_IMAGE_DECODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHED_IMAGES: usize = 16;
const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_QUEUED_IMAGES: usize = 16;
const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;

pub(super) struct PendingInlineImage {
    sequence: u64,
    data: Arc<Vec<u8>>,
    placement: ImagePlacement,
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
        self.queued.push_back(PendingInlineImage { sequence, data, placement });
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
        self.decoded_bytes = self.images.iter().map(|(_, image)| image.decoded_bytes).sum();
        self.images.iter().map(|(_, image)| image.clone()).collect()
    }
}

pub(super) fn decode(pending: PendingInlineImage) -> Result<(u64, InlineImage), String> {
    let (render_image, decoded_bytes) = decode_bytes(pending.data.as_slice())?;
    Ok((
        pending.sequence,
        InlineImage { image: render_image, placement: pending.placement, decoded_bytes },
    ))
}

/// Paint only cells present in this frame's grid snapshot. Sampling always
/// uses the full image bounds, so erased/reflowed fragments are never stretched.
pub(super) fn paint(
    images: &[InlineImage],
    runs: &[ImageRun],
    bounds: Bounds<Pixels>,
    cell_width: Pixels,
    cell_height: Pixels,
    window: &mut Window,
) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        for run in runs {
            let Some(inline) = images.iter().find(|image| image.placement.id == run.id) else {
                continue;
            };
            let layout = inline.placement.layout;
            let origin = point(
                bounds.origin.x + cell_width * run.column as f32,
                bounds.origin.y + cell_height * run.row as f32,
            );
            let image_bounds = Bounds::new(
                point(
                    origin.x - cell_width * run.source_column as f32,
                    origin.y - cell_height * run.source_row as f32,
                ),
                size(
                    cell_width * (layout.width / layout.cell_width),
                    cell_height * (layout.height / layout.cell_height),
                ),
            );
            let clip = Bounds::new(origin, size(cell_width * run.columns as f32, cell_height))
                .intersect(&image_bounds);
            if clip.size.width <= px(0.0) || clip.size.height <= px(0.0) {
                continue;
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
    use nebula_terminal::inline_image::{ImageIdentity, ImageLayout};

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
            },
        }
    }

    fn pending(data: Vec<u8>) -> PendingInlineImage {
        PendingInlineImage {
            sequence: 7,
            data: Arc::new(data),
            placement: placement(&ImageIdentity::new()),
        }
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
}
