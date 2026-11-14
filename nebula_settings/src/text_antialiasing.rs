/// Desktop glyph rasterization, independent of the selected font family.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAntialiasing {
    #[default]
    Grayscale,
    Subpixel,
    System,
}

impl TextAntialiasing {
    pub const KEY: &str = "text_antialiasing";
    pub const VALUES: &'static [&'static str] = &["grayscale", "subpixel", "system"];

    pub fn from_settings(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("grayscale") {
            Some(Self::Grayscale)
        } else if value.eq_ignore_ascii_case("subpixel") {
            Some(Self::Subpixel)
        } else if value.eq_ignore_ascii_case("system") {
            Some(Self::System)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn missing_empty_and_invalid_preferences_use_grayscale() {
        for text in ["", "text_antialiasing=", "text_antialiasing=unknown"] {
            let loaded = RuntimeSettings::from_raw(&RawSettings::from_text(text));
            assert_eq!(loaded.text_antialiasing, TextAntialiasing::Grayscale);
        }
    }

    #[test]
    fn supported_values_round_trip_without_changing_the_font() {
        for (value, expected) in TextAntialiasing::VALUES.iter().zip([
            TextAntialiasing::Grayscale,
            TextAntialiasing::Subpixel,
            TextAntialiasing::System,
        ]) {
            let text = apply_updates(
                "font_family=Maple Mono\nligatures=off\n",
                &[(TextAntialiasing::KEY, (*value).into())],
            );
            let loaded = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(loaded.text_antialiasing, expected);
            assert_eq!(loaded.font_family.as_deref(), Some("Maple Mono"));
            assert_eq!(loaded.ligatures, crate::Ligatures::Off);
        }
    }
}
