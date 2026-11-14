use std::time::Duration;

use crate::RawSettings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabSleep {
    #[default]
    Off,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
}

impl TabSleep {
    pub const VALUES: &'static [&'static str] = &["0", "5", "15", "30", "60"];
    const MODES: [Self; Self::VALUES.len()] =
        [Self::Off, Self::FiveMinutes, Self::FifteenMinutes, Self::ThirtyMinutes, Self::OneHour];

    pub(crate) fn from_raw(raw: &RawSettings) -> Self {
        raw.value("tab_sleep_minutes")
            .and_then(|value| Self::VALUES.iter().position(|id| *id == value))
            .map_or_else(Self::default, |index| Self::MODES[index])
    }

    pub fn settings_value(self) -> &'static str {
        Self::VALUES[self as usize]
    }

    pub fn timeout(self) -> Option<Duration> {
        let minutes = match self {
            Self::Off => return None,
            Self::FiveMinutes => 5,
            Self::FifteenMinutes => 15,
            Self::ThirtyMinutes => 30,
            Self::OneHour => 60,
        };
        Some(Duration::from_secs(minutes * 60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeSettings, apply_updates};

    #[test]
    fn session_sleep_is_opt_in_and_persisted_choices_round_trip() {
        for input in
            ["", "tab_sleep_minutes=invalid", "tab_sleep_minutes=-5", "tab_sleep_minutes=3"]
        {
            assert_eq!(
                RuntimeSettings::from_raw(&RawSettings::from_text(input)).tab_sleep,
                TabSleep::Off
            );
        }
        for (index, value) in TabSleep::VALUES.iter().enumerate() {
            let text =
                apply_updates("custom=keep\n", &[("tab_sleep_minutes", (*value).to_owned())]);
            let mode = RuntimeSettings::from_raw(&RawSettings::from_text(&text)).tab_sleep;
            assert_eq!(mode, TabSleep::MODES[index]);
            assert_eq!(mode.settings_value(), *value);
            assert_eq!(
                mode.timeout().map(|time| time.as_secs() / 60).unwrap_or(0).to_string(),
                *value
            );
            assert!(text.contains("custom=keep"));
        }
    }
}
