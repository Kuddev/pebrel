//! Graphics controls, protocol replies and placement geometry; no I/O or retained pixels.

use crate::event::WindowSize;
use crate::inline_image::{ImageFormat, ImageIdentity, KittyPlacement, MAX_IMAGE_CELLS};
use std::sync::Arc;

pub(super) type Result<T> = std::result::Result<T, &'static str>;

fn value(control: &[u8], key: u8) -> Option<&[u8]> {
    control.split(|byte| *byte == b',').find_map(|pair| {
        (pair.first() == Some(&key) && pair.get(1) == Some(&b'=')).then(|| &pair[2..])
    })
}

pub(crate) fn is_feature_query(bytes: &[u8]) -> bool {
    bytes.len() <= 1024 && value(bytes, b'a') == Some(b"q") && value(bytes, b'e').is_some()
}

pub(super) fn single_query(bytes: &[u8]) -> bool {
    value(bytes, b'a') == Some(b"q")
        && (value(bytes, b'e').is_some() || value(bytes, b'm') != Some(b"1"))
}

pub(super) fn is_delete(bytes: &[u8]) -> bool {
    value(bytes, b'a') == Some(b"d")
}

fn number(control: &[u8], key: u8) -> Result<Option<u32>> {
    value(control, key)
        .map(|bytes| {
            std::str::from_utf8(bytes).ok().and_then(|text| text.parse().ok()).ok_or("EINVAL")
        })
        .transpose()
}

#[derive(Clone, Copy, Default)]
pub(super) struct Reply {
    pub id: Option<u32>,
    pub number: Option<u32>,
    pub placement: Option<u32>,
    pub quiet: u32,
}

impl Reply {
    pub fn read(control: &[u8]) -> Self {
        Self {
            id: number(control, b'i').ok().flatten().filter(|id| *id != 0),
            number: number(control, b'I').ok().flatten().filter(|id| *id != 0),
            placement: number(control, b'p').ok().flatten().filter(|id| *id != 0),
            quiet: number(control, b'q').ok().flatten().filter(|q| *q <= 2).unwrap_or(0),
        }
    }

    pub fn message(self, features: Option<&str>, result: Result<()>) -> Option<String> {
        let identity = match (self.id, self.number) {
            (Some(id), Some(number)) => format!("i={id},I={number}"),
            (Some(id), None) => format!("i={id}"),
            (None, Some(number)) => format!("I={number}"),
            (None, None) => return None,
        };
        if self.quiet >= 2 || self.quiet == 1 && result.is_ok() {
            return None;
        }
        let placement = self.placement.map(|id| format!(",p={id}")).unwrap_or_default();
        let features = features.map(|features| format!(",{features}")).unwrap_or_default();
        let status = result.err().unwrap_or("OK");
        Some(format!("\x1b_G{identity}{placement}{features};{status}\x1b\\"))
    }
}

#[derive(Clone)]
pub(super) struct Control {
    pub action: Option<u8>,
    pub reply: Reply,
    pub quiet: Option<u32>,
    pub more: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Byte count per pixel for raw payloads; zero when `format` is [`ImageFormat::Png`].
    pub channels: u8,
    pub format: ImageFormat,
    pub format_specified: bool,
    pub selector: u8,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    pub x: u32,
    pub y: u32,
    pub crop_width: Option<u32>,
    pub crop_height: Option<u32>,
    pub offset_x: u32,
    pub offset_y: u32,
    pub keep_cursor: bool,
    /// `e=<action>` of a feature query, kept so the reply can echo its keys.
    pub feature: Option<u8>,
    /// Verbatim control string, needed to echo unsupported-by-default query keys.
    query: Vec<u8>,
}

impl Control {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 1024 {
            return Err("E2BIG");
        }
        let probing = is_feature_query(bytes);
        let mut seen = [false; 128];
        for pair in bytes.split(|byte| *byte == b',').filter(|pair| !pair.is_empty()) {
            let key = *pair.first().ok_or("EINVAL")? as usize;
            if key >= seen.len() || pair.get(1) != Some(&b'=') || seen[key] {
                return Err("EINVAL");
            }
            seen[key] = true;
            if !probing && !b"aipqmsvfcdrxywhXYCtoUzIe".contains(&(key as u8)) {
                return Err("ENOTSUP");
            }
        }
        let action = value(bytes, b'a')
            .map(|value| if value.len() != 1 { Err("EINVAL") } else { Ok(value[0]) })
            .transpose()?;
        if !matches!(action, None | Some(b't' | b'T' | b'p' | b'q' | b'd')) {
            return Err("ENOTSUP");
        }
        // `e` names the action a feature query asks about, so it is only allowed
        // on a query action, where it never carries or applies image data.
        let feature = match value(bytes, b'e') {
            None => None,
            Some([feature]) => Some(*feature),
            Some(_) => return Err("EINVAL"),
        };
        if feature.is_some() && action != Some(b'q') {
            return Err("EINVAL");
        }
        // A feature query never acts, so the keys it names are answered by the
        // reply rather than narrowed to the subset this terminal accepts.
        let numeric =
            |key| if probing { Ok(number(bytes, key).ok().flatten()) } else { number(bytes, key) };
        if !probing
            && (!matches!(value(bytes, b't'), None | Some(b"d"))
                || value(bytes, b'o').is_some()
                || number(bytes, b'U')?.unwrap_or(0) != 0)
        {
            return Err("ENOTSUP");
        }
        // `z` is the signed placement layer; `o=z` independently selects zlib
        // compression. Accept the reader's z=-1 in the existing image overlay
        // order. Independent layer sorting is not advertised by feature queries.
        if let Some(layer) = value(bytes, b'z').filter(|_| !probing) {
            std::str::from_utf8(layer)
                .ok()
                .and_then(|text| text.parse::<i32>().ok())
                .ok_or("EINVAL")?;
        }
        let (channels, format) = match numeric(b'f')?.unwrap_or(32) {
            24 => (3, ImageFormat::Raw),
            32 => (4, ImageFormat::Raw),
            // PNG carries its own geometry, so the pixel budget is checked after
            // the container header is decoded.
            100 => (0, ImageFormat::Png),
            _ if probing => (4, ImageFormat::Raw),
            _ => return Err("ENOTSUP"),
        };
        let quiet = number(bytes, b'q')?;
        if quiet.is_some_and(|quiet| quiet > 2) {
            return Err("EINVAL");
        }
        let more = number(bytes, b'm')?.unwrap_or(0);
        let cursor = numeric(b'C')?.unwrap_or(0);
        if more > 1 || !probing && cursor > 1 {
            return Err("EINVAL");
        }
        let image_number = number(bytes, b'I')?;
        if image_number == Some(0) || image_number.is_some() && value(bytes, b'i').is_some() {
            return Err("EINVAL");
        }
        let reply = Reply {
            id: number(bytes, b'i')?.filter(|id| *id != 0),
            number: image_number,
            placement: number(bytes, b'p')?.filter(|id| *id != 0),
            quiet: quiet.unwrap_or(0),
        };
        if action == Some(b'q') && reply.id.is_none() {
            return Err("EINVAL");
        }
        let selector = match value(bytes, b'd') {
            None => b'a',
            Some([selector]) => *selector,
            _ if probing => 0,
            _ => return Err("EINVAL"),
        };
        Ok(Self {
            action,
            reply,
            quiet,
            more: more == 1,
            width: numeric(b's')?,
            height: numeric(b'v')?,
            channels,
            format,
            format_specified: value(bytes, b'f').is_some(),
            selector,
            columns: numeric(b'c')?.filter(|n| *n != 0),
            rows: numeric(b'r')?.filter(|n| *n != 0),
            x: numeric(b'x')?.unwrap_or(0),
            y: numeric(b'y')?.unwrap_or(0),
            crop_width: numeric(b'w')?.filter(|n| *n != 0),
            crop_height: numeric(b'h')?.filter(|n| *n != 0),
            offset_x: numeric(b'X')?.unwrap_or(0),
            offset_y: numeric(b'Y')?.unwrap_or(0),
            keep_cursor: cursor == 1,
            feature,
            query: if feature.is_some() { bytes.to_vec() } else { Vec::new() },
        })
    }

    /// The action a feature query asks about; `None` means this terminal does not
    /// implement it, and the reply must then omit `e` entirely.
    pub fn queried_action(&self) -> Option<u8> {
        let feature = self.feature?;
        matches!(feature, b't' | b'T' | b'p' | b'd').then_some(feature)
    }

    /// Echoes the query keys this terminal honours for the queried action, in the
    /// `key=value` form the reply expects; unsupported keys stay absent.
    pub fn features(&self, action: u8) -> String {
        let mut reply = format!("e={}", char::from(action));
        for key in [
            b'p', b'C', b'U', b'X', b'Y', b'c', b'r', b'x', b'y', b'w', b'h', b's', b'v', b't',
            b'f', b'd',
        ] {
            let Some(value) = value(&self.query, key) else { continue };
            let placing = matches!(action, b'T' | b'p');
            let supported = match key {
                b'p' => {
                    self.reply.placement.is_some()
                        && (placing
                            || action == b'd'
                                && matches!(self.selector.to_ascii_lowercase(), b'i' | b'n'))
                },
                b'C' => placing && value == b"1",
                b'U' | b't' => false,
                b'f' => matches!(action, b't' | b'T') && matches!(value, b"24" | b"100"),
                b'd' => action == b'd' && value != b"a" && Self::selector_supported(value),
                b's' | b'v' => {
                    matches!(action, b't' | b'T')
                        && self.format == ImageFormat::Raw
                        && std::str::from_utf8(value)
                            .is_ok_and(|text| text.parse::<u32>().is_ok_and(|number| number != 0))
                },
                _ => {
                    let selector = self.selector.to_ascii_lowercase();
                    let cell_parameter = key == b'x' && matches!(selector, b'p' | b'r' | b'x')
                        || key == b'y' && matches!(selector, b'p' | b'r' | b'y');
                    let relevant = placing || action == b'd' && cell_parameter;
                    relevant
                        && std::str::from_utf8(value)
                            .is_ok_and(|text| text.parse::<u32>().is_ok_and(|number| number != 0))
                },
            };
            if !supported {
                continue;
            }
            reply.push(',');
            reply.push(char::from(key));
            reply.push('=');
            reply.push_str(&String::from_utf8_lossy(value));
        }
        reply
    }

    fn selector_supported(value: &[u8]) -> bool {
        match value {
            [selector] => matches!(
                selector.to_ascii_lowercase(),
                b'a' | b'i' | b'p' | b'c' | b'x' | b'y' | b'r' | b'n'
            ),
            _ => false,
        }
    }

    pub fn dimensions(&self) -> Result<(u32, u32, usize)> {
        let (width, height) = (self.width.ok_or("EINVAL")?, self.height.ok_or("EINVAL")?);
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 {
            return Err("EINVAL");
        }
        if pixels > super::MAX_PIXELS as u64 {
            return Err("E2BIG");
        }
        Ok((width, height, pixels as usize))
    }

    pub fn placement(
        &self,
        width: u32,
        height: u32,
        viewport: WindowSize,
        resource: Arc<ImageIdentity>,
        identity: &Arc<ImageIdentity>,
    ) -> Result<Arc<KittyPlacement>> {
        if self.x >= width || self.y >= height {
            return Err("EINVAL");
        }
        let source_width = self.crop_width.unwrap_or(width - self.x).min(width - self.x) as f32;
        let source_height = self.crop_height.unwrap_or(height - self.y).min(height - self.y) as f32;
        let cw = f32::from(viewport.cell_width.max(1));
        let ch = f32::from(viewport.cell_height.max(1));
        if self.offset_x as f32 >= cw || self.offset_y as f32 >= ch {
            return Err("EINVAL");
        }
        let requested_w = self.columns.map(|n| n as f32 * cw);
        let requested_h = self.rows.map(|n| n as f32 * ch);
        let scale = match (requested_w, requested_h) {
            (Some(w), Some(h)) => (w / source_width).min(h / source_height),
            (Some(w), None) => w / source_width,
            (None, Some(h)) => h / source_height,
            (None, None) => 1.0,
        };
        let width_cells = source_width * scale / cw;
        let height_cells = source_height * scale / ch;
        let offset_x_cells = self.offset_x as f32 / cw;
        let offset_y_cells = self.offset_y as f32 / ch;
        let columns = self
            .columns
            .map(|n| n as usize)
            .unwrap_or((width_cells + offset_x_cells).ceil() as usize);
        let rows = self
            .rows
            .map(|n| n as usize)
            .unwrap_or((height_cells + offset_y_cells).ceil() as usize);
        if columns == 0
            || rows == 0
            || columns.checked_mul(rows).is_none_or(|n| n > MAX_IMAGE_CELLS)
        {
            return Err("ENOSPC");
        }
        Ok(Arc::new(KittyPlacement {
            id: identity.id,
            identity: Arc::downgrade(identity),
            resource,
            source_x: self.x as f32,
            source_y: self.y as f32,
            source_width,
            source_height,
            width_cells,
            height_cells,
            offset_x_cells,
            offset_y_cells,
            columns,
            rows,
        }))
    }
}
