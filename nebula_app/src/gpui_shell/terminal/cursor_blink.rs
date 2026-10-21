//! Presentation-only blinking. VT visibility, focus and IME gating stay in the view.

use nebula_settings::CursorBlinkStyle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BlinkFrame {
    pub opacity: f32,
    pub height: f32,
}

impl Default for BlinkFrame {
    fn default() -> Self {
        Self { opacity: 1.0, height: 1.0 }
    }
}

pub(super) struct CursorBlink {
    pub style: CursorBlinkStyle,
    pub frame: BlinkFrame,
    started: Instant,
}

impl Default for CursorBlink {
    fn default() -> Self {
        Self { style: CursorBlinkStyle::Blink, frame: Default::default(), started: Instant::now() }
    }
}

impl CursorBlink {
    pub fn restart(&mut self, style: CursorBlinkStyle, reduce_motion: bool) {
        self.style = if reduce_motion
            && !matches!(style, CursorBlinkStyle::Solid | CursorBlinkStyle::Blink)
        {
            CursorBlinkStyle::Blink
        } else {
            style
        };
        self.started = Instant::now();
        self.frame = BlinkFrame::default();
    }

    pub fn interval(&self) -> Duration {
        Duration::from_millis(if self.style == CursorBlinkStyle::Blink { 750 } else { 16 })
    }

    pub fn tick(&mut self) -> BlinkFrame {
        self.frame = if self.style == CursorBlinkStyle::Blink {
            BlinkFrame { opacity: if self.frame.opacity > 0.0 { 0.0 } else { 1.0 }, height: 1.0 }
        } else {
            sample(self.style, self.started.elapsed())
        };
        self.frame
    }
}

fn sample(style: CursorBlinkStyle, elapsed: Duration) -> BlinkFrame {
    let phase = (elapsed.as_secs_f32() / 1.5).fract();
    let fade = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let envelope = if phase < 0.35 {
        1.0
    } else if phase < 0.5 {
        1.0 - fade((phase - 0.35) / 0.15)
    } else if phase < 0.85 {
        0.0
    } else {
        fade((phase - 0.85) / 0.15)
    };
    let mut frame = BlinkFrame::default();
    match style {
        CursorBlinkStyle::Blink => frame.opacity = if phase < 0.5 { 1.0 } else { 0.0 },
        CursorBlinkStyle::Smooth => frame.opacity = envelope,
        CursorBlinkStyle::Phase => {
            frame.opacity = 0.1 + 0.9 * (1.0 + (phase * std::f32::consts::TAU).cos()) * 0.5
        },
        CursorBlinkStyle::Expand => frame.height = envelope,
        CursorBlinkStyle::Solid => {},
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_have_distinct_opacity_and_height_cycles_with_no_discontinuity_at_restart() {
        let time = Duration::from_millis(640);
        assert_eq!(sample(CursorBlinkStyle::Blink, time).opacity, 1.0);
        let smooth = sample(CursorBlinkStyle::Smooth, time);
        assert!(smooth.opacity > 0.0 && smooth.opacity < 1.0);
        assert_eq!(smooth.height, 1.0);
        let expand = sample(CursorBlinkStyle::Expand, time);
        assert_eq!(expand.opacity, 1.0);
        assert_eq!(expand.height, smooth.opacity);
        for style in [
            CursorBlinkStyle::Blink,
            CursorBlinkStyle::Smooth,
            CursorBlinkStyle::Phase,
            CursorBlinkStyle::Expand,
            CursorBlinkStyle::Solid,
        ] {
            assert_eq!(sample(style, Duration::ZERO), BlinkFrame::default());
            assert_eq!(sample(style, Duration::from_millis(1500)), BlinkFrame::default());
        }
        assert_eq!(sample(CursorBlinkStyle::Solid, time), BlinkFrame::default());
        assert!(sample(CursorBlinkStyle::Phase, Duration::from_millis(750)).opacity >= 0.09);
        assert_eq!(sample(CursorBlinkStyle::Blink, Duration::from_millis(750)).opacity, 0.0);
    }

    #[test]
    fn switching_styles_and_reduced_motion_reset_to_a_visible_cursor() {
        let mut blink = CursorBlink::default();
        assert_eq!(blink.tick().opacity, 0.0);
        blink.restart(CursorBlinkStyle::Solid, false);
        assert_eq!(blink.frame, BlinkFrame::default());
        blink.restart(CursorBlinkStyle::Smooth, true);
        assert_eq!(blink.style, CursorBlinkStyle::Blink);
        blink.restart(CursorBlinkStyle::Solid, true);
        assert_eq!(blink.style, CursorBlinkStyle::Solid);
    }
}
