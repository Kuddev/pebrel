//! Cursor presentation, independent of its shape and movement animation.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorBlinkStyle {
    #[default]
    Blink,
    Smooth,
    Phase,
    Expand,
    Solid,
}

impl CursorBlinkStyle {
    pub const VALUES: &'static [&'static str] = &["blink", "smooth", "phase", "expand", "solid"];

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "blink" => Some(Self::Blink),
            "smooth" => Some(Self::Smooth),
            "phase" => Some(Self::Phase),
            "expand" => Some(Self::Expand),
            "solid" => Some(Self::Solid),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        Self::VALUES[self as usize]
    }

    pub(crate) fn resolve(raw: &crate::RawSettings) -> Self {
        raw.value("cursor_blink_style").and_then(Self::from_settings).unwrap_or_else(|| {
            if raw.bool_on("cursor_blink") == Some(false) { Self::Solid } else { Self::Blink }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings};

    #[test]
    fn blink_styles_round_trip_and_preserve_the_legacy_disabled_preference() {
        for value in CursorBlinkStyle::VALUES {
            let raw = RawSettings::from_text(&format!("cursor_blink_style={value}\n"));
            assert_eq!(RuntimeSettings::from_raw(&raw).cursor_blink_style.settings_value(), *value);
        }
        for (text, expected) in [
            ("", CursorBlinkStyle::Blink),
            ("cursor_blink=0\n", CursorBlinkStyle::Solid),
            ("cursor_blink=1\n", CursorBlinkStyle::Blink),
            ("cursor_blink_style=invalid\ncursor_blink=0\n", CursorBlinkStyle::Solid),
            ("cursor_blink_style=smooth\ncursor_blink=0\n", CursorBlinkStyle::Smooth),
        ] {
            assert_eq!(
                RuntimeSettings::from_raw(&RawSettings::from_text(text)).cursor_blink_style,
                expected
            );
        }
        assert_eq!(CursorBlinkStyle::from_settings(" PHASE "), Some(CursorBlinkStyle::Phase));
    }
}
