//! Explicit cold-resume arguments; never inferred from command history.

use crate::RawSettings;

pub const AGENT_RESUME_SETTINGS: [(&str, &str); 11] = [
    ("claude", "agent_resume_args_claude"),
    ("codex", "agent_resume_args_codex"),
    ("gemini", "agent_resume_args_gemini"),
    ("opencode", "agent_resume_args_opencode"),
    ("amp", "agent_resume_args_amp"),
    ("cursor", "agent_resume_args_cursor"),
    ("copilot", "agent_resume_args_copilot"),
    ("grok", "agent_resume_args_grok"),
    ("pi", "agent_resume_args_pi"),
    ("omp", "agent_resume_args_omp"),
    ("kimi", "agent_resume_args_kimi"),
];

#[derive(Clone, Debug, Default)]
pub struct AgentResumeArgs(std::collections::HashMap<String, String>);

impl AgentResumeArgs {
    pub(crate) fn from_raw(raw: &RawSettings) -> Self {
        Self(
            AGENT_RESUME_SETTINGS
                .iter()
                .filter_map(|(source, key)| {
                    raw.value(key).map(|value| ((*source).to_owned(), value.to_owned()))
                })
                .collect(),
        )
    }

    pub fn get(&self, source: &str) -> &str {
        self.0.get(source).map(String::as_str).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn explicit_resume_arguments_round_trip_without_affecting_other_agents() {
        let original = "resume_ai=1\ncustom=keep\n";
        let value = r#"["--yolo", "--config", "model=example"]"#;
        let text = apply_updates(original, &[("agent_resume_args_codex", value.into())]);
        let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
        assert_eq!(runtime.agent_resume_args.get("codex"), value);
        assert_eq!(runtime.agent_resume_args.get("claude"), "");
        assert!(text.starts_with(original));
        let cleared = apply_updates(&text, &[("agent_resume_args_codex", String::new())]);
        let defaults = RuntimeSettings::from_raw(&RawSettings::from_text(&cleared));
        assert_eq!(defaults.agent_resume_args.get("codex"), "");
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::default()).agent_resume_args.get("codex"),
            ""
        );
    }
}
