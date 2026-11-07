/// The editor starts at the natural shaped height; absent settings preserve typography.
pub const DEFAULT_TERMINAL_LINE_HEIGHT: f32 = 1.0;

pub fn normalize_terminal_line_height(value: f32) -> f32 {
    if value.is_finite() {
        (value.clamp(0.5, 5.0) * 100.0).round() / 100.0
    } else {
        DEFAULT_TERMINAL_LINE_HEIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn line_height_round_trip_range_and_precision() {
        assert!(
            RuntimeSettings::from_raw(&RawSettings::from_text("")).terminal_line_height.is_none()
        );
        for (input, expected) in [(0.1, 0.5), (1.0, 1.0), (1.456, 1.46), (6.0, 5.0)] {
            let text =
                apply_updates("font_size=18\n", &[("terminal_line_height", input.to_string())]);
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(settings.terminal_line_height, Some(expected));
            assert_eq!(settings.font_size_px, Some(18.0));
        }
        for input in ["NaN", "inf", "invalid"] {
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&format!(
                "terminal_line_height={input}"
            )));
            assert!(settings.terminal_line_height.is_none());
        }
    }
}
