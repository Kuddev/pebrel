//! Shared timing for the confirmed input-focus and theme-reveal transitions.

use gpui::{Animation, AnimationExt as _, Hsla, IntoElement as _, Styled as _, div, px, relative};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
pub(super) struct FocusUnderline {
    focused: bool,
    generation: usize,
    from: f32,
    progress: Rc<Cell<f32>>,
}

impl FocusUnderline {
    pub(super) fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.from = self.progress.get();
            self.focused = focused;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub(super) fn underline(&self, color: Hsla) -> gpui::AnyElement {
        let target = if self.focused { 1.0 } else { 0.0 };
        let line = div().absolute().bottom_0().h(px(1.0)).bg(color);
        if self.from == target {
            return line
                .left(relative((1.0 - target) * 0.5))
                .w(relative(target))
                .into_any_element();
        }
        let from = self.from;
        let progress = self.progress.clone();
        line.with_animation(
            ("search-focus-line", self.generation),
            Animation::new(std::time::Duration::from_millis(180))
                .with_easing(crate::gpui_shell::motion::ease),
            move |line, delta| {
                let value = from + (target - from) * delta;
                progress.set(value);
                line.left(relative((1.0 - value) * 0.5)).w(relative(value))
            },
        )
        .into_any_element()
    }
}

/// CSS cubic-bezier(0.85, 0, 0.15, 1), including its time-axis inversion.
pub(super) fn ease(delta: f32) -> f32 {
    let delta = delta.clamp(0.0, 1.0);
    if delta == 0.0 || delta == 1.0 {
        return delta;
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..16 {
        let t = (low + high) * 0.5;
        let u = 1.0 - t;
        let x = 3.0 * 0.85 * u * u * t + 3.0 * 0.15 * u * t * t + t * t * t;
        if x < delta {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) * 0.5;
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::ease;

    #[test]
    fn easing_matches_the_reference_time_axis_and_preserves_endpoints() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.3953125) - 0.15625).abs() < 0.0001);
        assert!((ease(0.6046875) - 0.84375).abs() < 0.0001);
        let samples = (0..=100).map(|step| ease(step as f32 / 100.0)).collect::<Vec<_>>();
        assert!(samples.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
