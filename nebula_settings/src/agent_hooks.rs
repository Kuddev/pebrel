//! Per-agent hook preferences. Missing keys inherit the historical global switch.

use crate::RawSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentHook {
    Claude,
    Codex,
    OpenCode,
    Pi,
    Copilot,
    Grok,
    OhMyPi,
    Cursor,
    Kimi,
}

impl AgentHook {
    pub const DEFAULT: [Self; 2] = [Self::Claude, Self::Codex];

    pub const ALL: [Self; 9] = [
        Self::Claude,
        Self::Codex,
        Self::OpenCode,
        Self::Pi,
        Self::Copilot,
        Self::Grok,
        Self::OhMyPi,
        Self::Cursor,
        Self::Kimi,
    ];

    pub const fn settings_key(self) -> &'static str {
        match self {
            Self::Claude => "ai_hooks_claude",
            Self::Codex => "ai_hooks_codex",
            Self::OpenCode => "ai_hooks_opencode",
            Self::Pi => "ai_hooks_pi",
            Self::Copilot => "ai_hooks_copilot",
            Self::Grok => "ai_hooks_grok",
            Self::OhMyPi => "ai_hooks_omp",
            Self::Cursor => "ai_hooks_cursor",
            Self::Kimi => "ai_hooks_kimi",
        }
    }

    pub fn enabled(self, raw: &RawSettings) -> bool {
        let legacy_default = Self::DEFAULT.contains(&self);
        raw.bool_on(self.settings_key())
            .unwrap_or_else(|| legacy_default && raw.bool_on("ai_hooks").unwrap_or(true))
    }

    /// Explicit CLI setup/removal applies to all agents, including UI overrides.
    pub fn all_updates(enabled: bool) -> Vec<(&'static str, String)> {
        let value = if enabled { "1" } else { "0" };
        std::iter::once(("ai_hooks", value.to_owned()))
            .chain(Self::ALL.map(|agent| (agent.settings_key(), value.to_owned())))
            .collect()
    }

    /// 安装器/CLI 恢复默认的两个入口；其他 Agent 保留用户在菜单中的选择。
    pub fn setup_updates() -> Vec<(&'static str, String)> {
        std::iter::once(("ai_hooks", "1".to_owned()))
            .chain(Self::DEFAULT.map(|agent| (agent.settings_key(), "1".to_owned())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply_updates;

    #[test]
    fn legacy_global_setting_and_individual_overrides_round_trip() {
        for agent in AgentHook::ALL {
            assert_eq!(agent.enabled(&RawSettings::default()), AgentHook::DEFAULT.contains(&agent));
            assert!(!agent.enabled(&RawSettings::from_text("ai_hooks=0\n")));
        }
        let original = "# keep\nai_hooks=0\nshell=zsh\ncustom=untouched\n";
        let changed = apply_updates(original, &[(AgentHook::Codex.settings_key(), "1".into())]);
        let raw = RawSettings::from_text(&changed);
        assert!(AgentHook::Codex.enabled(&raw));
        assert!(!AgentHook::Claude.enabled(&raw));
        assert!(changed.starts_with(original));
        let disabled = apply_updates(&changed, &AgentHook::all_updates(false));
        for agent in AgentHook::ALL {
            assert!(!agent.enabled(&RawSettings::from_text(&disabled)));
        }
        let enabled = apply_updates(&disabled, &AgentHook::all_updates(true));
        for agent in AgentHook::ALL {
            assert!(agent.enabled(&RawSettings::from_text(&enabled)));
        }
    }

    #[test]
    fn disabling_one_agent_preserves_the_others() {
        let raw = RawSettings::from_text("ai_hooks_claude=off\nai_hooks_codex=invalid\n");
        assert!(!AgentHook::Claude.enabled(&raw));
        assert!(AgentHook::Codex.enabled(&raw));
        for agent in [AgentHook::OpenCode, AgentHook::Pi, AgentHook::Kimi] {
            assert!(!agent.enabled(&raw));
        }
    }

    #[test]
    fn explicit_setup_enables_only_defaults_and_preserves_optional_choices() {
        let text = apply_updates(
            "ai_hooks=0\nai_hooks_grok=1\nai_hooks_pi=0\n",
            &AgentHook::setup_updates(),
        );
        let raw = RawSettings::from_text(&text);
        assert!(AgentHook::Claude.enabled(&raw) && AgentHook::Codex.enabled(&raw));
        assert!(AgentHook::Grok.enabled(&raw));
        assert!(!AgentHook::Pi.enabled(&raw) && !AgentHook::Kimi.enabled(&raw));
    }
}
