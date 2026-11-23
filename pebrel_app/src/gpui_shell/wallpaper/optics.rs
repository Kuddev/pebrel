//! Wallpaper filters and masks work on BGRA pixels off the UI thread.

use image::{Rgba, RgbaImage, imageops};

pub(super) const FEATHER: f32 = 64.0;
const SOFT_OVERHANG: f32 = 20.0;
const PADDING: f32 = 26.0;
const MAX_STRIP_PIXELS: f32 = 512.0 * 1024.0;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct StripGeometry {
    pub sidebar: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub image: [f32; 4],
}

pub(super) fn workspace_strip(
    bytes: &[u8],
    (source_width, source_height): (u32, u32),
    geometry: StripGeometry,
    tint: [u8; 3],
    cancelled: impl Fn() -> bool,
) -> Option<RgbaImage> {
    let StripGeometry { sidebar, width, height, scale, image } = geometry;
    // Bound the strip and its filter intermediates even for giant virtual desktops.
    let scale = scale
        .min(1.5)
        .min((MAX_STRIP_PIXELS / (width * height)).sqrt())
        .min(1024.0 / (width + 2.0 * PADDING))
        .min(2048.0 / (height + 2.0 * PADDING));
    let w = (width * scale).floor().max(1.0) as u32;
    let h = (height * scale).floor().max(1.0) as u32;
    let pad = (PADDING * scale).ceil() as u32;
    let pitch_x = width / w as f32;
    let pitch_y = height / h as f32;
    let columns: Vec<_> = (0..w + 2 * pad)
        .map(|x| sample_axis(x, pad, pitch_x, image[0], image[2], source_width))
        .collect();
    let weights: Vec<_> = (0..w).map(|x| masks((x as f32 + 0.5) * pitch_x, sidebar)).collect();

    let mut source = RgbaImage::new(w + 2 * pad, h + 2 * pad);
    for (y, row) in source.rows_mut().enumerate() {
        if cancelled() {
            return None;
        }
        if let Some(vertical) =
            sample_axis(y as u32, pad, pitch_y, image[1], image[3], source_height)
        {
            let stride = source_width as usize * 4;
            let top = &bytes[vertical.low * stride..][..stride];
            let bottom = &bytes[vertical.high * stride..][..stride];
            for (pixel, column) in row.zip(&columns) {
                if let Some(horizontal) = column {
                    *pixel = bilinear(top, bottom, *horizontal, vertical.weight);
                }
            }
        }
    }

    let mut result = RgbaImage::new(w, h);
    {
        let mid = filtered(&source, 2.6 * scale, 0.55);
        for (x, y, pixel) in result.enumerate_pixels_mut() {
            let (mid_alpha, _) = weights[x as usize];
            *pixel = mix(
                *source.get_pixel(x + pad, y + pad),
                *mid.get_pixel(x + pad, y + pad),
                mid_alpha,
            );
        }
    }
    if cancelled() {
        return None;
    }
    let soft = filtered(&source, 5.5 * scale, 1.0);
    drop(source);
    if cancelled() {
        return None;
    }
    for (y, row) in result.rows_mut().enumerate() {
        if cancelled() {
            return None;
        }
        let logical_y = (y as f32 + 0.5) * pitch_y;
        for (x, pixel) in row.enumerate() {
            let logical_x = (x as f32 + 0.5) * pitch_x;
            let (_, soft_alpha) = weights[x];
            if soft_alpha > 0.0 {
                let mut frost = *soft.get_pixel(x as u32 + pad, y as u32 + pad);
                let (grain_value, grain_alpha) = grain(logical_x as u32, logical_y as u32);
                color_overlay(&mut frost, tint, 0.12);
                color_overlay(&mut frost, [grain_value; 3], grain_alpha * 0.030);
                *pixel = mix(*pixel, frost, soft_alpha);
            }
            *pixel = unpremultiply(*pixel);
        }
    }
    Some(result)
}

fn masks(x: f32, sidebar: f32) -> (f32, f32) {
    let mid = (x - (sidebar - 12.0)) / (FEATHER + 12.0);
    let soft = x / (sidebar + SOFT_OVERHANG);
    (
        stops(mid, &[(0.0, 1.0), (0.45, 0.52), (0.78, 0.14), (1.0, 0.0)]),
        stops(soft, &[(0.0, 1.0), (0.56, 0.92), (0.84, 0.34), (1.0, 0.0)]),
    )
}

fn stops(position: f32, stops: &[(f32, f32)]) -> f32 {
    for pair in stops.windows(2) {
        let [(start, from), (end, to)] = [pair[0], pair[1]];
        if position <= end {
            let weight = ((position - start) / (end - start)).max(0.0);
            return from + (to - from) * weight;
        }
    }
    0.0
}

fn filtered(source: &RgbaImage, sigma: f32, strength: f32) -> RgbaImage {
    let contrast = 1.0 - 0.24 * strength;
    let exposure = 1.0 - 0.06 * strength;
    let gain = contrast * exposure;
    let offset = (1.0 - contrast) * exposure * 0.5;
    let prepared = RgbaImage::from_fn(source.width(), source.height(), |x, y| {
        let mut pixel = *source.get_pixel(x, y);
        let [b, g, r, alpha] = pixel.0.map(f32::from);
        let luma = 0.072 * b + 0.715 * g + 0.213 * r;
        let bias = alpha * offset - luma * gain * 0.14;
        // Contrast, exposure and saturation are affine in premultiplied color.
        // Only saturation can clip at this stage.
        for channel in &mut pixel.0[..3] {
            *channel = byte((f32::from(*channel) * gain * 1.14 + bias).min(alpha));
        }
        pixel
    });
    imageops::fast_blur(&prepared, sigma)
}

#[derive(Clone, Copy)]
struct Sample {
    low: usize,
    high: usize,
    weight: u32,
}

fn sample_axis(
    index: u32,
    pad: u32,
    pitch: f32,
    origin: f32,
    length: f32,
    extent: u32,
) -> Option<Sample> {
    let coordinate = ((index as f32 - pad as f32 + 0.5) * pitch - origin) / length;
    if !(0.0..1.0).contains(&coordinate) {
        return None;
    }
    let sample = (coordinate * extent as f32 - 0.5).clamp(0.0, (extent - 1) as f32);
    let low = sample as usize;
    Some(Sample {
        low,
        high: (low + 1).min(extent as usize - 1),
        weight: (sample.fract() * 256.0 + 0.5) as u32,
    })
}

fn bilinear(top: &[u8], bottom: &[u8], horizontal: Sample, vertical: u32) -> Rgba<u8> {
    let x = horizontal.weight;
    let sample = |row: &[u8], index: usize| {
        let i = index * 4;
        premultiply(Rgba([row[i], row[i + 1], row[i + 2], row[i + 3]]))
    };
    let tl = sample(top, horizontal.low);
    let tr = sample(top, horizontal.high);
    let bl = sample(bottom, horizontal.low);
    let br = sample(bottom, horizontal.high);
    Rgba(std::array::from_fn(|i| {
        let top = u32::from(tl[i]) * (256 - x) + u32::from(tr[i]) * x;
        let bottom = u32::from(bl[i]) * (256 - x) + u32::from(br[i]) * x;
        ((top * (256 - vertical) + bottom * vertical + 32768) >> 16) as u8
    }))
}

fn premultiply(mut pixel: Rgba<u8>) -> Rgba<u8> {
    let alpha = u16::from(pixel[3]);
    if alpha != 255 {
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    pixel
}

fn unpremultiply(mut pixel: Rgba<u8>) -> Rgba<u8> {
    let alpha = u16::from(pixel[3]);
    if alpha > 0 && alpha < 255 {
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    pixel
}

fn mix(a: Rgba<u8>, b: Rgba<u8>, weight: f32) -> Rgba<u8> {
    Rgba(std::array::from_fn(|i| byte(f32::from(a[i]) * (1.0 - weight) + f32::from(b[i]) * weight)))
}

fn color_overlay(pixel: &mut Rgba<u8>, color: [u8; 3], opacity: f32) {
    let alpha = f32::from(pixel[3]) / 255.0;
    for (channel, color) in pixel.0[..3].iter_mut().zip(color) {
        *channel = byte(f32::from(*channel) * (1.0 - opacity) + f32::from(color) * opacity * alpha);
    }
}

fn grain(x: u32, y: u32) -> (u8, f32) {
    // Fixed 128² tile. Black and white are equally likely, and a 3% strength
    // applied to the tile's mean alpha yields roughly 1.2% in the soft band.
    let mut hash = ((y % 128) * 128 + x % 128).wrapping_add(1);
    hash = ((hash >> 16) ^ hash).wrapping_mul(0x45d9f3b);
    hash = ((hash >> 16) ^ hash).wrapping_mul(0x45d9f3b);
    hash ^= hash >> 16;
    (if hash & 1 == 0 { 0 } else { 255 }, ((hash >> 8) % 190) as f32 / 255.0)
}

fn byte(value: f32) -> u8 {
    (value + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l4_masks_end_at_the_reference_distances() {
        let (mid, soft) = masks(0.0, 234.0);
        assert_eq!((mid, soft), (1.0, 1.0));
        let (mid, soft) = masks(254.0, 234.0);
        assert!(mid > 0.0);
        assert_eq!(soft, 0.0);
        assert_eq!(masks(298.0, 234.0), (0.0, 0.0));
    }

    #[test]
    fn white_wallpaper_is_compressed_at_the_sidebar_and_retains_its_right_edge() {
        let source = RgbaImage::from_pixel(360, 80, Rgba([255; 4]));
        let geo = StripGeometry {
            sidebar: 234.0,
            width: 298.0,
            height: 80.0,
            scale: 1.0,
            image: [0.0, 0.0, 360.0, 80.0],
        };
        let strip =
            workspace_strip(source.as_raw(), source.dimensions(), geo, [24; 3], || false).unwrap();
        assert!(strip.get_pixel(20, 40)[0] < 200);
        assert_eq!(strip.get_pixel(297, 40).0, [255; 4]);
        assert!(
            workspace_strip(source.as_raw(), source.dimensions(), geo, [24; 3], || true).is_none()
        );
    }
}
