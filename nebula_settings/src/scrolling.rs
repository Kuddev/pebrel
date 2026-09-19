use crate::RawSettings;

/// Persisted choices for newly created terminals; existing history is never resized by this key.
pub const SCROLLBACK_VALUES: &[&str] =
    &["1000", "2000", "5000", "10000", "20000", "50000", "100000"];
pub const DEFAULT_SCROLLBACK_LINES: usize = 10_000;
pub const DEFAULT_SCROLL_SPEED: f32 = 1.0;
pub const MIN_SCROLL_SPEED: f32 = 0.25;
pub const MAX_SCROLL_SPEED: f32 = 4.0;
pub const SCROLL_SPEED_STEP: f32 = 0.25;

pub(crate) fn scrollback_lines(raw: &RawSettings) -> usize {
    raw.value("scrollback_lines")
        .filter(|value| SCROLLBACK_VALUES.contains(value))
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_SCROLLBACK_LINES)
}

pub fn normalize_scroll_speed(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_SCROLL_SPEED, MAX_SCROLL_SPEED)
    } else {
        DEFAULT_SCROLL_SPEED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeSettings, apply_updates};

    #[test]
    fn all_seven_history_choices_round_trip_without_changing_other_preferences() {
        assert_eq!(
            SCROLLBACK_VALUES,
            ["1000", "2000", "5000", "10000", "20000", "50000", "100000"]
        );
        for value in SCROLLBACK_VALUES {
            let text = apply_updates(
                "font_size=18\ncustom=keep\n",
                &[("scrollback_lines", (*value).into())],
            );
            let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(runtime.scrollback_lines, value.parse::<usize>().unwrap());
            assert_eq!(runtime.font_size_px, Some(18.0));
            assert!(text.contains("custom=keep"));
        }
    }

    #[test]
    fn invalid_history_preferences_preserve_the_existing_default_limit() {
        for value in ["", "0", "-1", "999", "200000", "1,000", "invalid"] {
            let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&format!(
                "scrollback_lines={value}\n"
            )));
            assert_eq!(runtime.scrollback_lines, 10_000, "{value}");
        }
    }

    #[test]
    fn speed_preserves_fractional_multipliers_and_rejects_nonfinite_values() {
        for (value, expected) in [
            ("0.25", 0.25),
            ("1", 1.0),
            ("1.75", 1.75),
            ("4", 4.0),
            ("-1", 0.25),
            ("99", 4.0),
            ("NaN", 1.0),
            ("inf", 1.0),
            ("invalid", 1.0),
        ] {
            let text = apply_updates("scrollback_lines=50000\n", &[("scroll_speed", value.into())]);
            let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(runtime.scroll_speed, expected, "{value}");
            assert_eq!(runtime.scrollback_lines, 50_000);
        }
    }
}
