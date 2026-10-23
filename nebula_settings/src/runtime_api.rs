/// Where the remote Runtime API listener binds; `Off` keeps only the private per-launch endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuntimeApiExposure {
    #[default]
    Off,
    Local,
    Lan,
}

impl RuntimeApiExposure {
    pub const VALUES: &'static [&'static str] = &["off", "local", "lan"];

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim() {
            value if value.eq_ignore_ascii_case("off") => Some(Self::Off),
            value if value.eq_ignore_ascii_case("local") => Some(Self::Local),
            value if value.eq_ignore_ascii_case("lan") => Some(Self::Lan),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Local => "local",
            Self::Lan => "lan",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn exposure_round_trips_and_defaults_to_off() {
        for value in RuntimeApiExposure::VALUES {
            let text = apply_updates("custom=keep\n", &[("runtime_api_exposure", (*value).into())]);
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(settings.runtime_api_exposure.settings_value(), *value);
            assert!(text.contains("custom=keep"));
        }
        for text in ["", "runtime_api_exposure=invalid", "runtime_api_exposure=1"] {
            assert_eq!(
                RuntimeSettings::from_raw(&RawSettings::from_text(text)).runtime_api_exposure,
                RuntimeApiExposure::Off,
            );
        }
    }
}
