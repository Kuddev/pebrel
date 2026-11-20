//! Independent, opt-in model selection for native empty-input predictions.

use crate::RawSettings;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptSuggestions {
    pub enabled: bool,
    pub provider_id: String,
    pub model: String,
}

impl PromptSuggestions {
    pub const ENABLED_KEY: &'static str = "prompt_suggestions_enabled";
    pub const PROVIDER_KEY: &'static str = "prompt_suggestions_provider_id";
    pub const MODEL_KEY: &'static str = "prompt_suggestions_model";
    pub const MAX_VALUE_CHARS: usize = 256;

    pub(crate) fn from_raw(raw: &RawSettings) -> Self {
        let read =
            |key| raw.value(key).filter(|value| Self::valid_value(value)).unwrap_or("").to_owned();
        let mut value = Self {
            enabled: raw.bool_on(Self::ENABLED_KEY).unwrap_or(false),
            provider_id: read(Self::PROVIDER_KEY),
            model: read(Self::MODEL_KEY),
        };
        value.enabled &= value.is_configured();
        value
    }

    /// Metadata only. Provider availability and credentials belong to the application.
    pub fn is_configured(&self) -> bool {
        !self.provider_id.is_empty()
            && !self.model.is_empty()
            && Self::valid_value(&self.provider_id)
            && Self::valid_value(&self.model)
    }

    pub fn valid_value(value: &str) -> bool {
        value.chars().take(Self::MAX_VALUE_CHARS + 1).count() <= Self::MAX_VALUE_CHARS
            && !value.chars().any(|c| c.is_whitespace() || c.is_control())
    }

    /// Reject line injection and incomplete opt-ins before touching the settings file.
    pub fn updates(&self) -> Option<[(&'static str, String); 3]> {
        if !Self::valid_value(&self.provider_id)
            || !Self::valid_value(&self.model)
            || (self.enabled && !self.is_configured())
        {
            return None;
        }
        Some([
            (Self::ENABLED_KEY, (self.enabled as u8).to_string()),
            (Self::PROVIDER_KEY, self.provider_id.clone()),
            (Self::MODEL_KEY, self.model.clone()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeSettings, apply_updates};

    fn read(text: &str) -> PromptSuggestions {
        RuntimeSettings::from_raw(&RawSettings::from_text(text)).prompt_suggestions
    }

    #[test]
    fn old_and_incomplete_settings_never_enable_paid_predictions() {
        for text in [
            "",
            "ghost=1\n",
            "prompt_suggestions_enabled=yes\n",
            "prompt_suggestions_enabled=true\nprompt_suggestions_model=gpt-6.1-sol\n",
            "prompt_suggestions_enabled=invalid\nprompt_suggestions_provider_id=one\nprompt_suggestions_model=sol\n",
        ] {
            assert!(!read(text).enabled);
        }
        assert_eq!(read(""), PromptSuggestions::default());
    }

    #[test]
    fn prediction_selection_round_trips_without_changing_cli_or_shell_preferences() {
        let expected = PromptSuggestions {
            enabled: true,
            provider_id: "custom-2".into(),
            model: "vendor/gpt-6.1-sol".into(),
        };
        let original = "ghost=0\ncompletion_style=popup\ncli_model=keep\nunknown=value\n";
        let saved = apply_updates(original, &expected.updates().unwrap());
        assert_eq!(read(&saved), expected);
        assert!(saved.starts_with(original));
        let disabled = PromptSuggestions { enabled: false, ..expected };
        let saved = apply_updates(&saved, &disabled.updates().unwrap());
        assert_eq!(read(&saved), disabled);
        assert_eq!(saved.matches("prompt_suggestions_model=").count(), 1);
    }

    #[test]
    fn invalid_identifiers_cannot_inject_settings_or_trigger_requests() {
        for model in [
            "sol\nother=1".into(),
            "sol\rmodel".into(),
            "model name".into(),
            "model\u{2028}next".into(),
            "a".repeat(257),
        ] {
            let value = PromptSuggestions { enabled: true, provider_id: "one".into(), model };
            assert!(value.updates().is_none());
        }
        assert!(!read("prompt_suggestions_enabled=1\nprompt_suggestions_provider_id=one\nprompt_suggestions_model=not valid\n").enabled);
        let value = PromptSuggestions {
            enabled: true,
            provider_id: "one\nmalformed=1".into(),
            model: "sol".into(),
        };
        assert!(value.updates().is_none());
    }
}
