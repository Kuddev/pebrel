//! Chrome animation types and animation stepping methods.

use std::time::Duration;

use super::settings;
use super::ui;
use super::{SettingsHit, SplitDirection};

#[derive(Debug, Clone, Copy)]
pub(super) struct UiAnim {
    spring: crate::motion::Spring,
}

impl UiAnim {
    pub(super) fn new(value: f32) -> Self {
        Self { spring: crate::motion::Spring::new(value.clamp(0.0, 1.0)).with_response(0.14) }
    }

    pub(super) fn value(self) -> f32 {
        self.spring.value().clamp(0.0, 1.0)
    }

    pub(super) fn visible(self, target_open: bool) -> bool {
        target_open || self.value() > 0.004
    }

    pub(super) fn animating_to(self, target: f32) -> bool {
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
pub(super) struct SettingsToggleAnim {
    position: crate::motion::Tween,
    stretch: crate::motion::Tween,
    color: crate::motion::Tween,
    hover: crate::motion::Tween,
}

impl SettingsToggleAnim {
    pub(super) fn new(on: bool) -> Self {
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

    pub(super) fn value(self) -> ui::widgets::ToggleMotion {
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
pub(super) struct NebulaUiAnims {
    clock: crate::motion::MotionClock,
    frame: Option<crate::motion::Frame>,
    /// Continuous sidebar-spinner phase in turns (`0.0..1.0`). Advancing it
    /// from the shared monotonic frame delta avoids wall-clock jumps and needs
    /// only four bytes per window.
    pub(super) spinner_phase: f32,
    pub(super) left_sidebar: UiAnim,
    pub(super) right_drawer: UiAnim,
    pub(super) ssh_editor: UiAnim,
    pub(super) settings_toggles: [SettingsToggleAnim; settings::SETTINGS_TOGGLE_COUNT],
}

impl NebulaUiAnims {
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

    fn step(
        &mut self,
        left_open: bool,
        right_open: bool,
        ssh_open: bool,
        toggle_targets: [bool; settings::SETTINGS_TOGGLE_COUNT],
        toggle_pressed: SettingsHit,
        toggle_hover: SettingsHit,
    ) {
        let frame = self.clock.tick();
        self.frame = Some(frame);
        self.left_sidebar.step(frame, if left_open { 1.0 } else { 0.0 });
        self.right_drawer.step(frame, if right_open { 1.0 } else { 0.0 });
        self.ssh_editor.step(frame, if ssh_open { 1.0 } else { 0.0 });
        for (index, (anim, target)) in
            self.settings_toggles.iter_mut().zip(toggle_targets).enumerate()
        {
            let pressed = settings::settings_toggle_slot(toggle_pressed) == Some(index);
            let hovered = settings::settings_toggle_slot(toggle_hover) == Some(index);
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

    fn animating(
        &self,
        left_open: bool,
        right_open: bool,
        toggle_targets: [bool; settings::SETTINGS_TOGGLE_COUNT],
        toggle_pressed: SettingsHit,
        toggle_hover: SettingsHit,
    ) -> bool {
        self.left_sidebar.animating_to(if left_open { 1.0 } else { 0.0 })
            || self.right_drawer.animating_to(if right_open { 1.0 } else { 0.0 })
            || self.settings_toggles.iter().zip(toggle_targets).enumerate().any(
                |(index, (anim, target))| {
                    anim.animating_to(
                        target,
                        settings::settings_toggle_slot(toggle_pressed) == Some(index),
                        settings::settings_toggle_slot(toggle_hover) == Some(index),
                    )
                },
            )
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ResizeHud {
    pub(super) columns: usize,
    pub(super) rows: usize,
    pub(super) opacity: crate::motion::Tween,
}

impl ResizeHud {
    pub(super) fn new(columns: usize, rows: usize) -> Self {
        let mut opacity = crate::motion::Tween::new(1.0);
        opacity.animate_to(
            0.0,
            Duration::from_millis(900),
            crate::motion::Easing::Linear,
            crate::motion::MotionPolicy::Full,
        );
        Self { columns, rows, opacity }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SplitReveal {
    pub rect: (f32, f32, f32, f32),
    pub direction: SplitDirection,
    pub motion: crate::motion::Tween,
}

impl SplitReveal {
    pub fn new(rect: (f32, f32, f32, f32), direction: SplitDirection) -> Self {
        let mut motion = crate::motion::Tween::new(0.0);
        motion.animate_role(
            1.0,
            crate::motion::MotionRole::Enter,
            crate::motion::MotionPolicy::Full,
        );
        Self { rect, direction, motion }
    }
}

use super::Display;

impl Display {
    pub fn step_chrome_anims(&mut self) {
        let toggle_targets = self.settings_toggle_targets();
        self.nebula_ui_anims.step(
            !self.nebula_sidebar_collapsed,
            self.nebula_side_panel.open,
            self.nebula_ssh_editor_open,
            toggle_targets,
            self.nebula_settings_pressed,
            self.nebula_settings_hover,
        );
    }

    pub fn chrome_animating(&self) -> bool {
        self.nebula_ui_anims.animating(
            !self.nebula_sidebar_collapsed,
            self.nebula_side_panel.open,
            self.settings_toggle_targets(),
            self.nebula_settings_pressed,
            self.nebula_settings_hover,
        )
    }

    pub fn left_sidebar_progress(&self) -> f32 {
        self.nebula_ui_anims.left_sidebar.value()
    }

    pub fn left_sidebar_visible(&self) -> bool {
        self.nebula_ui_anims.left_sidebar.visible(!self.nebula_sidebar_collapsed)
    }
}