//! Grid-owned image identity and visible fragments.
//!
//! Cells own the lifetime; snapshots and renderer placements keep no strong
//! reference, so grid edits remain the authority for surviving image tiles.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

static NEXT_IMAGE_ID: AtomicU64 = AtomicU64::new(1);

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
