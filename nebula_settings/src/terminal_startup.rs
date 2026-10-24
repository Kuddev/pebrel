//! Preferences used when opening an ordinary terminal window or tab.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerminalStartup {
    #[default]
    Shell,
    Welcome,
}

impl TerminalStartup {
    pub const VALUES: &'static [&'static str] = &["shell", "welcome"];

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim() {
            value if value.eq_ignore_ascii_case("shell") => Some(Self::Shell),
            value if value.eq_ignore_ascii_case("welcome") => Some(Self::Welcome),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Welcome => "welcome",
        }
    }
}

impl crate::RuntimeSettings {
    /// Check again at launch: a saved startup directory may have been removed.
    pub fn startup_directory_path(&self) -> Option<std::path::PathBuf> {
        let directory = self.startup_directory.as_deref()?.trim();
        if directory.is_empty() {
            return None;
        }
        let path = std::path::PathBuf::from(directory);
        path.is_dir().then_some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn missing_or_invalid_preference_keeps_direct_shell_startup() {
        for text in ["", "terminal_startup=", "terminal_startup=unknown"] {
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(text));
            assert_eq!(settings.terminal_startup, TerminalStartup::Shell);
        }
        assert_eq!(TerminalStartup::from_settings(" WELCOME "), Some(TerminalStartup::Welcome));
    }

    #[test]
    fn startup_choice_round_trips_without_changing_shell_or_restore_preferences() {
        for value in TerminalStartup::VALUES {
            let saved = apply_updates(
                "shell=zsh\nrestore_session=0\ncustom_data=keep\n",
                &[("terminal_startup", (*value).to_owned())],
            );
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&saved));
            assert_eq!(settings.terminal_startup.settings_value(), *value);
            assert_eq!(settings.shell.as_deref(), Some("zsh"));
            assert!(!settings.restore_session);
            assert!(saved.contains("custom_data=keep"));
        }
    }
}
