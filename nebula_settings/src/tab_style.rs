//! Appearance of the horizontal workspace tabs; sidebar tabs keep their layout.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabStyle {
    #[default]
    Chrome,
    Minimal,
    Tag,
}

impl TabStyle {
    pub const VALUES: &'static [&'static str] = &["chrome", "minimal", "tag"];

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chrome" => Some(Self::Chrome),
            "minimal" => Some(Self::Minimal),
            "tag" => Some(Self::Tag),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        Self::VALUES[self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings};

    #[test]
    fn styles_round_trip_and_old_or_invalid_preferences_use_chrome() {
        for value in TabStyle::VALUES {
            let raw = RawSettings::from_text(&format!("tab_style={value}\n"));
            assert_eq!(RuntimeSettings::from_raw(&raw).tab_style.settings_value(), *value);
        }
        for text in ["", "tab_style=unknown\n"] {
            assert_eq!(
                RuntimeSettings::from_raw(&RawSettings::from_text(text)).tab_style,
                TabStyle::Chrome
            );
        }
        assert_eq!(TabStyle::from_settings(" TAG "), Some(TabStyle::Tag));
    }
}
