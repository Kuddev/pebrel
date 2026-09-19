//! Bounded, serial wallpaper preparation. No GPUI objects or UI-thread file I/O.
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use image::{DynamicImage, ImageDecoder, ImageError, ImageReader, RgbaImage};

// These bound the resources owned by this loader, not the entire process or a
// decoder's undocumented scratch space. One job runs at a time across windows.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
pub(super) const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_EDGE: u32 = 2048;

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

pub(super) fn load(request: Request) -> Result<Option<LoadedImage>, LoadError> {
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
    fn pixels_move_to_bgra_without_baking_opacity_and_unchanged_files_are_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wallpaper.png");
        RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 128])).save(&path).unwrap();
        let loaded = load(request(path.clone())).unwrap().unwrap();
        assert_eq!(loaded.pixels.get_pixel(0, 0).0, [30, 20, 10, 128]);
        let mut cached = request(path.clone());
        cached.cached = Some(loaded.stamp);
        assert!(load(cached).unwrap().is_none());
        let mut stale = request(path);
        stale.version = 0;
        assert!(load(stale).unwrap().is_none());
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
