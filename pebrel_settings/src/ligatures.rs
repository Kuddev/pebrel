//! Terminal ligature preference and theme precedence.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ligatures {
    #[default]
    On,
    Off,
    Theme,
}

impl Ligatures {
    pub const VALUES: &'static [&'static str] = &["on", "off", "theme"];

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim() {
            value if value.eq_ignore_ascii_case("on") => Some(Self::On),
            value if value.eq_ignore_ascii_case("off") => Some(Self::Off),
            value if value.eq_ignore_ascii_case("theme") => Some(Self::Theme),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Theme => "theme",
        }
    }

    pub fn enabled(self, theme: Option<bool>) -> bool {
        match self {
            Self::On => true,
            Self::Off => false,
            Self::Theme => theme.unwrap_or(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn old_and_invalid_settings_enable_ligatures_by_default() {
        for value in ["", "ligatures=", "ligatures=invalid"] {
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(value));
            assert_eq!(settings.ligatures, Ligatures::On);
            assert!(settings.ligatures.enabled(Some(false)));
        }
    }

    #[test]
    fn preference_round_trips_and_only_theme_mode_defers_to_the_theme() {
        for preference in [Ligatures::On, Ligatures::Off, Ligatures::Theme] {
            let saved = apply_updates(
                "font_family=Maple Mono Normal NF CN\ncustom=keep\n",
                &[("ligatures", preference.settings_value().into())],
            );
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&saved));
            assert_eq!(settings.ligatures, preference);
            assert_eq!(settings.font_family.as_deref(), Some("Maple Mono Normal NF CN"));
            assert!(saved.contains("custom=keep"));
        }
        for theme in [None, Some(false), Some(true)] {
            assert!(Ligatures::On.enabled(theme));
            assert!(!Ligatures::Off.enabled(theme));
            assert_eq!(Ligatures::Theme.enabled(theme), theme.unwrap_or(true));
        }
    }
}
