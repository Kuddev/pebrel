//! Bounded, serial wallpaper preparation. No GPUI objects or UI-thread file I/O.
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use image::{DynamicImage, ImageDecoder, ImageError, ImageReader, RgbaImage};

use super::optics::{self, StripGeometry};

// These bound the resources owned by this loader, not the entire process or a
// decoder's undocumented scratch space. One job runs at a time across windows.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
pub(super) const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_EDGE: u32 = 2048;
const MAX_PREVIEW_EDGE: u32 = 512;
const MAX_BLUR_EDGE: u32 = 512;
static PREPARATION: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileStamp {
    modified: Option<SystemTime>,
    bytes: u64,
}

pub(super) struct Request {
    pub path: PathBuf,
    pub cached: Option<FileStamp>,
    pub generation: Arc<AtomicU64>,
    pub version: u64,
}

impl Request {
    fn cancelled(&self) -> bool {
        self.generation.load(Ordering::Acquire) != self.version
    }
}

pub(super) struct LoadedImage {
    pub pixels: RgbaImage,
    pub layout_width: u32,
    pub layout_height: u32,
    pub stamp: FileStamp,
}

pub(super) struct LoadedWallpaper {
    pub source: LoadedImage,
    pub blurred: RgbaImage,
    pub mica: RgbaImage,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LoadError {
    TooLarge,
    Read,
    Decode,
}

impl From<ImageError> for LoadError {
    fn from(error: ImageError) -> Self {
        match error {
            ImageError::Limits(_) => Self::TooLarge,
            ImageError::IoError(_) => Self::Read,
            _ => Self::Decode,
        }
    }
}

pub(super) fn load(request: Request) -> Result<Option<LoadedWallpaper>, LoadError> {
    let _preparation = PREPARATION.lock().unwrap_or_else(|error| error.into_inner());
    let Some(source) = load_inner(&request)? else { return Ok(None) };
    if request.cancelled() {
        return Ok(None);
    }
    let blurred = blur_wallpaper(&source.pixels, false);
    if request.cancelled() {
        return Ok(None);
    }
    let mica = blur_wallpaper(&source.pixels, true);
    Ok((!request.cancelled()).then_some(LoadedWallpaper { source, blurred, mica }))
}

pub(super) fn load_preview(request: Request) -> Result<Option<LoadedImage>, LoadError> {
    let _preparation = PREPARATION.lock().unwrap_or_else(|error| error.into_inner());
    let Some(mut loaded) = load_inner(&request)? else { return Ok(None) };
    if request.cancelled() {
        return Ok(None);
    }
    if loaded.pixels.width() > MAX_PREVIEW_EDGE || loaded.pixels.height() > MAX_PREVIEW_EDGE {
        loaded.pixels =
            image::imageops::thumbnail(&loaded.pixels, MAX_PREVIEW_EDGE, MAX_PREVIEW_EDGE);
    }
    Ok((!request.cancelled()).then_some(loaded))
}

pub(super) fn prepare_strip(
    bytes: &[u8],
    dimensions: (u32, u32),
    geometry: StripGeometry,
    appearance: optics::StripAppearance,
    cancelled: impl Fn() -> bool,
) -> Option<RgbaImage> {
    let _preparation = PREPARATION.lock().unwrap_or_else(|error| error.into_inner());
    if cancelled() {
        return None;
    }
    optics::workspace_strip(bytes, dimensions, geometry, appearance, cancelled)
}

fn blur_wallpaper(pixels: &RgbaImage, mica: bool) -> RgbaImage {
    use image::imageops::{fast_blur, thumbnail};

    let has_alpha = pixels.pixels().any(|pixel| pixel[3] < 255);
    let (width, height) = pixels.dimensions();
    let edge = width.max(height);
    let downsample = |image: &RgbaImage| {
        if edge <= MAX_BLUR_EDGE {
            image.clone()
        } else {
            let scaled = |length: u32| ((length * MAX_BLUR_EDGE + edge / 2) / edge).max(1);
            thumbnail(image, scaled(width), scaled(height))
        }
    };
    let reduced = if has_alpha {
        // Interpolate color in premultiplied form so invisible pixels do not tint visible edges.
        let mut premultiplied = pixels.clone();
        for pixel in premultiplied.pixels_mut() {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel.0[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        downsample(&premultiplied)
    } else {
        downsample(pixels)
    };

    // S1 Mica uses a 14 px blur at quarter resolution; retain the light theme kernel.
    let sigma =
        (if mica { 56.0 } else { 18.0 } / 1240.0) * reduced.width().max(reduced.height()) as f32;
    let mut blurred = fast_blur(&reduced, sigma);
    if has_alpha {
        for pixel in blurred.pixels_mut() {
            let alpha = u16::from(pixel[3]);
            if alpha != 0 {
                for channel in &mut pixel.0[..3] {
                    *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
    }
    if mica {
        for pixel in blurred.pixels_mut() {
            let [b, g, r, _] = pixel.0.map(f32::from);
            let luma = 0.072 * b + 0.715 * g + 0.213 * r;
            for channel in &mut pixel.0[..3] {
                *channel = ((luma + (f32::from(*channel) - luma) * 1.75) * 0.78)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    blurred
}

fn load_inner(request: &Request) -> Result<Option<LoadedImage>, LoadError> {
    if request.cancelled() {
        return Ok(None);
    }
    let file = File::open(&request.path).map_err(|_| LoadError::Read)?;
    let metadata = file.metadata().map_err(|_| LoadError::Read)?;
    let stamp = FileStamp { modified: metadata.modified().ok(), bytes: metadata.len() };
    if stamp.bytes > MAX_FILE_BYTES {
        return Err(LoadError::TooLarge);
    }
    if request.cached.as_ref() == Some(&stamp) {
        return Ok(None);
    }

    // Stream the encoded input instead of retaining a second full-file Vec.
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|_| LoadError::Read)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    admit_dimensions(width, height, decoder.total_bytes())?;
    if request.cancelled() {
        return Ok(None);
    }
    let (target_width, target_height) = bounded_dimensions(width, height);
    let mut decoded = DynamicImage::from_decoder(decoder)?;
    if request.cancelled() {
        return Ok(None);
    }
    if (width, height) != (target_width, target_height) {
        // Integer area thumbnailing avoids Triangle's full-width RGBA32F
        // intermediate. Shrink before RGBA conversion, including 16-bit input.
        decoded = decoded.thumbnail_exact(target_width, target_height);
    }
    let mut pixels = decoded.into_rgba8();
    if request.cancelled() {
        return Ok(None);
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    // Preserve the previous native-fit geometry independently of texture quality.
    let layout_scale = (2560.0_f64 / f64::from(width.max(height))).min(1.0);
    let layout_width = (f64::from(width) * layout_scale).round().max(1.0) as u32;
    let layout_height = (f64::from(height) * layout_scale).round().max(1.0) as u32;
    Ok(Some(LoadedImage { pixels, layout_width, layout_height, stamp }))
}

fn admit_dimensions(width: u32, height: u32, decoded_bytes: u64) -> Result<(), LoadError> {
    let rgba_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(LoadError::TooLarge)?;
    if width == 0 || height == 0 {
        return Err(LoadError::Decode);
    }
    if decoded_bytes > MAX_DECODE_BYTES || rgba_bytes > MAX_DECODE_BYTES {
        return Err(LoadError::TooLarge);
    }
    Ok(())
}

fn bounded_dimensions(width: u32, height: u32) -> (u32, u32) {
    let pixels = f64::from(width) * f64::from(height);
    let scale = (f64::from(MAX_IMAGE_EDGE) / f64::from(width.max(height)))
        .min((MAX_IMAGE_BYTES as f64 / (pixels * 4.0)).sqrt())
        .min(1.0);
    (
        (f64::from(width) * scale).floor().max(1.0) as u32,
        (f64::from(height) * scale).floor().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: PathBuf) -> Request {
        Request { path, cached: None, generation: Arc::new(AtomicU64::new(1)), version: 1 }
    }

    #[test]
    fn output_budget_handles_large_square_panorama_and_portrait() {
        for (w, h) in [(7680, 4320), (4096, 4096), (32000, 8), (8, 32000), (7, 3)] {
            let (width, height) = bounded_dimensions(w, h);
            assert!(width <= MAX_IMAGE_EDGE && height <= MAX_IMAGE_EDGE);
            assert!(u64::from(width) * u64::from(height) * 4 <= MAX_IMAGE_BYTES);
            assert!(width <= w && height <= h);
        }
        assert_eq!(bounded_dimensions(7, 3), (7, 3));
    }

    #[test]
    fn source_budget_is_checked_before_allocating_pixels() {
        assert!(admit_dimensions(7680, 4320, 7680 * 4320 * 4).is_ok());
        assert_eq!(admit_dimensions(10000, 10000, 1), Err(LoadError::TooLarge));
        assert_eq!(admit_dimensions(16, 16, MAX_DECODE_BYTES + 1), Err(LoadError::TooLarge));
        assert_eq!(admit_dimensions(0, 16, 0), Err(LoadError::Decode));
    }

    #[test]
    fn preview_has_a_one_mib_output_budget_and_keeps_layout_and_bgra() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview.png");
        RgbaImage::from_pixel(1200, 800, image::Rgba([10, 20, 30, 255])).save(&path).unwrap();
        let loaded = load_preview(request(path)).unwrap().unwrap();
        assert!(
            loaded.pixels.width() <= MAX_PREVIEW_EDGE && loaded.pixels.height() <= MAX_PREVIEW_EDGE
        );
        assert!(loaded.pixels.len() <= 1024 * 1024);
        assert_eq!((loaded.layout_width, loaded.layout_height), (1200, 800));
        assert_eq!(loaded.pixels.get_pixel(0, 0).0, [30, 20, 10, 255]);
    }

    #[test]
    fn cancelled_preview_does_not_open_or_decode_a_file() {
        let request = request(PathBuf::from("does-not-exist.png"));
        request.generation.fetch_add(1, Ordering::Release);
        assert!(load_preview(request).unwrap().is_none());
    }

    #[test]
    fn pixels_move_to_bgra_without_baking_opacity_and_unchanged_files_are_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wallpaper.png");
        RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 128])).save(&path).unwrap();
        let loaded = load(request(path.clone())).unwrap().unwrap();
        assert_eq!(loaded.source.pixels.get_pixel(0, 0).0, [30, 20, 10, 128]);
        let mut cached = request(path.clone());
        cached.cached = Some(loaded.source.stamp);
        assert!(load(cached).unwrap().is_none());
        let mut stale = request(path);
        stale.version = 0;
        assert!(load(stale).unwrap().is_none());
    }

    #[test]
    fn settings_blur_softens_edges_without_importing_hidden_color() {
        let image = RgbaImage::from_fn(1024, 512, |x, _| {
            if x < 512 { image::Rgba([180, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 0]) }
        });
        let blurred = blur_wallpaper(&image, false);
        assert_eq!(blurred.dimensions(), (512, 256));
        assert_eq!(blurred.get_pixel(200, 128).0, [180, 0, 0, 255]);
        assert_eq!(blurred.get_pixel(310, 128)[3], 0);
        let edge = blurred.get_pixel(255, 128);
        assert!((80..=180).contains(&edge[3]));
        assert!((175..=185).contains(&edge[0]));
        assert_eq!(edge[2], 0);
        assert_eq!(blur_wallpaper(&RgbaImage::new(1, 2048), false).dimensions(), (1, 512));
    }

    #[test]
    fn mica_has_a_broad_blur_and_retains_opaque_edges() {
        let image = RgbaImage::from_fn(1240, 620, |x, _| {
            image::Rgba(if x < 620 { [255; 4] } else { [0, 0, 0, 255] })
        });
        let light = blur_wallpaper(&image, false);
        let mica = blur_wallpaper(&image, true);
        assert!(mica.pixels().all(|pixel| pixel[3] == 255));
        assert_eq!(mica.get_pixel(0, 0).0, [199, 199, 199, 255]);
        assert!(mica.get_pixel(285, 128)[0] > light.get_pixel(285, 128)[0] + 5);
        assert!(mica.len() <= 1024 * 1024);
        let transparent = RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 255, 0]));
        assert!(blur_wallpaper(&transparent, true).pixels().all(|pixel| pixel[3] == 0));
    }

    #[test]
    fn corrupt_and_missing_images_fail_without_a_render_resource() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.png");
        assert!(matches!(load(request(path.clone())), Err(LoadError::Read)));
        std::fs::write(&path, b"not an image").unwrap();
        assert!(matches!(load(request(path)), Err(LoadError::Decode)));
    }
}
