//! Explicit local-profile variables; values never appear in diagnostic formatting.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, PartialEq, Eq, Serialize, nebula_config_derive::SerdeReplace)]
#[serde(transparent)]
pub struct ProfileEnvironment(pub BTreeMap<String, String>);

impl<'de> Deserialize<'de> for ProfileEnvironment {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = BTreeMap::<String, String>::deserialize(deserializer)?;
        Self::from_pairs(values).map_err(serde::de::Error::custom)
    }
}

impl fmt::Debug for ProfileEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ProfileEnvironment").field("names", &self.0.keys()).finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InvalidEnvironment {
    Name(String),
    Value(String),
    Reserved(String),
    Duplicate(String),
}

impl fmt::Display for InvalidEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(name) => write!(f, "invalid environment name: {name:?}"),
            Self::Value(name) => write!(f, "environment value contains NUL: {name:?}"),
            Self::Reserved(name) => {
                write!(f, "environment name is owned by Pebrel integration: {name:?}")
            },
            Self::Duplicate(name) => write!(f, "duplicate environment name: {name:?}"),
        }
    }
}

impl std::error::Error for InvalidEnvironment {}

impl ProfileEnvironment {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn from_pairs(
        pairs: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, InvalidEnvironment> {
        Self::from_pairs_with_case(pairs, crate::platform::environment::names_case_insensitive())
    }

    fn from_pairs_with_case(
        pairs: impl IntoIterator<Item = (String, String)>,
        insensitive: bool,
    ) -> Result<Self, InvalidEnvironment> {
        let mut values = BTreeMap::new();
        for (name, value) in pairs {
            validate_pair(&name, &value, insensitive)?;
            if values.keys().any(|existing: &String| same_name(existing, &name, insensitive)) {
                return Err(InvalidEnvironment::Duplicate(name));
            }
            values.insert(name, value);
        }
        Ok(Self(values))
    }

    pub(crate) fn validate(&self) -> Result<(), InvalidEnvironment> {
        Self::from_pairs(self.0.iter().map(|(name, value)| (name.clone(), value.clone())))
            .map(|_| ())
    }

    pub(crate) fn apply(&self, environment: &mut HashMap<String, String>) {
        overlay(environment, &self.0, crate::platform::environment::names_case_insensitive());
    }
}

/// Registry refresh/inheritance has already prepared the base. Each explicit layer
/// replaces only its own keys; empty strings remain present in the child.
pub(crate) fn overlay(
    environment: &mut HashMap<String, String>,
    values: &BTreeMap<String, String>,
    insensitive: bool,
) {
    for (name, value) in values {
        if insensitive {
            environment.retain(|existing, _| !same_name(existing, name, true));
        }
        environment.insert(name.clone(), value.clone());
    }
}

fn same_name(left: &str, right: &str, insensitive: bool) -> bool {
    if insensitive { left.eq_ignore_ascii_case(right) } else { left == right }
}

fn validate_pair(name: &str, value: &str, insensitive: bool) -> Result<(), InvalidEnvironment> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return Err(InvalidEnvironment::Name(name.into()));
    }
    if value.contains('\0') {
        return Err(InvalidEnvironment::Value(name.into()));
    }
    const RESERVED: &[&str] = &[
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "WINDOWID",
        "NEBULA_WINDOW_ID",
        "PEBREL_PANE_ID",
        "NEBULA_PANE_ID",
        "PEBREL_PROCESS_ID",
        "PEBREL_CLI",
        "NEBULA_CLI",
        "PEBREL_BIN_DIR",
        "NEBULA_BIN_DIR",
        "PEBREL_HOOK_EXE",
        "NEBULA_HOOK_EXE",
        "PEBREL_NOTIFY_PIPE",
        "NEBULA_NOTIFY_PIPE",
        "PEBREL_REMOTE_HOOK_TOKEN",
        "NEBULA_REMOTE_HOOK_TOKEN",
        "PEBREL_CONFIG_DIR",
        "NEBULA_CONFIG_DIR",
        "PEBREL_RUNTIME_ENDPOINT",
        "NEBULA_ZSH_INTEGRATION",
        "NEBULA_ORIGINAL_ZDOTDIR",
        "NEBULA_ZDOTDIR_WAS_SET",
    ];
    if RESERVED.iter().any(|reserved| same_name(name, reserved, insensitive)) {
        return Err(InvalidEnvironment::Reserved(name.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_environment_overlays_case_rules_and_preserves_empty_values() {
        for insensitive in [false, true] {
            let profile = ProfileEnvironment::from_pairs_with_case(
                [("PATH".into(), "profile".into()), ("EMPTY".into(), "".into())],
                insensitive,
            )
            .unwrap();
            let mut environment =
                HashMap::from([("Path".into(), "base".into()), ("KEEP".into(), "base".into())]);
            overlay(&mut environment, &profile.0, insensitive);
            assert_eq!(environment["PATH"], "profile");
            assert_eq!(environment["EMPTY"], "");
            assert_eq!(environment["KEEP"], "base");
            assert_eq!(environment.contains_key("Path"), !insensitive);
        }
    }

    #[test]
    fn local_profile_environment_keeps_selected_network_proxy_authoritative() {
        let profile = ProfileEnvironment::from_pairs([
            ("HTTP_PROXY".into(), "http://profile.invalid:1234".into()),
            ("PEBREL_HTTP_PROXY".into(), "http://profile.invalid:1234".into()),
            ("QA_UNRELATED".into(), "keep".into()),
        ])
        .unwrap();
        let mut environment = HashMap::new();
        profile.apply(&mut environment);
        let mut settings =
            nebula_settings::RuntimeSettings::from_raw(&nebula_settings::RawSettings::default());
        settings.ssh_proxy_mode = nebula_settings::ProxyModeName::Custom;
        settings.ssh_proxy_url = "socks5://127.0.0.1:7897".into();
        assert!(crate::ssh_proxy::apply_saved_terminal_proxy_env(&mut environment, &settings));
        assert!(!environment.contains_key("HTTP_PROXY"));
        for name in ["http_proxy", "https_proxy", "all_proxy"] {
            assert_eq!(environment[name], "socks5://127.0.0.1:7897");
        }
        assert_eq!(environment["PEBREL_HTTP_PROXY"], "");
        assert_eq!(environment["QA_UNRELATED"], "keep");
    }

    #[test]
    fn profile_environment_validates_without_disclosing_values() {
        let secret = "synthetic-token-only";
        let environment =
            ProfileEnvironment::from_pairs([("TOKEN".into(), secret.into())]).unwrap();
        assert!(!format!("{environment:?}").contains(secret));
        let invalid =
            ProfileEnvironment::from_pairs([("TOKEN".into(), format!("{secret}\0"))]).unwrap_err();
        assert!(!invalid.to_string().contains(secret));
        assert!(
            ProfileEnvironment::from_pairs_with_case(
                [("Path".into(), "a".into()), ("PATH".into(), "b".into())],
                true
            )
            .is_err()
        );
        assert!(
            ProfileEnvironment::from_pairs_with_case(
                [("Path".into(), "a".into()), ("PATH".into(), "b".into())],
                false
            )
            .is_ok()
        );
        assert!(
            ProfileEnvironment::from_pairs([("PEBREL_PANE_ID".into(), "forged".into())]).is_err()
        );
    }
}
