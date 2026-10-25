//! OSC 1337 image geometry and grid-owned image identity.
//!
//! Cells own the lifetime; renderer queues/caches only keep weak references.
//! Clearing, scrolling, screen swaps and reflow therefore use the same source
//! of truth as text, including when an asynchronous decode finishes late.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use crate::event::WindowSize;

/// Bound grid metadata independently of the encoded/decoded pixel budgets.
const MAX_IMAGE_CELLS: usize = 16 * 1024;
static NEXT_IMAGE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageDimension {
    #[default]
    Auto,
    Cells(u32),
    Pixels(u32),
    Percent(u32),
}

impl ImageDimension {
    fn parse(value: &str) -> Option<Self> {
        if value == "auto" {
            return Some(Self::Auto);
        }
        let (number, unit) = if let Some(number) = value.strip_suffix("px") {
            (number, 1)
        } else if let Some(number) = value.strip_suffix('%') {
            (number, 2)
        } else {
            (value, 0)
        };
        let number = number.parse::<u32>().ok().filter(|number| *number > 0)?;
        Some(match unit {
            1 => Self::Pixels(number),
            2 => Self::Percent(number),
            _ => Self::Cells(number),
        })
    }

    fn pixels(self, cell: f32, viewport: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Cells(value) => Some(value as f32 * cell),
            Self::Pixels(value) => Some(value as f32),
            Self::Percent(value) => Some(viewport * value as f32 / 100.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageOptions {
    pub width: ImageDimension,
    pub height: ImageDimension,
    pub preserve_aspect_ratio: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            width: ImageDimension::Auto,
            height: ImageDimension::Auto,
            preserve_aspect_ratio: true,
        }
    }
}

impl ImageOptions {
    pub fn parse(args: &[u8]) -> Option<Self> {
        let mut options = Self::default();
        for arg in std::str::from_utf8(args).ok()?.split(';') {
            let Some((key, value)) = arg.split_once('=') else { continue };
            match key {
                "width" => options.width = ImageDimension::parse(value)?,
                "height" => options.height = ImageDimension::parse(value)?,
                "preserveAspectRatio" => options.preserve_aspect_ratio = value != "0",
                _ => (),
            }
        }
        Some(options)
    }

    pub fn layout(
        self,
        width: u32,
        height: u32,
        viewport: WindowSize,
        start_column: usize,
    ) -> Option<ImageLayout> {
        let cell_width = f32::from(viewport.cell_width.max(1));
        let cell_height = f32::from(viewport.cell_height.max(1));
        let max_width = f32::from(viewport.num_cols) * cell_width;
        let max_height = f32::from(viewport.num_lines) * cell_height;
        let requested_width = self.width.pixels(cell_width, max_width);
        let requested_height = self.height.pixels(cell_height, max_height);
        let (natural_width, natural_height) = (width as f32, height as f32);
        if width == 0 || height == 0 || viewport.num_cols == 0 {
            return None;
        }
        let (mut box_width, mut box_height) = match (requested_width, requested_height) {
            (None, None) => (natural_width, natural_height),
            (Some(width), None) => (width, natural_height * width / natural_width),
            (None, Some(height)) => (natural_width * height / natural_height, height),
            (Some(width), Some(height)) => (width, height),
        };
        let available_width =
            usize::from(viewport.num_cols).saturating_sub(start_column) as f32 * cell_width;
        if box_width > available_width {
            box_height *= available_width / box_width;
            box_width = available_width;
        }
        let columns = (box_width / cell_width).ceil() as usize;
        let rows = (box_height / cell_height).ceil() as usize;
        if columns == 0 || rows == 0 || columns.checked_mul(rows)? > MAX_IMAGE_CELLS {
            return None;
        }
        let (width, height) = if self.preserve_aspect_ratio {
            let scale = (box_width / natural_width).min(box_height / natural_height);
            (natural_width * scale, natural_height * scale)
        } else {
            (box_width, box_height)
        };
        Some(ImageLayout { width, height, cell_width, cell_height, columns, rows })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImageLayout {
    pub width: f32,
    pub height: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub columns: usize,
    pub rows: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImageIdentity {
    pub id: u64,
}

impl ImageIdentity {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { id: NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed) })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageCell {
    pub image: Arc<ImageIdentity>,
    /// Source tile coordinates; grid edits move these with their cells.
    pub column: usize,
    pub row: usize,
}

#[derive(Clone, Debug)]
pub struct ImagePlacement {
    pub id: u64,
    pub lifetime: Weak<ImageIdentity>,
    pub layout: ImageLayout,
}

impl ImagePlacement {
    pub fn is_alive(&self) -> bool {
        self.lifetime.strong_count() != 0
    }
}

/// A horizontally contiguous visible portion of an image, in cell units.
/// No strong image reference escapes the terminal lock through a snapshot.
#[derive(Clone, Debug)]
pub struct ImageRun {
    pub id: u64,
    pub row: u16,
    pub column: u16,
    pub source_row: usize,
    pub source_column: usize,
    pub columns: u16,
}

pub fn push_image_run(runs: &mut Vec<ImageRun>, row: u16, column: u16, tile: &ImageCell) {
    if let Some(last) = runs.last_mut() {
        if last.id == tile.image.id
            && last.row == row
            && last.column + last.columns == column
            && last.source_row == tile.row
            && last.source_column + usize::from(last.columns) == tile.column
        {
            last.columns += 1;
            return;
        }
    }
    runs.push(ImageRun {
        id: tile.image.id,
        row,
        column,
        source_row: tile.row,
        source_column: tile.column,
        columns: 1,
    });
}
