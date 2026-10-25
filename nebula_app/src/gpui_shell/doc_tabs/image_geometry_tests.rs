use super::*;
use gpui::{AppContext as _, Modifiers, TestAppContext, VisualTestContext, point, size};
use gpui_component::Root;

fn draw(window: &mut VisualTestContext) {
    window.run_until_parked();
    window.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn decoded_png_dimensions_enable_real_pointer_zoom_and_pan(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("actual.png");
    image::RgbaImage::from_pixel(200, 200, image::Rgba([50, 80, 120, 255])).save(&path).unwrap();
    let mut image = None;
    let (_, mut window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ImageTabView::new(path, cx));
        image = Some(view.clone());
        Root::new(view, window, cx)
    });
    window.simulate_resize(size(px(800.0), px(600.0)));
    draw(&mut window);
    let image = image.unwrap();
    assert_eq!(
        image.read_with(window, |view, _| view.geometry_snapshot_for_test().0),
        Some((200, 200)),
        "metadata must come from the actual PNG decoder"
    );
    let position = image.read_with(window, |view, _| view.area.borrow().center());
    window.simulate_mouse_move(position, None, Modifiers::default());
    window.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(160.0))),
        touch_phase: gpui::TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(&mut window);
    window.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(
        position + point(px(30.0), px(25.0)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    window.simulate_mouse_up(
        position + point(px(30.0), px(25.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(&mut window);
    image.read_with(window, |view, _| {
        let geometry = view.geometry_snapshot_for_test();
        assert!(view.image.is_some() && view.error.is_none());
        assert!(geometry.1 > 1.0, "wheel input must zoom the decoded image");
        assert_ne!(geometry.2, (0.0, 0.0), "pointer input must pan the zoomed image");
        assert!(!view.geometry.dragging());
    });
}
