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

pub(super) fn single_query(bytes: &[u8]) -> bool {
    value(bytes, b'a') == Some(b"q") && value(bytes, b'm') != Some(b"1")
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
    pub placement: Option<u32>,
    pub quiet: u32,
}

impl Reply {
    pub fn read(control: &[u8]) -> Self {
        Self {
            id: number(control, b'i').ok().flatten().filter(|id| *id != 0),
            placement: number(control, b'p').ok().flatten().filter(|id| *id != 0),
            quiet: number(control, b'q').ok().flatten().filter(|q| *q <= 2).unwrap_or(0),
        }
    }

    pub fn message(self, features: Option<&str>, result: Result<()>) -> Option<String> {
        let id = self.id?;
        if self.quiet >= 2 || self.quiet == 1 && result.is_ok() {
            return None;
        }
        let placement = self.placement.map(|id| format!(",p={id}")).unwrap_or_default();
        let features = features.map(|features| format!(",{features}")).unwrap_or_default();
        let status = result.err().unwrap_or("OK");
        Some(format!("\x1b_Gi={id}{placement}{features};{status}\x1b\\"))
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
    /// Number of newest images a `d=n` delete asks for (`I`).
    pub count: u32,
    /// Verbatim control string, needed to echo unsupported-by-default query keys.
    query: Vec<u8>,
}

impl Control {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 1024 {
            return Err("E2BIG");
        }
        let mut seen = [false; 128];
        for pair in bytes.split(|byte| *byte == b',').filter(|pair| !pair.is_empty()) {
            let key = *pair.first().ok_or("EINVAL")? as usize;
            if key >= seen.len() || pair.get(1) != Some(&b'=') || seen[key] {
                return Err("EINVAL");
            }
            seen[key] = true;
            if !b"aipqmsvfcdrxywhXYCtoUzIe".contains(&(key as u8)) {
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
        let probing = feature.is_some();
        if !probing
            && (!matches!(value(bytes, b't'), None | Some(b"d"))
                || value(bytes, b'o').is_some()
                || number(bytes, b'U')?.unwrap_or(0) != 0)
        {
            return Err("ENOTSUP");
        }
        // `z` only describes how the payload is packed when `o=z` is present, and
        // readers commonly send `z=-1` to mean "nothing to undo" — a signed value
        // the other numeric keys never are, so it is deliberately not parsed.
        // The payload is checked against the declared size in `decode` anyway.
        let (channels, format) = match number(bytes, b'f')?.unwrap_or(32) {
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
        let cursor = number(bytes, b'C')?.unwrap_or(0);
        if more > 1 || cursor > 1 {
            return Err("EINVAL");
        }
        let reply = Reply {
            id: number(bytes, b'i')?.filter(|id| *id != 0),
            placement: number(bytes, b'p')?.filter(|id| *id != 0),
            quiet: quiet.unwrap_or(0),
        };
        let selector = match value(bytes, b'd') {
            None => b'a',
            Some([selector]) => *selector,
            _ => return Err("EINVAL"),
        };
        Ok(Self {
            action,
            reply,
            quiet,
            more: more == 1,
            width: number(bytes, b's')?,
            height: number(bytes, b'v')?,
            channels,
            format,
            selector,
            columns: number(bytes, b'c')?.filter(|n| *n != 0),
            rows: number(bytes, b'r')?.filter(|n| *n != 0),
            x: number(bytes, b'x')?.unwrap_or(0),
            y: number(bytes, b'y')?.unwrap_or(0),
            crop_width: number(bytes, b'w')?.filter(|n| *n != 0),
            crop_height: number(bytes, b'h')?.filter(|n| *n != 0),
            offset_x: number(bytes, b'X')?.unwrap_or(0),
            offset_y: number(bytes, b'Y')?.unwrap_or(0),
            keep_cursor: cursor == 1,
            feature,
            count: number(bytes, b'I')?.unwrap_or(0),
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
        for key in [b'C', b'U', b'X', b'Y', b'c', b'r', b'x', b'y', b'w', b'h', b't', b'f', b'd'] {
            let Some(value) = value(&self.query, key) else { continue };
            let supported = match key {
                b'C' => matches!(value, b"0" | b"1"),
                b'U' => value == b"0",
                b't' => value == b"d",
                b'f' => matches!(value, b"24" | b"32" | b"100"),
                b'd' => Self::selector_supported(value),
                _ => std::str::from_utf8(value).is_ok_and(|text| text.parse::<u32>().is_ok()),
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
        origin: (usize, usize),
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
            column: origin.0,
            row: origin.1,
        }))
    }
}
