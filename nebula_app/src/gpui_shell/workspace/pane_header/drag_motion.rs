//! Visual offsets for the existing pane exchange gesture.

use super::*;
use gpui::{Point, point};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const SLIDE: Duration = Duration::from_millis(150);

#[derive(Default)]
pub(super) struct PaneDragMotion {
    slides: HashMap<u64, Slide>,
}

struct Slide {
    start: Instant,
    from: Point<Pixels>,
    to: Point<Pixels>,
    instant: bool,
}

#[derive(Clone, Copy)]
pub(super) struct PanePreview {
    pub(super) offset: Point<Pixels>,
    pub(super) running: bool,
    pub(super) priority: usize,
}

impl Slide {
    fn sample(&self, now: Instant) -> (Point<Pixels>, bool) {
        let progress =
            now.saturating_duration_since(self.start).as_secs_f32() / SLIDE.as_secs_f32();
        if self.instant || progress >= 1.0 {
            return (self.to, false);
        }
        let eased = 1.0 - (1.0 - progress).powi(3);
        (self.from + (self.to - self.from) * eased, true)
    }
}

impl PaneDragMotion {
    pub(super) fn change_target(
        &mut self,
        previous: Option<u64>,
        next: Option<(u64, Point<Pixels>)>,
        now: Instant,
        instant: bool,
    ) {
        self.slides.retain(|id, slide| Some(*id) == previous || slide.sample(now).1);
        let mut slide_to = |id, to| {
            let from = self.slides.get(&id).map_or(Point::default(), |slide| slide.sample(now).0);
            self.slides.insert(id, Slide { start: now, from, to, instant });
        };
        if let Some(previous) = previous {
            slide_to(previous, Point::default());
        }
        if let Some((id, offset)) = next {
            slide_to(id, offset);
        }
    }
}

impl NebulaWorkspace {
    pub(super) fn pane_drag_preview(&self, id: u64, cx: &App) -> Option<PanePreview> {
        let drag = self.pane_drag.as_ref().filter(|drag| drag.active)?;
        if id == drag.pane {
            return Some(PanePreview {
                offset: point(px(drag.x - drag.press_x), px(drag.y - drag.press_y)),
                running: false,
                priority: 2,
            });
        }
        let (offset, running) = drag.motion.slides.get(&id)?.sample(cx.background_executor().now());
        (running || offset != Point::default()).then_some(PanePreview {
            offset,
            running,
            priority: 1,
        })
    }
}
