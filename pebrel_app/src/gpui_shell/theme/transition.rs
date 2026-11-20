//! Window-owned day/night reveal: one old client frame, one live new theme.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, IntoElement, Pixels, RenderImage, Size,
    Styled as _, Window, canvas, point, px, size,
};

mod geometry;
use geometry::for_each_visible_rect;

const DURATION: Duration = Duration::from_millis(220);

#[derive(Default)]
pub(crate) struct ThemeTransition {
    light: Option<bool>,
    generation: usize,
    snapshot: Option<Snapshot>,
}

struct Snapshot {
    image: Arc<RenderImage>,
    viewport: Size<Pixels>,
    scale: f32,
    progress: Rc<Cell<f32>>,
    failed: Rc<Cell<bool>>,
}

impl ThemeTransition {
    pub(crate) fn clear(&mut self, window: &mut Window) {
        if let Some(snapshot) = self.snapshot.take() {
            let _ = window.drop_image(snapshot.image);
        }
    }

    pub(crate) fn render(&mut self, window: &mut Window, cx: &App) -> Option<AnyElement> {
        let light = super::resolved_skin(cx).is_light;
        let changed = self.light.replace(light).is_some_and(|previous| previous != light);
        if cx.reduce_motion()
            || !window.is_window_active()
            || !crate::platform::window_capture::supported()
        {
            self.clear(window);
            return None;
        }
        if changed {
            // Capture before replacing the previous texture, so rapid toggles
            // start from the currently displayed composite rather than jumping.
            let frame = crate::platform::window_capture::capture(window);
            self.clear(window);
            match frame {
                Ok(frame) => {
                    let viewport = window.viewport_size();
                    let scale = window.scale_factor();
                    if (f32::from(viewport.width) * scale).round() as u32 != frame.width
                        || (f32::from(viewport.height) * scale).round() as u32 != frame.height
                    {
                        return None;
                    }
                    // GPUI RenderImage consumes BGRA, already supplied by the DIB.
                    let Some(pixels) =
                        image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
                    else {
                        return None;
                    };
                    self.generation = self.generation.wrapping_add(1);
                    self.snapshot = Some(Snapshot {
                        image: Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
                        viewport,
                        scale,
                        progress: Rc::default(),
                        failed: Rc::default(),
                    });
                },
                Err(error) => log::debug!("Theme reveal unavailable: {error}"),
            }
        }
        let snapshot = self.snapshot.as_ref()?;
        if snapshot.progress.get() >= 1.0
            || snapshot.failed.get()
            || snapshot.viewport != window.viewport_size()
            || snapshot.scale != window.scale_factor()
        {
            self.clear(window);
            return None;
        }
        let image = Arc::downgrade(&snapshot.image);
        let progress = snapshot.progress.clone();
        let phase = progress.clone();
        let failed = snapshot.failed.clone();
        let scale = snapshot.scale;
        let overlay = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let delta = phase.get();
                if delta >= 1.0 {
                    // One final frame releases the CPU pixels and atlas entry.
                    window.request_animation_frame();
                    return;
                }
                let Some(image) = image.upgrade() else {
                    return;
                };
                let width = f32::from(bounds.size.width);
                let height = f32::from(bounds.size.height);
                let radius = width.hypot(height) * 0.5 * delta;
                for_each_visible_rect(width, height, radius, scale, |x, y, w, h| {
                    if failed.get() {
                        return;
                    }
                    let rect = Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w), px(h)));
                    if window
                        .paint_image(rect, bounds, Default::default(), image.clone(), 0, false)
                        .is_err()
                    {
                        failed.set(true);
                    }
                });
                if failed.get() {
                    window.request_animation_frame();
                }
            },
        )
        .absolute()
        .size_full()
        .with_animation(
            ("theme-reveal", self.generation),
            Animation::new(DURATION).with_easing(crate::gpui_shell::motion::ease),
            move |overlay, delta| {
                progress.set(delta);
                overlay
            },
        );
        // Above theme pickers and dialog layers; canvas adds no input hitbox.
        Some(gpui::deferred(overlay).with_priority(12).into_any_element())
    }
}
