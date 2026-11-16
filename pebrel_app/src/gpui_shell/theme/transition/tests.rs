use super::for_each_visible_rect;

#[test]
fn reveal_keeps_the_old_frame_until_it_starts_and_releases_it_when_complete() {
    let mut rectangles = Vec::new();
    for_each_visible_rect(80.0, 60.0, 0.0, 1.0, |x, y, w, h| {
        rectangles.push((x, y, w, h));
    });
    assert_eq!(rectangles, [(0.0, 0.0, 80.0, 60.0)]);
    rectangles.clear();
    for_each_visible_rect(80.0, 60.0, 50.0, 1.0, |x, y, w, h| {
        rectangles.push((x, y, w, h));
    });
    assert!(rectangles.is_empty());
}

#[test]
fn reveal_crops_cover_only_the_outside_of_the_circle_at_each_dpi() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let width = 80.0;
        let height = 60.0;
        let radius = 20.0;
        let mut rectangles = Vec::new();
        for_each_visible_rect(width, height, radius, scale, |x, y, w, h| {
            assert!(x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0);
            assert!(x + w <= width + 0.001 && y + h <= height + 0.001);
            rectangles.push((x, y, w, h));
        });
        for row in 0..(height * scale) as usize {
            for column in 0..(width * scale) as usize {
                let x = (column as f32 + 0.5) / scale;
                let y = (row as f32 + 0.5) / scale;
                let covering = rectangles
                    .iter()
                    .filter(|&&(rx, ry, w, h)| x >= rx && x < rx + w && y >= ry && y < ry + h)
                    .count();
                let outside = (x - width * 0.5).hypot(y - height * 0.5) >= radius;
                assert_eq!(covering, usize::from(outside), "{scale}: {x}, {y}");
            }
        }
    }
}

#[test]
fn equal_pixel_edges_merge_without_submitting_each_row_separately() {
    let mut rectangles = Vec::new();
    for_each_visible_rect(1920.0, 1080.0, 800.0, 1.0, |x, y, w, h| {
        rectangles.push((x, y, w, h));
    });
    assert!(rectangles.len() < 1080);
    assert!(rectangles.iter().any(|&(_, _, _, height)| height > 1.0));
}
