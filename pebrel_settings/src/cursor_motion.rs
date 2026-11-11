//! Persistent opt-in for presentation-only terminal cursor movement.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorMotion {
    #[default]
    Off,
    Smooth,
}

impl CursorMotion {
    pub const VALUES: &'static [&'static str] = &["off", "smooth"];

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim() {
            value if value.eq_ignore_ascii_case("off") => Some(Self::Off),
            value if value.eq_ignore_ascii_case("smooth") => Some(Self::Smooth),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Smooth => "smooth",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn cursor_motion_is_opt_in_and_invalid_values_preserve_instant_motion() {
        for text in [
            "",
            "cursor_motion=",
            "cursor_motion=true",
            "cursor_motion=90",
            "cursor_motion=unknown",
        ] {
            assert_eq!(
                RuntimeSettings::from_raw(&RawSettings::from_text(text)).cursor_motion,
                CursorMotion::Off
            );
        }
    }

    #[test]
    fn cursor_motion_round_trips_independently_of_shape_blink_and_unknown_data() {
        for value in CursorMotion::VALUES {
            let text = apply_updates(
                "cursor_shape=block\ncursor_blink=0\ncustom=keep\n",
                &[("cursor_motion", (*value).into())],
            );
            let loaded = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(loaded.cursor_motion.settings_value(), *value);
            assert_eq!(loaded.cursor_shape, Some(crate::CursorShapeName::Block));
            assert_eq!(loaded.cursor_blink, Some(false));
            assert!(text.contains("custom=keep"));
        }
        assert_eq!(CursorMotion::from_settings(" SMOOTH "), Some(CursorMotion::Smooth));
    }
}
