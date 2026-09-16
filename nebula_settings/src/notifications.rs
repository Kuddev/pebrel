//! Shared in-app notification lifetime, independent of delivery and approvals.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotificationDuration {
    #[default]
    Default,
    FiveSeconds,
    TenSeconds,
    ThirtySeconds,
    NinetySeconds,
    Persistent,
}

impl NotificationDuration {
    pub const VALUES: &'static [&'static str] = &["default", "5", "10", "30", "90", "persistent"];

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim() {
            value if value.eq_ignore_ascii_case("default") => Some(Self::Default),
            "5" => Some(Self::FiveSeconds),
            "10" => Some(Self::TenSeconds),
            "30" => Some(Self::ThirtySeconds),
            "90" => Some(Self::NinetySeconds),
            value if value.eq_ignore_ascii_case("persistent") => Some(Self::Persistent),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::FiveSeconds => "5",
            Self::TenSeconds => "10",
            Self::ThirtySeconds => "30",
            Self::NinetySeconds => "90",
            Self::Persistent => "persistent",
        }
    }

    /// Retain each caller's previous lifetime unless the user overrides it.
    /// `None` is an originally persistent notification, such as an update action.
    pub fn timeout(self, default: Option<Duration>) -> Option<Duration> {
        match self {
            Self::Default => default,
            Self::FiveSeconds => Some(Duration::from_secs(5)),
            Self::TenSeconds => Some(Duration::from_secs(10)),
            Self::ThirtySeconds => Some(Duration::from_secs(30)),
            Self::NinetySeconds => Some(Duration::from_secs(90)),
            Self::Persistent => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn missing_or_invalid_duration_preserves_each_notifications_existing_lifetime() {
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("")).notification_duration,
            NotificationDuration::Default
        );
        for value in ["", "invalid", "-1", "0", "1", "999999999999999999999"] {
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&format!(
                "notification_duration={value}\n"
            )));
            assert_eq!(settings.notification_duration, NotificationDuration::Default);
            for default in [Some(Duration::from_secs(5)), Some(Duration::from_secs(90)), None] {
                assert_eq!(settings.notification_duration.timeout(default), default);
            }
        }
    }

    #[test]
    fn every_duration_round_trips_without_changing_the_delivery_switch_or_other_data() {
        for value in NotificationDuration::VALUES {
            let saved = apply_updates(
                "ai_toasts=0\ncustom_data=keep\n",
                &[("notification_duration", (*value).to_owned())],
            );
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&saved));
            assert_eq!(settings.notification_duration.settings_value(), *value);
            assert!(!settings.ai_toasts);
            assert!(saved.contains("custom_data=keep"));
        }
    }

    #[test]
    fn timed_and_persistent_modes_have_explicit_lifetimes() {
        for (value, seconds) in [("5", 5), ("10", 10), ("30", 30), ("90", 90)] {
            assert_eq!(
                NotificationDuration::from_settings(value).unwrap().timeout(None),
                Some(Duration::from_secs(seconds))
            );
        }
        assert_eq!(
            NotificationDuration::from_settings(" PERSISTENT ")
                .unwrap()
                .timeout(Some(Duration::from_secs(5))),
            None
        );
        assert_eq!(
            NotificationDuration::from_settings(" DEFAULT "),
            Some(NotificationDuration::Default)
        );
    }
}
