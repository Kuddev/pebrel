use std::collections::HashMap;

use crate::RawSettings;

pub const AGENT_RESUME_SETTINGS: [(&str, &str, &str); 12] = [
    ("claude", "agent_resume_args_claude", "agent_resume_enabled_claude"),
    ("codex", "agent_resume_args_codex", "agent_resume_enabled_codex"),
    ("antigravity", "agent_resume_args_antigravity", "agent_resume_enabled_antigravity"),
    ("gemini", "agent_resume_args_gemini", "agent_resume_enabled_gemini"),
    ("opencode", "agent_resume_args_opencode", "agent_resume_enabled_opencode"),
    ("cursor", "agent_resume_args_cursor", "agent_resume_enabled_cursor"),
    ("kimi", "agent_resume_args_kimi", "agent_resume_enabled_kimi"),
    ("pi", "agent_resume_args_pi", "agent_resume_enabled_pi"),
    ("omp", "agent_resume_args_omp", "agent_resume_enabled_omp"),
    ("copilot", "agent_resume_args_copilot", "agent_resume_enabled_copilot"),
    ("grok", "agent_resume_args_grok", "agent_resume_enabled_grok"),
    ("amp", "agent_resume_args_amp", "agent_resume_enabled_amp"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentResumeRule {
    pub source: &'static str,
    pub enabled: bool,
    pub args: String,
}

/// Raw user-configured extra argument rules for AI session resume and fork.
///
/// Argument tokenization and shell quoting are handled by the application
/// layer so `nebula_settings` stays free of shell or UI dependencies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentResumeArgs {
    values: HashMap<&'static str, (bool, String)>,
}

impl AgentResumeArgs {
    pub fn from_raw(raw: &RawSettings) -> Self {
        let mut values = HashMap::new();
        for (source, args_key, enabled_key) in AGENT_RESUME_SETTINGS {
            if let Some(value) = raw
                .value(args_key)
                .map(str::trim)
                .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
            {
                let enabled = raw.bool_on(enabled_key).unwrap_or(true);
                values.insert(source, (enabled, value.to_owned()));
            }
        }
        Self { values }
    }

    pub fn get(&self, source: &str) -> &str {
        Self::settings_entry(source)
            .and_then(|(canonical, _, _)| self.values.get(canonical))
            .and_then(|(enabled, args)| (*enabled).then_some(args.as_str()))
            .unwrap_or_default()
    }

    pub fn rule(&self, source: &str) -> Option<AgentResumeRule> {
        let (canonical, _, _) = Self::settings_entry(source)?;
        let (enabled, args) = self.values.get(canonical)?;
        Some(AgentResumeRule { source: canonical, enabled: *enabled, args: args.clone() })
    }

    pub fn rules(&self) -> Vec<AgentResumeRule> {
        AGENT_RESUME_SETTINGS
            .into_iter()
            .filter_map(|(source, _, _)| {
                let (enabled, args) = self.values.get(source)?;
                Some(AgentResumeRule { source, enabled: *enabled, args: args.clone() })
            })
            .collect()
    }

    pub fn upsert_updates(
        source: &str,
        enabled: bool,
        args: String,
    ) -> Option<[(&'static str, String); 2]> {
        let (_, args_key, enabled_key) = Self::settings_entry(source)?;
        let enabled_text = if enabled { "1" } else { "0" };
        Some([(args_key, args), (enabled_key, enabled_text.to_owned())])
    }

    pub fn remove_updates(source: &str) -> Option<[(&'static str, String); 2]> {
        let (_, args_key, enabled_key) = Self::settings_entry(source)?;
        Some([(args_key, String::new()), (enabled_key, String::new())])
    }

    fn settings_entry(source: &str) -> Option<(&'static str, &'static str, &'static str)> {
        let source = source.trim();
        AGENT_RESUME_SETTINGS.into_iter().find(|(known, _, _)| known.eq_ignore_ascii_case(source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeSettings, apply_updates};

    #[test]
    fn agent_resume_rules_default_to_empty_and_round_trip() {
        let settings = RuntimeSettings::from_raw(&RawSettings::default());
        assert!(settings.agent_resume_args.rules().is_empty());
        for (source, _, _) in AGENT_RESUME_SETTINGS {
            assert_eq!(settings.agent_resume_args.get(source), "");
            assert!(settings.agent_resume_args.rule(source).is_none());
        }

        let updates = AgentResumeArgs::upsert_updates("codex", true, "--yolo".to_owned()).unwrap();
        let text = apply_updates("", &updates);
        let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
        assert_eq!(settings.agent_resume_args.get("codex"), "--yolo");
        assert_eq!(settings.agent_resume_args.get("Codex"), "--yolo");
        assert_eq!(
            settings.agent_resume_args.rules(),
            vec![AgentResumeRule { source: "codex", enabled: true, args: "--yolo".to_owned() }]
        );
    }

    #[test]
    fn disabled_or_removed_rules_preserve_canonical_state() {
        let mut text = String::new();
        for updates in [
            AgentResumeArgs::upsert_updates(
                "antigravity",
                false,
                "--dangerously-skip-permissions".to_owned(),
            )
            .unwrap(),
            AgentResumeArgs::upsert_updates("claude", true, "--verbose".to_owned()).unwrap(),
        ] {
            text = apply_updates(&text, &updates);
        }

        let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
        // Disabled rules stay in the rule list for UI toggling, but `get` returns empty.
        assert_eq!(settings.agent_resume_args.get("antigravity"), "");
        assert_eq!(settings.agent_resume_args.get("claude"), "--verbose");
        assert_eq!(
            settings.agent_resume_args.rules(),
            vec![
                AgentResumeRule { source: "claude", enabled: true, args: "--verbose".to_owned() },
                AgentResumeRule {
                    source: "antigravity",
                    enabled: false,
                    args: "--dangerously-skip-permissions".to_owned(),
                },
            ]
        );

        let removed = apply_updates(&text, &AgentResumeArgs::remove_updates("claude").unwrap());
        let after_remove = RuntimeSettings::from_raw(&RawSettings::from_text(&removed));
        assert!(after_remove.agent_resume_args.rule("claude").is_none());
        assert_eq!(after_remove.agent_resume_args.rules().len(), 1);
    }
}
