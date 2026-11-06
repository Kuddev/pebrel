//! Terminal font weight and its corresponding ANSI bold weight.

/// A standard font weight from 100 (thin) through 900 (black), in steps of 100.
/// The private value keeps renderer adapters from constructing invalid weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalFontWeight(u16);

impl Default for TerminalFontWeight {
    fn default() -> Self {
        Self(400)
    }
}

impl TerminalFontWeight {
    pub const VALUES: &'static [&'static str] =
        &["100", "200", "300", "400", "500", "600", "700", "800", "900"];

    pub fn from_settings(value: &str) -> Option<Self> {
        Self::VALUES
            .iter()
            .position(|candidate| *candidate == value.trim())
            .map(|index| Self((index as u16 + 1) * 100))
    }

    pub fn settings_value(self) -> &'static str {
        Self::VALUES[(self.0 / 100 - 1) as usize]
    }

    pub const fn value(self) -> u16 {
        self.0
    }

    /// Preserve the existing 400/700 regular/bold pair by default. Increasing
    /// the regular weight also increases ANSI bold, up to the standard maximum.
    pub const fn bold_weight(self) -> u16 {
        let weight = self.0 + 300;
        if weight > 900 { 900 } else { weight }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn old_or_invalid_settings_preserve_regular_and_bold_defaults() {
        let old = RuntimeSettings::from_raw(&RawSettings::from_text("font_size=16\n"));
        assert_eq!(old.font_weight.value(), 400);
        assert_eq!(old.font_weight.bold_weight(), 700);
        for value in ["", "normal", "0", "99", "150", "450", "901", "1000", "-100", "400.0"] {
            assert_eq!(TerminalFontWeight::from_settings(value), None);
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&format!(
                "font_weight={value}\n"
            )));
            assert_eq!(settings.font_weight, TerminalFontWeight::default());
        }
    }

    #[test]
    fn every_weight_round_trips_without_changing_other_font_preferences_or_unknown_data() {
        for value in TerminalFontWeight::VALUES {
            let saved = apply_updates(
                "font_family=Maple Mono Normal NF CN\nfont_size=16\nligatures=off\ncustom=keep\n",
                &[("font_weight", (*value).to_owned())],
            );
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&saved));
            assert_eq!(settings.font_weight.settings_value(), *value);
            assert_eq!(settings.font_weight.value().to_string(), *value);
            assert_eq!(settings.font_family.as_deref(), Some("Maple Mono Normal NF CN"));
            assert_eq!(settings.font_size_px, Some(16.0));
            assert_eq!(settings.ligatures, crate::Ligatures::Off);
            assert!(saved.contains("custom=keep"));
        }
        assert_eq!(TerminalFontWeight::from_settings(" 600 ").unwrap().settings_value(), "600");
    }

    #[test]
    fn bold_follows_the_regular_weight_without_exceeding_the_standard_maximum() {
        for (regular, bold) in [
            ("100", 400),
            ("200", 500),
            ("300", 600),
            ("400", 700),
            ("500", 800),
            ("600", 900),
            ("700", 900),
            ("800", 900),
            ("900", 900),
        ] {
            assert_eq!(TerminalFontWeight::from_settings(regular).unwrap().bold_weight(), bold);
        }
    }
}
