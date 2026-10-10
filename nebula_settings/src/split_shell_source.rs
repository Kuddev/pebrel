/// Launch source for interactive terminal splits. Missing or invalid values
/// preserve the default-shell choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitShellSource {
    #[default]
    Default,
    Focused,
    Ask,
}

impl SplitShellSource {
    pub const ALL: [Self; 3] = [Self::Default, Self::Focused, Self::Ask];
    pub const VALUES: &'static [&'static str] = &["default", "focused", "ask"];

    pub const fn next(self) -> Self {
        match self {
            Self::Default => Self::Focused,
            Self::Focused => Self::Ask,
            Self::Ask => Self::Default,
        }
    }

    pub fn from_settings(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.settings_value().eq_ignore_ascii_case(value.trim()))
    }

    pub const fn settings_value(self) -> &'static str {
        match self {
            Self::Focused => "focused",
            Self::Default => "default",
            Self::Ask => "ask",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn split_sources_parse_and_round_trip_without_changing_other_settings() {
        for source in SplitShellSource::ALL {
            let value = source.settings_value();
            assert_eq!(SplitShellSource::from_settings(value), Some(source));
            assert_eq!(SplitShellSource::from_settings(&value.to_uppercase()), Some(source));
            let text = apply_updates("custom=keep\n", &[("split_shell_source", value.into())]);
            assert!(text.contains("custom=keep"));
            assert_eq!(
                RuntimeSettings::from_raw(&RawSettings::from_text(&text)).split_shell_source,
                source
            );
        }
        assert_eq!(SplitShellSource::VALUES, ["default", "focused", "ask"]);
        for (source, value) in SplitShellSource::ALL.into_iter().zip(SplitShellSource::VALUES) {
            assert_eq!(source.settings_value(), *value);
        }
    }

    #[test]
    fn cycling_visits_every_source_and_wraps_to_default() {
        let mut source = SplitShellSource::Default;
        for expected in
            [SplitShellSource::Focused, SplitShellSource::Ask, SplitShellSource::Default]
        {
            source = source.next();
            assert_eq!(source, expected);
        }
    }

    #[test]
    fn missing_and_invalid_settings_use_default_shell() {
        for text in ["", "split_shell_source=invalid", "split_shell_source="] {
            assert_eq!(
                RuntimeSettings::from_raw(&RawSettings::from_text(text)).split_shell_source,
                SplitShellSource::Default
            );
        }
    }
}
