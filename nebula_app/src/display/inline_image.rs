//! Adapt grid-owned image fragments to the legacy OpenGL renderer.

use std::sync::Arc;

use nebula_terminal::event::EventListener;
use nebula_terminal::grid::Dimensions;
use nebula_terminal::inline_image::push_image_run;
use nebula_terminal::term::{Term, point_to_viewport_from};

use super::{NebulaInlineImage, SizeInfo};

pub(super) struct ImageDraw {
    pub id: u64,
    pub rgba: Arc<Vec<u8>>,
    pub pixels: (u32, u32),
    pub bounds: (f32, f32, f32, f32),
    pub clip: (f32, f32, f32, f32),
}

pub(super) fn capture<T: EventListener>(
    term: &Term<T>,
    images: &mut Vec<NebulaInlineImage>,
    view: &SizeInfo,
) -> Vec<ImageDraw> {
    images.retain(|image| image.placement.is_alive());
    if images.is_empty() {
        return Vec::new();
    }
    let content = term.renderable_content_with_viewport(view.screen_lines(), view.columns());
    let mut runs = Vec::new();
    for indexed in content.display_iter {
        let Some(tile) = indexed.cell.image() else { continue };
        let Some(point) = point_to_viewport_from(content.viewport_origin, indexed.point) else {
            continue;
        };
        push_image_run(&mut runs, point.line as u16, point.column.0 as u16, tile);
    }
    runs.iter()
        .filter_map(|run| {
            let image = images.iter().find(|image| image.placement.id == run.id)?;
            let layout = image.placement.layout;
            let cell_w = view.cell_width();
            let cell_h = view.cell_height();
            let x = view.padding_x() + f32::from(run.column) * cell_w;
            let y = view.padding_y() + f32::from(run.row) * cell_h;
            Some(ImageDraw {
                id: image.placement.id,
                rgba: image.rgba.clone(),
                pixels: (image.px_w, image.px_h),
                bounds: (
                    x - run.source_column as f32 * cell_w,
                    y - run.source_row as f32 * cell_h,
                    layout.width / layout.cell_width * cell_w,
                    layout.height / layout.cell_height * cell_h,
                ),
                clip: (x, y, f32::from(run.columns) * cell_w, cell_h),
            })
        })
        .collect()
}
