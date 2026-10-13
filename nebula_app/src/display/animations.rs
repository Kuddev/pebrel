//! 旧渲染入口的窗口动画状态；显示入口只提交目标、读取进度。
//!
//! 同一窗口共享帧快照，避免各绘制器重复取时产生不同步。状态与重置规则
//! 留在这里，命中测试、设置枚举和具体绘制继续由调用方适配。

use std::time::Duration;

use super::{settings, ui};

#[derive(Debug, Clone, Copy)]
struct UiAnim {
    spring: crate::motion::Spring,
}

impl UiAnim {
    fn new(value: f32) -> Self {
        Self { spring: crate::motion::Spring::new(value.clamp(0.0, 1.0)).with_response(0.14) }
    }

    fn value(self) -> f32 {
        self.spring.value().clamp(0.0, 1.0)
    }

    fn visible(self, target_open: bool) -> bool {
        target_open || self.value() > 0.004
    }

    fn animating_to(self, target: f32) -> bool {
        (self.value() - target.clamp(0.0, 1.0)).abs() > 0.004 || self.spring.is_active()
    }

    fn step(&mut self, frame: crate::motion::Frame, target: f32) {
        self.spring.set_target(target.clamp(0.0, 1.0), crate::motion::MotionPolicy::Full);
        self.spring.step(frame);
    }
}

/// Independent motion channels for one settings toggle. The reference HTML
/// animates travel, active stretch, color and hover through different CSS
/// transitions; keeping four Tweens per switch preserves that separation.
#[derive(Debug, Clone, Copy)]
struct SettingsToggleAnim {
    position: crate::motion::Tween,
    stretch: crate::motion::Tween,
    color: crate::motion::Tween,
    hover: crate::motion::Tween,
}

impl SettingsToggleAnim {
    fn new(on: bool) -> Self {
        let value = if on { 1.0 } else { 0.0 };
        Self {
            position: crate::motion::Tween::new(value),
            stretch: crate::motion::Tween::new(0.0),
            color: crate::motion::Tween::new(value),
            hover: crate::motion::Tween::new(0.0),
        }
    }

    fn step(&mut self, frame: crate::motion::Frame, on: bool, pressed: bool, hovered: bool) {
        // The settings input commits the new boolean on mouse-down. The
        // active selector therefore only changes the thumb geometry; it never
        // hides the newly selected track or reverses an already-on switch.
        let position = if on { if pressed { 16.0 / 24.0 } else { 1.0 } } else { 0.0 };
        let color = if on { 1.0 } else { 0.0 };
        let stretch = if pressed { 1.0 } else { 0.0 };
        let hover = if hovered { 1.0 } else { 0.0 };
        const POSITION: Duration = Duration::from_millis(400);
        const STRETCH: Duration = Duration::from_millis(250);
        const COLOR: Duration = Duration::from_millis(300);

        if (self.position.target() - position).abs() > f32::EPSILON {
            self.position.animate_to(
                position,
                POSITION,
                crate::motion::Easing::LiquidToggle,
                crate::motion::MotionPolicy::Full,
            );
        }
        if (self.stretch.target() - stretch).abs() > f32::EPSILON {
            self.stretch.animate_to(
                stretch,
                STRETCH,
                crate::motion::Easing::CssStandard,
                crate::motion::MotionPolicy::Full,
            );
        }
        if (self.color.target() - color).abs() > f32::EPSILON {
            self.color.animate_to(
                color,
                COLOR,
                crate::motion::Easing::CssEase,
                crate::motion::MotionPolicy::Full,
            );
        }
        if (self.hover.target() - hover).abs() > f32::EPSILON {
            self.hover.animate_to(
                hover,
                COLOR,
                crate::motion::Easing::CssEase,
                crate::motion::MotionPolicy::Full,
            );
        }
        self.position.step(frame);
        self.stretch.step(frame);
        self.color.step(frame);
        self.hover.step(frame);
    }

    fn value(self) -> ui::widgets::ToggleMotion {
        ui::widgets::ToggleMotion {
            // Do not clamp position: the supplied cubic-bezier deliberately
            // crosses 0/1 to create the same brief elastic overshoot as CSS.
            position: self.position.value(),
            stretch: self.stretch.value().clamp(0.0, 1.0),
            color: self.color.value().clamp(0.0, 1.0),
            hover: self.hover.value().clamp(0.0, 1.0),
        }
    }

    fn animating_to(self, on: bool, pressed: bool, hovered: bool) -> bool {
        let position = if on { if pressed { 16.0 / 24.0 } else { 1.0 } } else { 0.0 };
        let color = if on { 1.0 } else { 0.0 };
        let stretch = if pressed { 1.0 } else { 0.0 };
        let hover = if hovered { 1.0 } else { 0.0 };
        [
            (self.position, position),
            (self.stretch, stretch),
            (self.color, color),
            (self.hover, hover),
        ]
        .into_iter()
        .any(|(tween, target)| tween.is_active() || (tween.value() - target).abs() > 0.004)
    }
}

#[derive(Debug, Clone)]
pub(super) struct WindowAnimations {
    clock: crate::motion::MotionClock,
    frame: Option<crate::motion::Frame>,
    /// Continuous sidebar-spinner phase in turns (`0.0..1.0`). Advancing it
    /// from the shared monotonic frame delta avoids wall-clock jumps and needs
    /// only four bytes per window.
    spinner_phase: f32,
    left_sidebar: UiAnim,
    right_drawer: UiAnim,
    ssh_editor: UiAnim,
    settings_toggles: [SettingsToggleAnim; settings::SETTINGS_TOGGLE_COUNT],
}

impl WindowAnimations {
    pub(super) fn new() -> Self {
        Self {
            clock: crate::motion::MotionClock::default(),
            frame: None,
            spinner_phase: 0.0,
            left_sidebar: UiAnim::new(1.0),
            right_drawer: UiAnim::new(0.0),
            ssh_editor: UiAnim::new(0.0),
            settings_toggles: std::array::from_fn(|_| SettingsToggleAnim::new(false)),
        }
    }

    pub(super) fn step(
        &mut self,
        left_open: bool,
        right_open: bool,
        ssh_open: bool,
        toggle_targets: [bool; settings::SETTINGS_TOGGLE_COUNT],
        toggle_pressed: Option<usize>,
        toggle_hover: Option<usize>,
    ) {
        let frame = self.clock.tick();
        self.frame = Some(frame);
        self.left_sidebar.step(frame, if left_open { 1.0 } else { 0.0 });
        self.right_drawer.step(frame, if right_open { 1.0 } else { 0.0 });
        self.ssh_editor.step(frame, if ssh_open { 1.0 } else { 0.0 });
        for (index, (anim, target)) in
            self.settings_toggles.iter_mut().zip(toggle_targets).enumerate()
        {
            let pressed = toggle_pressed == Some(index);
            let hovered = toggle_hover == Some(index);
            anim.step(frame, target, pressed, hovered);
        }
    }

    pub(super) fn frame(&mut self) -> crate::motion::Frame {
        if let Some(frame) = self.frame {
            frame
        } else {
            let frame = self.clock.tick();
            self.frame = Some(frame);
            frame
        }
    }

    pub(super) fn animating(
        &self,
        left_open: bool,
        right_open: bool,
        toggle_targets: [bool; settings::SETTINGS_TOGGLE_COUNT],
        toggle_pressed: Option<usize>,
        toggle_hover: Option<usize>,
    ) -> bool {
        self.left_sidebar.animating_to(if left_open { 1.0 } else { 0.0 })
            || self.right_drawer.animating_to(if right_open { 1.0 } else { 0.0 })
            || self.settings_toggles.iter().zip(toggle_targets).enumerate().any(
                |(index, (anim, target))| {
                    anim.animating_to(
                        target,
                        toggle_pressed == Some(index),
                        toggle_hover == Some(index),
                    )
                },
            )
    }

    pub(super) fn sidebar_progress(&self) -> f32 {
        self.left_sidebar.value()
    }

    pub(super) fn sidebar_visible(&self, open: bool) -> bool {
        self.left_sidebar.visible(open)
    }

    pub(super) fn drawer_progress(&self) -> f32 {
        self.right_drawer.value()
    }

    pub(super) fn drawer_visible(&self, open: bool) -> bool {
        self.right_drawer.visible(open)
    }

    pub(super) fn ssh_editor_progress(&self) -> f32 {
        self.ssh_editor.value()
    }

    pub(super) fn ssh_editor_animating(&self, open: bool) -> bool {
        self.ssh_editor.animating_to(if open { 1.0 } else { 0.0 })
    }

    pub(super) fn reset_ssh_editor(&mut self) {
        // 编辑器重开只重置自己的运动，不能重置同一窗口的其他动画或时钟。
        self.ssh_editor = UiAnim::new(0.0);
    }

    pub(super) fn toggle_motion(
        &self,
    ) -> [ui::widgets::ToggleMotion; settings::SETTINGS_TOGGLE_COUNT] {
        std::array::from_fn(|index| self.settings_toggles[index].value())
    }

    pub(super) fn advance_spinner(&mut self, running: bool) -> f32 {
        if running {
            self.spinner_phase = advance_spinner_phase(self.spinner_phase, self.frame().delta);
        }
        self.spinner_phase
    }
}

const SPINNER_PERIOD: std::time::Duration = std::time::Duration::from_millis(800);

#[inline]
fn advance_spinner_phase(phase: f32, delta: std::time::Duration) -> f32 {
    (phase + delta.as_secs_f32() / SPINNER_PERIOD.as_secs_f32()).rem_euclid(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::Frame;
    use std::time::Instant;

    #[test]
    fn toggle_press_and_release_keep_selection_color_and_hover_independent() {
        let frame = Frame { now: Instant::now(), delta: Duration::from_millis(400) };
        let mut toggle = SettingsToggleAnim::new(false);
        toggle.step(frame, true, true, true);
        let pressed = toggle.value();
        assert_eq!(pressed.position, 16.0 / 24.0);
        assert_eq!((pressed.stretch, pressed.color, pressed.hover), (1.0, 1.0, 1.0));
        assert!(!toggle.animating_to(true, true, true));
        assert!(toggle.animating_to(true, false, true));

        toggle.step(frame, true, false, true);
        let released = toggle.value();
        assert_eq!(released.position, 1.0);
        assert_eq!((released.stretch, released.color, released.hover), (0.0, 1.0, 1.0));
        assert!(!toggle.animating_to(true, false, true));

        toggle.step(frame, false, false, false);
        let off = toggle.value();
        assert_eq!((off.position, off.stretch, off.color, off.hover), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn shared_frame_and_unrelated_motion_survive_editor_reset() {
        let mut animations = WindowAnimations::new();
        let mut targets = [false; settings::SETTINGS_TOGGLE_COUNT];
        targets[0] = true;
        animations.step(true, true, true, targets, Some(0), Some(0));
        let frame = animations.frame();
        let drawer = animations.drawer_progress();
        let toggle = animations.toggle_motion()[0];
        assert!(drawer > 0.0);
        assert!(animations.ssh_editor_progress() > 0.0);

        animations.reset_ssh_editor();
        assert_eq!(animations.ssh_editor_progress(), 0.0);
        assert_eq!(animations.sidebar_progress(), 1.0);
        assert_eq!(animations.drawer_progress(), drawer);
        assert_eq!(animations.toggle_motion()[0].position, toggle.position);
        assert_eq!(animations.frame().now, frame.now);
        assert_eq!(animations.frame().delta, frame.delta);
    }

    #[test]
    fn spinner_uses_the_window_frame_and_pauses_without_resetting() {
        let mut animations = WindowAnimations::new();
        let frame = Frame { now: Instant::now(), delta: Duration::from_millis(400) };
        animations.frame = Some(frame);
        assert_eq!(animations.advance_spinner(true), 0.5);
        assert_eq!(animations.advance_spinner(false), 0.5);
        assert_eq!(animations.frame().now, frame.now);
    }

    #[test]
    fn spinner_phase_advances_fractionally_and_preserves_wrap_remainder() {
        let half_turn = advance_spinner_phase(0.0, std::time::Duration::from_millis(400));
        let wrapped = advance_spinner_phase(0.99, std::time::Duration::from_millis(16));

        assert!((half_turn - 0.5).abs() < f32::EPSILON);
        assert!((wrapped - 0.01).abs() < 0.000_001);
    }
}
