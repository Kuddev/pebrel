//! Prediction-specific drafts reuse provider metadata without changing CLI configuration.

use super::*;
use crate::i18n::Message;
use gpui_component::menu::PopupMenuItem;
use nebula_settings::PromptSuggestions;

pub(super) struct PredictionSettingsUi {
    provider_id: String,
    model: Entity<InputState>,
    error: Option<Message>,
    save_error: Option<String>,
    saved: bool,
    _subscription: Subscription,
}

fn available(provider: &AiProvider) -> bool {
    crate::prompt_suggestions::client::supported(provider)
        && !provider.base_url.trim().is_empty()
        && (!provider.kind.requires_api_key() || provider.api_key_set)
}

impl SettingsPane {
    pub(super) fn prepare_prediction_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.provider_ui.predictions.is_some() {
            return;
        }
        let settings = &self.runtime.prompt_suggestions;
        let model = cx.new(|cx| InputState::new(window, cx).default_value(settings.model.clone()));
        let subscription = cx.subscribe(&model, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(ui) = this.provider_ui.predictions.as_mut() {
                    ui.error = None;
                    ui.save_error = None;
                    ui.saved = false;
                }
                cx.notify();
            }
        });
        self.provider_ui.predictions = Some(PredictionSettingsUi {
            provider_id: settings.provider_id.clone(),
            model,
            error: None,
            save_error: None,
            saved: false,
            _subscription: subscription,
        });
    }

    fn save_prediction_settings(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(ui) = self.provider_ui.predictions.as_ref() else { return };
        let next = PromptSuggestions {
            enabled,
            provider_id: ui.provider_id.clone(),
            model: ui.model.read(cx).value().trim().to_owned(),
        };
        let provider_available = self
            .provider_store
            .providers
            .iter()
            .any(|provider| provider.id == next.provider_id && available(provider));
        let validation = if !provider_available {
            Some(Message::PromptSuggestionsChooseProvider)
        } else if !next.is_configured() {
            Some(Message::PromptSuggestionsInvalidModel)
        } else {
            None
        };
        if let Some(error) = validation {
            self.provider_ui.predictions.as_mut().unwrap().error = Some(error);
            cx.notify();
            return;
        }
        let Some(updates) = next.updates() else { return };
        let result = self.try_persist(&updates, cx);
        let ui = self.provider_ui.predictions.as_mut().unwrap();
        ui.error = None;
        ui.save_error = result.as_ref().err().map(ToString::to_string);
        ui.saved = result.is_ok();
        cx.notify();
    }

    fn set_prediction_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled {
            self.save_prediction_settings(true, cx);
        } else {
            // Disabling must remain possible even if a provider was removed or a draft is invalid.
            let result = self.try_persist(&[(PromptSuggestions::ENABLED_KEY, "0".into())], cx);
            if let Some(ui) = self.provider_ui.predictions.as_mut() {
                ui.error = None;
                ui.save_error = result.as_ref().err().map(ToString::to_string);
                ui.saved = result.is_ok();
            }
            cx.notify();
        }
    }

    pub(super) fn prediction_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let Some(ui) = self.provider_ui.predictions.as_ref() else { return div() };
        let language = crate::gpui_shell::config::ui_language(cx);
        let providers: Vec<_> = self
            .provider_store
            .providers
            .iter()
            .filter(|provider| available(provider))
            .cloned()
            .collect();
        let selected = ui.provider_id.clone();
        let provider_label = providers
            .iter()
            .find(|provider| provider.id == selected)
            .map(|provider| provider.name.clone())
            .unwrap_or_else(|| language.text(Message::PromptSuggestionsChooseProvider).into());
        let owner = cx.entity().downgrade();
        let picker = Button::new("prediction-provider")
            .debug_selector(|| "prediction-provider".to_owned())
            .label(provider_label)
            .w_full()
            .h(px(34.0))
            .disabled(providers.is_empty())
            .map(ProviderSelectTrigger::new)
            .dropdown_menu(move |mut menu, _, _| {
                for provider in &providers {
                    let owner = owner.clone();
                    let provider = provider.clone();
                    menu = menu.item(
                        PopupMenuItem::new(provider.name.clone())
                            .checked(provider.id == selected)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if let Some(ui) = this.provider_ui.predictions.as_mut() {
                                        if ui.provider_id != provider.id {
                                            ui.provider_id = provider.id.clone();
                                            ui.model.update(cx, |input, cx| {
                                                input.set_value(provider.model.clone(), window, cx);
                                            });
                                        }
                                        ui.error = None;
                                        ui.save_error = None;
                                        ui.saved = false;
                                    }
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            });
        let enabled = self.runtime.prompt_suggestions.enabled;
        let model = ui.model.read(cx).value();
        let draft = PromptSuggestions {
            enabled,
            provider_id: ui.provider_id.clone(),
            model: model.trim().into(),
        };
        let dirty = draft != self.runtime.prompt_suggestions;
        let configured = draft.is_configured()
            && self
                .provider_store
                .providers
                .iter()
                .any(|provider| provider.id == draft.provider_id && available(provider));
        let status = if let Some(error) = &ui.save_error {
            Some((language.format(Message::PromptSuggestionsSaveFailed, &[("error", error)]), true))
        } else if let Some(error) = ui.error {
            Some((language.text(error).to_owned(), true))
        } else if !model.trim().is_empty() && !PromptSuggestions::valid_value(model.trim()) {
            Some((language.text(Message::PromptSuggestionsInvalidModel).to_owned(), true))
        } else if ui.saved && !dirty {
            Some((language.text(Message::PromptSuggestionsSaved).to_owned(), false))
        } else if self.provider_store.providers.iter().all(|provider| !available(provider)) {
            Some((language.text(Message::PromptSuggestionsNoProviders).to_owned(), false))
        } else {
            None
        };
        div()
            .w_full()
            .pb_4()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.6))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_4()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .child(language.text(Message::PromptSuggestionsTitle)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(language.text(Message::PromptSuggestionsDescription)),
                            ),
                    )
                    .child(
                        crate::gpui_shell::widgets::NebulaSwitch::new("prompt-suggestions-enabled")
                            .checked(enabled)
                            .disabled(!enabled && !configured)
                            .on_click(cx.listener(|this, value: &bool, _, cx| {
                                this.set_prediction_enabled(*value, cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .mt_3()
                    .gap_3()
                    .flex_wrap()
                    .items_end()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(180.0))
                            .gap_1()
                            .child(language.text(Message::PromptSuggestionsProvider))
                            .child(picker),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(180.0))
                            .gap_1()
                            .child(language.text(Message::PromptSuggestionsModel))
                            .child(Input::new(&ui.model).h(px(34.0))),
                    )
                    .child(
                        Button::new("prediction-save")
                            .debug_selector(|| "prediction-save".to_owned())
                            .label(language.text(Message::CommonSave))
                            .h(px(34.0))
                            .disabled(!dirty || !configured)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.save_prediction_settings(enabled, cx);
                            })),
                    ),
            )
            .when_some(status, |view, (text, error)| {
                view.child(
                    div()
                        .mt_2()
                        .text_size(px(12.0))
                        .text_color(if error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(text),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prediction_picker_requires_a_usable_supported_provider() {
        let mut provider = AiProvider::preset(ProviderKind::Custom, "prediction-test");
        provider.model = "main-model".into();
        provider.base_url = "https://example.invalid/v1".into();
        assert!(!available(&provider));
        provider.api_key_set = true;
        assert!(available(&provider));
        provider.enabled = false;
        assert!(!available(&provider));
        provider.enabled = true;
        provider.kind = ProviderKind::Google;
        assert!(!available(&provider));
        provider.kind = ProviderKind::AzureOpenAi;
        assert!(!available(&provider));
        provider.kind = ProviderKind::Ollama;
        provider.api_key_set = false;
        assert!(available(&provider));
    }
}
