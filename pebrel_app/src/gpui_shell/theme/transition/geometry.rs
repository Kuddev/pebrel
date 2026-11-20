/// Cover the region outside a central circle with non-overlapping image crops.
/// Sample at device-pixel centres and merge rows with the same snapped edge.
/// The screenshot is never copied per frame.
pub(super) fn for_each_visible_rect(
    width: f32,
    height: f32,
    radius: f32,
    scale: f32,
    mut paint: impl FnMut(f32, f32, f32, f32),
) {
    if width <= 0.0 || height <= 0.0 || scale <= 0.0 {
        return;
    }
    if radius <= 0.0 {
        paint(0.0, 0.0, width, height);
        return;
    }
    if radius >= width.hypot(height) * 0.5 {
        return;
    }
    let rows = (height * scale).ceil() as u32;
    let first = (((height * 0.5 - radius) * scale).floor().max(0.0) as u32).min(rows);
    let last = (((height * 0.5 + radius) * scale).ceil().max(0.0) as u32).min(rows);
    let top = first as f32 / scale;
    let bottom = (last as f32 / scale).min(height);
    if top > 0.0 {
        paint(0.0, 0.0, width, top);
    }
    if bottom < height {
        paint(0.0, bottom, width, height - bottom);
    }
    let mut run_edge = 0.0;
    let mut run_top = top;
    let mut run_bottom = top;
    let mut flush = |edge: f32, y: f32, end: f32| {
        if edge > 0.0 && end > y {
            paint(0.0, y, edge, end - y);
            paint(width - edge, y, edge, end - y);
        }
    };
    for row in first..last {
        let y = row as f32 / scale;
        let h = (1.0 / scale).min(height - y);
        let dy = y + h * 0.5 - height * 0.5;
        let half = (radius * radius - dy * dy).max(0.0).sqrt();
        let edge = (((width * 0.5 - half).max(0.0) * scale).round() / scale).min(width * 0.5);
        if edge != run_edge {
            flush(run_edge, run_top, run_bottom);
            run_edge = edge;
            run_top = y;
        }
        run_bottom = y + h;
    }
    flush(run_edge, run_top, run_bottom);
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
