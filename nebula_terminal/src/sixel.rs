//! Bounded SIXEL syntax, geometry and RGBA decoding.
//!
//! One parser serves both the allocation-free geometry pass on the PTY thread
//! and pixel decoding on a frontend's background executor. No image encoding
//! or renderer dependency is needed in the terminal core.

use crate::vte::ansi::{Color, NamedColor};
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub const MAX_BYTES: usize = 12 * 1024 * 1024;
pub const MAX_EDGE: usize = 4096;
const MAX_PIXELS: usize = MAX_EDGE * MAX_EDGE;
const MAX_PAINTED_PIXELS: usize = 64 * 1024 * 1024;
pub const COLOR_REGISTERS: usize = 256;

pub(crate) fn graphics_reply(sequence: &[u8]) -> Option<String> {
    let body = sequence.strip_prefix(b"?")?.strip_suffix(b"S")?;
    let mut at = 0;
    let (values, count) = parameters::<4>(body, &mut at)?;
    if at != body.len() || count < 2 {
        return None;
    }
    let (item, action) = (values[0], values[1]);
    let status = if !matches!(item, 1 | 2) {
        1
    } else if action == 3 {
        3
    } else if !matches!(action, 1 | 2 | 4) {
        2
    } else {
        0
    };
    let value = if status != 0 {
        "0".to_owned()
    } else if item == 1 {
        COLOR_REGISTERS.to_string()
    } else {
        format!("{MAX_EDGE};{MAX_EDGE}")
    };
    Some(format!("\x1b[?{item};{status};{value}S"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SixelImage {
    bytes: Arc<Vec<u8>>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) aspect: (u32, u32),
    pub(crate) transparent: bool,
    pub(crate) background: Option<Color>,
}

impl SixelImage {
    pub fn parse(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() > MAX_BYTES {
            return None;
        }
        let info = parse(&bytes, None)?;
        Some(Self {
            bytes: Arc::new(bytes),
            width: info.width as u32,
            height: info.height as u32,
            aspect: info.aspect,
            transparent: info.transparent,
            background: None,
        })
    }

    pub fn encoded_len(&self) -> usize {
        self.bytes.len()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Background captured at VT replay; unresolved named colors use the
    /// frontend's configured palette, never a later OSC color override.
    pub fn background_color(&self) -> Option<Color> {
        (!self.transparent).then(|| self.background.unwrap_or(Color::Named(NamedColor::Background)))
    }

    /// Allocate only after the exact same parser has bounded every dimension,
    /// repeat and total paint operation. All indexing remains checked Rust.
    pub fn decode_rgba(&self) -> Option<Vec<u8>> {
        self.decode_rgba_with_background([0, 0, 0])
    }

    pub fn decode_rgba_with_background(&self, background: [u8; 3]) -> Option<Vec<u8>> {
        let mut pixels = vec![0; self.width as usize * self.height as usize * 4];
        if !self.transparent {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[background[0], background[1], background[2], 255]);
            }
        }
        let info = parse(&self.bytes, Some((&mut pixels, self.width as usize)))?;
        (info.width == self.width as usize && info.height == self.height as usize).then_some(pixels)
    }
}

struct Info {
    width: usize,
    height: usize,
    aspect: (u32, u32),
    transparent: bool,
}

/// Read at most N decimal parameters; checked arithmetic rejects oversized
/// numbers instead of wrapping them into small, apparently safe dimensions.
fn parameters<const N: usize>(bytes: &[u8], at: &mut usize) -> Option<([u32; N], usize)> {
    let mut values = [0u32; N];
    let mut count = 1;
    loop {
        match bytes.get(*at) {
            Some(0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f) => *at += 1,
            Some(b'0'..=b'9') => {
                values[count - 1] =
                    values[count - 1].checked_mul(10)?.checked_add(u32::from(bytes[*at] - b'0'))?;
                *at += 1;
            },
            Some(b';') if count < N => {
                count += 1;
                *at += 1;
            },
            Some(b';') => return None,
            _ => return Some((values, count)),
        }
    }
}

fn parse(bytes: &[u8], mut output: Option<(&mut Vec<u8>, usize)>) -> Option<Info> {
    let body = bytes.strip_prefix(b"\x1bP")?.strip_suffix(b"\x1b\\")?;
    let mut at = 0;
    let (header, _) = parameters::<3>(body, &mut at)?;
    if body.get(at) != Some(&b'q') || header[1] > 2 {
        return None;
    }
    at += 1;
    let mut info = Info {
        width: 0,
        height: 0,
        aspect: (
            match header[0] {
                0 | 1 | 5 | 6 => 2,
                2 => 5,
                3 | 4 => 3,
                _ => 1,
            },
            1,
        ),
        transparent: header[1] == 1,
    };
    let mut palette = default_palette();
    let mut color = 0;
    let (mut x, mut y, mut work) = (0usize, 0usize, 0usize);
    while at < body.len() {
        let byte = body[at];
        at += 1;
        match byte {
            b'?'..=b'~' | b'!' => {
                let (bits, repeat) = if byte == b'!' {
                    let (values, _) = parameters::<1>(body, &mut at)?;
                    let ch = *body.get(at)?;
                    if !(b'?'..=b'~').contains(&ch) {
                        return None;
                    }
                    at += 1;
                    (ch - b'?', values[0].max(1) as usize)
                } else {
                    (byte - b'?', 1)
                };
                let end = x.checked_add(repeat)?;
                work = work.checked_add(repeat.checked_mul(6)?)?;
                let painted_height = (u8::BITS - bits.leading_zeros()) as usize;
                if end > MAX_EDGE || y + painted_height > MAX_EDGE || work > MAX_PAINTED_PIXELS {
                    return None;
                }
                info.width = info.width.max(end);
                // Opaque zero bits still fill a six-pixel background band.
                // Only unused background is clipped at the advertised edge;
                // a set bit beyond it is rejected above.
                let height =
                    if info.transparent { y + painted_height } else { (y + 6).min(MAX_EDGE) };
                info.height = info.height.max(height);
                if let Some((pixels, stride)) = &mut output {
                    for row in 0..6 {
                        if bits & (1 << row) == 0 {
                            continue;
                        }
                        let start = ((y + row) * *stride + x) * 4;
                        let end = start + repeat * 4;
                        for pixel in pixels.get_mut(start..end)?.chunks_exact_mut(4) {
                            pixel.copy_from_slice(&palette[color]);
                        }
                    }
                }
                x = end;
            },
            b'$' => x = 0,
            b'-' => {
                y = y.checked_add(6)?;
                if y > MAX_EDGE {
                    return None;
                }
                x = 0;
            },
            b'"' => {
                let (values, count) = parameters::<4>(body, &mut at)?;
                if count < 2 {
                    return None;
                }
                info.aspect = (values[0].max(1), values[1].max(1));
                info.width = info.width.max(values[2] as usize);
                info.height = info.height.max(values[3] as usize);
            },
            b'#' => {
                let (values, count) = parameters::<5>(body, &mut at)?;
                color = values[0] as usize;
                if color >= COLOR_REGISTERS {
                    return None;
                }
                if count == 5 {
                    palette[color] = match values[1] {
                        1 if values[2] <= 360 && values[3] <= 100 && values[4] <= 100 => {
                            hls(values[2], values[3], values[4])
                        },
                        2 if values[2..].iter().all(|n| *n <= 100) => {
                            [percent(values[2]), percent(values[3]), percent(values[4]), 255]
                        },
                        _ => return None,
                    };
                } else if count != 1 {
                    return None;
                }
            },
            // Embedded formatting controls do not move the SIXEL pen.
            0x00..=0x17 | 0x19 | 0x1c..=0x20 | 0x7f => (),
            _ => return None,
        }
        if info.width > MAX_EDGE
            || info.height > MAX_EDGE
            || info.width.checked_mul(info.height)? > MAX_PIXELS
        {
            return None;
        }
    }
    let display_height = u64::try_from(info.height)
        .ok()?
        .checked_mul(u64::from(info.aspect.0))?
        .div_ceil(u64::from(info.aspect.1));
    (info.width > 0 && info.height > 0 && display_height <= MAX_EDGE as u64).then_some(info)
}

fn percent(value: u32) -> u8 {
    ((value * 255 + 50) / 100) as u8
}

fn default_palette() -> [[u8; 4]; COLOR_REGISTERS] {
    // DEC's sixteen default registers, followed by a 6x6x6 cube and gray ramp.
    let base = [
        [0, 0, 0],
        [20, 20, 80],
        [80, 13, 13],
        [20, 80, 20],
        [80, 20, 80],
        [20, 80, 80],
        [80, 80, 20],
        [53, 53, 53],
        [26, 26, 26],
        [33, 33, 60],
        [60, 26, 26],
        [33, 60, 33],
        [60, 33, 60],
        [33, 60, 60],
        [60, 60, 33],
        [80, 80, 80],
    ];
    std::array::from_fn(|index| {
        let rgb = if index < 16 {
            base[index]
        } else if index < 232 {
            let n = (index - 16) as u32;
            [n / 36 * 20, n / 6 % 6 * 20, n % 6 * 20]
        } else {
            [((index - 232) * 100 / 23) as u32; 3]
        };
        [percent(rgb[0]), percent(rgb[1]), percent(rgb[2]), 255]
    })
}

fn hls(hue: u32, lightness: u32, saturation: u32) -> [u8; 4] {
    // DEC hue zero is blue, 120 is red and 240 is green.
    let h = ((hue + 240) % 360) as f32 / 60.0;
    let l = lightness as f32 / 100.0;
    let s = saturation as f32 / 100.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let rgb = match h as u32 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    let m = l - c / 2.0;
    [
        ((rgb[0] + m) * 255.0).round() as u8,
        ((rgb[1] + m) * 255.0).round() as u8,
        ((rgb[2] + m) * 255.0).round() as u8,
        255,
    ]
}
