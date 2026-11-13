use super::*;
use crate::ai_agents::AgentKind;
use crate::ai_providers::{AiProvider, ProviderKind};

mod brand;
mod predictions;
mod select_trigger;
use select_trigger::ProviderSelectTrigger;

#[cfg(all(test, feature = "gpui-test-support"))]
mod interaction_tests {
    use super::*;

    #[gpui::test]
    fn api_key_and_json_edits_stay_in_sync_without_recursive_updates(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
        });
        let mut pane = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| SettingsPane::new(window, cx));
            pane = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let pane = pane.unwrap();
        window.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.provider_store = crate::ai_providers::ProviderStore {
                    active_id: "ui-draft-only".into(),
                    providers: vec![AiProvider::preset(ProviderKind::Custom, "ui-draft-only")],
                    cli_active: Default::default(),
                };
                pane.select_provider("ui-draft-only".into(), window, cx);
            })
        });
        window.run_until_parked();
        window.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.provider_ui
                    .api_key
                    .update(cx, |input, cx| input.replace_all("draft-key", window, cx))
            })
        });
        window.run_until_parked();
        pane.read_with(window, |pane, cx| {
            let value: serde_json::Value =
                serde_json::from_str(pane.provider_ui.auth.read(cx).value().as_ref()).unwrap();
            assert_eq!(value["OPENAI_API_KEY"], "draft-key");
        });
        window.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.provider_ui.auth.update(cx, |input, cx| {
                    input.replace_all(r#"{"OPENAI_API_KEY":"json-key","custom":true}"#, window, cx)
                })
            })
        });
        window.run_until_parked();
        assert_eq!(
            pane.read_with(window, |pane, cx| pane
                .provider_ui
                .api_key
                .read(cx)
                .value()
                .to_string()),
            "json-key"
        );
        window.update(|window, cx| {
            pane.update(cx, |pane, cx| pane.close_provider_editor(window, cx))
        });
        window.run_until_parked();
        pane.read_with(window, |pane, cx| {
            assert!(pane.provider_ui.editing.is_none());
            assert!(pane.provider_ui.auth.read(cx).value().is_empty());
            assert!(pane.provider_ui.api_key.read(cx).value().is_empty());
        });
    }
}
mod editor;
mod key;
mod view;

pub(super) const REASONING_EFFORTS: [&str; 6] = ["ultra", "max", "xhigh", "high", "medium", "low"];

pub(super) struct ProviderUi {
    agent: AgentKind,
    editing: Option<String>,
    search: Entity<InputState>,
    models: Entity<InputState>,
    effort: Entity<InputState>,
    fetching: bool,
    sequence: u64,
    focus: FocusHandle,
    task: Option<Task<()>>,
    draft_kind: Option<ProviderKind>,
    draft_wire_api: Option<&'static str>,
    advanced_open: bool,
    model_picker_open: bool,
    auth: Entity<InputState>,
    api_key: Entity<InputState>,
    config: Entity<InputState>,
    generated_config: String,
    config_dirty: bool,
    closing: bool,
    close_generation: usize,
    close_task: Option<Task<()>>,
    brands: brand::BrandIcons,
    write_task: Option<Task<()>>,
    auth_task: Option<Task<()>>,
    loading_auth: bool,
    predictions: Option<predictions::PredictionSettingsUi>,
}

impl ProviderUi {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SettingsPane>) -> Self {
        Self {
            agent: AgentKind::Claude,
            editing: None,
            search: cx.new(|cx| InputState::new(window, cx)),
            models: cx.new(|cx| InputState::new(window, cx).multi_line(true)),
            effort: cx.new(|cx| InputState::new(window, cx).placeholder("xhigh / max / ultra")),
            fetching: false,
            sequence: 0,
            focus: cx.focus_handle(),
            task: None,
            draft_kind: None,
            draft_wire_api: None,
            advanced_open: true,
            model_picker_open: false,
            auth: cx.new(|cx| InputState::new(window, cx).code_editor("json").line_number(true)),
            api_key: cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("API Key")),
            config: cx.new(|cx| InputState::new(window, cx).code_editor("toml").line_number(true)),
            generated_config: String::new(),
            config_dirty: false,
            closing: false,
            close_generation: 0,
            close_task: None,
            brands: brand::BrandIcons::default(),
            write_task: None,
            auth_task: None,
            loading_auth: false,
            predictions: None,
        }
    }

    pub(super) fn subscribe(
        &self,
        inputs: &[Entity<InputState>],
        window: &mut Window,
        cx: &mut Context<SettingsPane>,
    ) -> Vec<Subscription> {
        let mut subscriptions =
            vec![cx.subscribe(&self.search, |_: &mut SettingsPane, _, _: &InputEvent, cx| {
                cx.notify()
            })];
        for input in inputs.iter().chain([&self.models, &self.effort]) {
            subscriptions.push(cx.subscribe(
                input,
                |this: &mut SettingsPane, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.provider_codex_confirm = None;
                        this.provider_ui.sequence = this.provider_ui.sequence.wrapping_add(1);
                        this.provider_ui.task = None;
                        this.provider_ui.fetching = false;
                        this.provider_test_seq = this.provider_test_seq.wrapping_add(1);
                        this.provider_test_running = false;
                        cx.notify();
                    }
                },
            ));
        }
        subscriptions.push(cx.subscribe(
            &self.config,
            |this: &mut SettingsPane, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.provider_ui.config_dirty =
                        this.provider_ui.config.read(cx).value().as_ref()
                            != this.provider_ui.generated_config;
                    this.provider_codex_confirm = None;
                    this.provider_ui.sequence = this.provider_ui.sequence.wrapping_add(1);
                    this.provider_ui.task = None;
                    this.provider_ui.fetching = false;
                    cx.notify();
                }
            },
        ));
        for (field, input) in [(3, &inputs[3]), (4, &inputs[4])] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this: &mut SettingsPane, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.sync_provider_config_fields(field, window, cx);
                    }
                },
            ));
        }
        for (from_key, input) in [(true, &self.api_key), (false, &self.auth)] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this: &mut SettingsPane, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.provider_ui.sequence = this.provider_ui.sequence.wrapping_add(1);
                        this.provider_ui.task = None;
                        this.provider_ui.fetching = false;
                        this.sync_provider_auth(from_key, window, cx);
                    }
                },
            ));
        }
        subscriptions
    }
}

impl SettingsPane {
    fn sync_provider_auth(&mut self, from_key: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_ui.editing.is_none() {
            return;
        }
        let text = self.provider_ui.auth.read(cx).value().to_string();
        let Ok(mut document) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        let Some(object) = document.as_object_mut() else {
            return;
        };
        let name = if object.contains_key("OPENAI_API_KEY") {
            "OPENAI_API_KEY"
        } else if object.contains_key("ANTHROPIC_API_KEY")
            || self.provider_ui.agent == AgentKind::Claude
        {
            "ANTHROPIC_API_KEY"
        } else {
            "OPENAI_API_KEY"
        };
        let json_key = object.get(name).and_then(|v| v.as_str()).unwrap_or("");
        let input_key = self.provider_ui.api_key.read(cx).value().to_string();
        if json_key == input_key {
            return;
        }
        if from_key {
            object.insert(name.into(), serde_json::Value::String(input_key));
            let text = serde_json::to_string_pretty(&document).unwrap_or_default();
            self.provider_ui.auth.update(cx, |input, cx| input.set_value(text, window, cx));
        } else {
            let key = json_key.to_owned();
            self.provider_ui.api_key.update(cx, |input, cx| input.set_value(key, window, cx));
        }
        self.provider_codex_confirm = None;
        self.provider_ui.sequence = self.provider_ui.sequence.wrapping_add(1);
        self.provider_ui.task = None;
        self.provider_ui.fetching = false;
        cx.notify();
    }

    pub(super) fn switch_provider(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.provider_ui.write_task.is_some() {
            return;
        }
        let Some(provider) = self.provider_store.providers.iter().find(|p| p.id == id).cloned()
        else {
            return;
        };
        if !matches!(provider.agent_kind(), AgentKind::Claude | AgentKind::Codex) {
            self.provider_status =
                Some(ProviderStatus::Notice(crate::i18n::Message::ProvidersCliUnsupported));
            cx.notify();
            return;
        }
        if self.provider_codex_confirm.as_deref() != Some(id) {
            self.provider_codex_confirm = Some(id.to_owned());
            self.provider_status = Some(if provider.agent_kind() == AgentKind::Claude {
                ProviderStatus::ClaudeConfirmation
            } else {
                ProviderStatus::CodexConfirmation
            });
            cx.notify();
            return;
        }
        self.apply_provider_switch(self.provider_store.clone(), provider, cx);
    }

    fn apply_provider_switch(
        &mut self,
        mut next: crate::ai_providers::ProviderStore,
        provider: AiProvider,
        cx: &mut Context<Self>,
    ) {
        self.provider_codex_confirm = None;
        self.provider_status =
            Some(ProviderStatus::Notice(crate::i18n::Message::ProvidersApplying));
        let work = cx.background_executor().spawn(async move {
            let claude = provider.agent_kind() == AgentKind::Claude;
            let path = if claude {
                crate::ai_providers::apply_claude_provider(&provider)
            } else {
                crate::codex_config::apply_provider(&provider)
            }?;
            next.cli_active.insert(provider.agent_kind().slug().to_owned(), provider.id.clone());
            next.active_id = provider.id;
            crate::ai_providers::save(&next).map_err(|error| error.to_string())?;
            Ok::<_, String>((
                next,
                if claude {
                    ProviderStatus::AppliedToClaude(path)
                } else {
                    ProviderStatus::AppliedToCodex(path)
                },
            ))
        });
        // 写入一旦开始就完成收尾；保留实体，避免关闭设置导致结果丢失。
        let owner = cx.entity();
        self.provider_ui.write_task = Some(cx.spawn(async move |_, cx| {
            let result = work.await;
            let _ = owner.update(cx, |this, cx| {
                this.provider_ui.write_task = None;
                match result {
                    Ok((store, status)) => {
                        this.provider_store = store;
                        this.provider_status = Some(status);
                    },
                    Err(error) => this.provider_status = Some(ProviderStatus::Error(error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn active_provider_index(&self) -> Option<usize> {
        self.provider_store
            .providers
            .iter()
            .position(|provider| Some(&provider.id) == self.provider_ui.editing.as_ref())
            .or_else(|| {
                self.provider_store
                    .providers
                    .iter()
                    .position(|p| p.id == self.provider_store.active_id)
            })
            .or_else(|| (!self.provider_store.providers.is_empty()).then_some(0))
    }

    pub(super) fn sync_provider_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.active_provider_index() else { return };
        let draft =
            crate::ai_providers::ProviderMetadataDraft::from(&self.provider_store.providers[index]);
        let provider = self.provider_store.providers[index].clone();
        let key_name = if provider.agent_kind() == AgentKind::Claude {
            "ANTHROPIC_API_KEY"
        } else {
            "OPENAI_API_KEY"
        };
        let auth =
            serde_json::to_string_pretty(&serde_json::json!({key_name: provider.api_key_hint}))
                .unwrap_or_default();
        self.provider_ui.auth.update(cx, |input, cx| input.set_value(auth, window, cx));
        self.sync_provider_auth(false, window, cx);
        if provider.api_key_set || provider.auth_document_set {
            self.provider_ui.loading_auth = true;
            let id = provider.id.clone();
            let sequence = self.provider_ui.sequence;
            let expected_id = id.clone();
            let work = cx
                .background_executor()
                .spawn(async move { crate::ai_providers::load_auth_document(&id) });
            self.provider_ui.auth_task = Some(cx.spawn_in(window, async move |this, cx| {
                let result = work.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.provider_ui.sequence != sequence
                        || this.provider_ui.editing.as_ref() != Some(&expected_id)
                    {
                        // 用户在系统凭据读取期间编辑了草稿；旧读取结果不得覆盖
                        // 新输入，但也不能把保存按钮永久锁在 loading 状态。
                        this.provider_ui.loading_auth = false;
                        this.provider_ui.auth_task = None;
                        cx.notify();
                        return;
                    }
                    this.provider_ui.loading_auth = false;
                    this.provider_ui.auth_task = None;
                    match result {
                        Ok(Some(text)) => {
                            let text = serde_json::from_str::<serde_json::Value>(&text)
                                .and_then(|v| serde_json::to_string_pretty(&v))
                                .unwrap_or_else(|_| text.to_string());
                            this.provider_ui
                                .auth
                                .update(cx, |input, cx| input.set_value(text, window, cx));
                            this.sync_provider_auth(false, window, cx);
                        },
                        Ok(None) => {},
                        Err(error) => {
                            this.provider_status = Some(ProviderStatus::Error(error.to_string()))
                        },
                    }
                    cx.notify();
                });
            }));
        }
        self.provider_ui.generated_config = crate::ai_providers::config_text(&provider);
        self.provider_ui.config_dirty = false;
        self.provider_ui.config.update(cx, |input, cx| {
            input.set_value(self.provider_ui.generated_config.clone(), window, cx)
        });
        self.provider_ui
            .models
            .update(cx, |input, cx| input.set_value(provider.models.join("\n"), window, cx));
        self.provider_ui
            .effort
            .update(cx, |input, cx| input.set_value(provider.reasoning_effort.clone(), window, cx));
        let values = [draft.name, draft.note, draft.website_url, draft.base_url, draft.model];
        for (input, value) in self.provider_inputs.iter().zip(values) {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }

    fn sync_provider_config_fields(
        &mut self,
        field: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.provider_ui.editing.is_none() {
            return;
        }
        let Some(index) = self.active_provider_index() else {
            return;
        };
        let mut provider = self.provider_store.providers[index].clone();
        let text = self.provider_ui.config.read(cx).value().to_string();
        if crate::ai_providers::apply_config_draft(&mut provider, &text).is_err() {
            return;
        }
        if field == 3 {
            provider.base_url = self.provider_inputs[3].read(cx).value().to_string();
        }
        if field == 4 {
            provider.model = self.provider_inputs[4].read(cx).value().to_string();
        }
        let text = crate::ai_providers::config_text(&provider);
        self.provider_ui.generated_config = text.clone();
        self.provider_ui.config.update(cx, |input, cx| input.set_value(text, window, cx));
    }

    pub(super) fn select_provider(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.provider_ui.write_task.is_some() {
            return;
        }
        self.provider_ui.auth_task = None;
        self.provider_ui.loading_auth = false;
        if self.provider_store.providers.iter().any(|provider| provider.id == id) {
            self.provider_ui.close_generation = self.provider_ui.close_generation.wrapping_add(1);
            self.provider_ui.closing = false;
            self.provider_ui.close_task = None;
            self.provider_ui.editing = Some(id);
            self.provider_ui.draft_kind = None;
            self.provider_ui.draft_wire_api = None;
            self.provider_ui.model_picker_open = false;
            self.provider_ui.sequence = self.provider_ui.sequence.wrapping_add(1);
            self.provider_ui.task = None;
            self.provider_ui.fetching = false;
            self.provider_test_running = false;
            self.provider_test_seq = self.provider_test_seq.wrapping_add(1);
            self.provider_ui.focus.focus(window, cx);
            self.provider_codex_confirm = None;
            self.provider_status = None;
            self.sync_provider_inputs(window, cx);
            cx.notify();
        }
    }

    fn provider_draft(&self, cx: &Context<Self>) -> Result<AiProvider, String> {
        let index = self.active_provider_index().ok_or("No provider selected")?;
        let values: Vec<String> =
            self.provider_inputs.iter().map(|input| input.read(cx).value().to_string()).collect();
        let draft = crate::ai_providers::ProviderMetadataDraft {
            name: values[0].clone(),
            note: values[1].clone(),
            website_url: values[2].clone(),
            base_url: values[3].clone(),
            model: values[4].clone(),
        };
        let mut snapshot = self.provider_store.providers[index].clone();
        let provider = &mut snapshot;
        provider.agent = self.provider_ui.agent.slug().to_owned();
        if let Some(kind) = self.provider_ui.draft_kind {
            provider.kind = kind;
        }
        if let Some(wire_api) = self.provider_ui.draft_wire_api {
            provider.wire_api = wire_api.to_owned();
        }
        crate::ai_providers::apply_metadata_draft(provider, draft);
        let config = self.provider_ui.config.read(cx).value().to_string();
        crate::ai_providers::apply_config_draft(provider, &config)
            .map_err(|error| format!("config.toml: {error}"))?;
        provider.models = crate::ai_providers::parse_model_list(
            self.provider_ui.models.read(cx).value().as_ref(),
        );
        if provider
            .subagent_model
            .as_ref()
            .is_some_and(|model| !crate::ai_providers::configured_models(provider).contains(model))
        {
            return Err(
                "The selected subagent model must remain in this provider's model list".into()
            );
        }
        Ok(snapshot)
    }

    pub(super) fn save_provider_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_ui.write_task.is_some() || self.provider_ui.loading_auth {
            return;
        }
        let provider = match self.provider_draft(cx) {
            Ok(provider) => provider,
            Err(error) => {
                self.provider_status = Some(ProviderStatus::Error(error));
                cx.notify();
                return;
            },
        };
        let store = self.provider_store.clone();
        let auth = zeroize::Zeroizing::new(self.provider_ui.auth.read(cx).value().to_string());
        self.provider_status = Some(ProviderStatus::Notice(crate::i18n::Message::ProvidersSaving));
        let work = cx.background_executor().spawn(async move {
            let id = provider.id.clone();
            let store = crate::ai_providers::save_provider_draft(store, provider, &auth)?;
            let provider = store.providers.iter().find(|p| p.id == id).expect("saved provider");
            let applied = if store.cli_active.get(provider.agent_kind().slug()) == Some(&id) {
                if provider.agent_kind() == AgentKind::Claude {
                    crate::ai_providers::apply_claude_provider(provider).map(|_| ())
                } else {
                    crate::codex_config::apply_provider(provider).map(|_| ())
                }
            } else {
                Ok(())
            };
            Ok::<_, std::io::Error>((store, applied))
        });
        let owner = cx.entity();
        self.provider_ui.write_task = Some(cx.spawn_in(window, async move |_, cx| {
            let result = work.await;
            let _ = owner.update(cx, |this, cx| {
                this.provider_ui.write_task = None;
                match result {
                    Ok((store, applied)) => {
                        this.provider_store = store;
                        this.provider_status = Some(match applied {
                            Ok(()) => ProviderStatus::Saved,
                            Err(error) => ProviderStatus::Error(error),
                        });
                    },
                    Err(error) => {
                        this.provider_status = Some(ProviderStatus::Error(error.to_string()))
                    },
                }
                cx.notify();
            });
            let _ = cx.update(|window, cx| {
                owner.update(cx, |this, cx| {
                    if matches!(this.provider_status, Some(ProviderStatus::Saved)) {
                        this.close_provider_editor(window, cx);
                    }
                });
            });
        }));
        cx.notify();
    }

    pub(super) fn add_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_ui.write_task.is_some() {
            return;
        }
        let id = crate::ai_providers::next_custom_id(&self.provider_store);
        let kind = match self.provider_ui.agent {
            AgentKind::Claude => ProviderKind::Anthropic,
            AgentKind::Gemini => ProviderKind::Google,
            _ => ProviderKind::Custom,
        };
        let mut provider = AiProvider::preset(kind, &id);
        provider.agent = self.provider_ui.agent.slug().to_owned();
        let mut next = self.provider_store.clone();
        next.providers.push(provider);
        match crate::ai_providers::save(&next) {
            Ok(()) => {
                self.provider_store = next;
                self.select_provider(id, window, cx);
                self.provider_status = Some(ProviderStatus::Added);
            },
            Err(error) => self.provider_status = Some(ProviderStatus::Error(error.to_string())),
        }
        cx.notify();
    }

    pub(super) fn delete_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_ui.write_task.is_some() {
            return;
        }
        let Some(index) = self.active_provider_index() else { return };
        if self.provider_store.providers.len() <= 1 {
            self.provider_status = Some(ProviderStatus::AtLeastOneRequired);
            cx.notify();
            return;
        }
        let id = self.provider_store.providers[index].id.clone();
        let mut store = self.provider_store.clone();
        let work = cx.background_executor().spawn(async move {
            crate::ai_providers::remove_provider(&mut store, &id).map(|()| store)
        });
        let owner = cx.entity();
        self.provider_ui.write_task = Some(cx.spawn_in(window, async move |_, cx| {
            let result = work.await;
            let _ = owner.update(cx, |this, cx| {
                this.provider_ui.write_task = None;
                match result {
                    Ok(store) => {
                        this.provider_store = store;
                        this.provider_status = Some(ProviderStatus::Deleted);
                    },
                    Err(error) => {
                        this.provider_status = Some(ProviderStatus::Error(error.to_string()))
                    },
                }
                cx.notify();
            });
            let _ = cx.update(|window, cx| {
                owner.update(cx, |this, cx| {
                    if matches!(this.provider_status, Some(ProviderStatus::Deleted)) {
                        this.close_provider_editor(window, cx);
                    }
                })
            });
        }));
        cx.notify();
    }

    pub(super) fn toggle_provider_flag(
        &mut self,
        flag: &'static str,
        value: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.active_provider_index() else { return };
        let old = self.provider_store.providers[index].clone();
        let provider = &mut self.provider_store.providers[index];
        match flag {
            "enabled" => provider.enabled = value,
            "codex_goals" => provider.codex_goals = value,
            "codex_remote_compaction" => provider.codex_remote_compaction = value,
            _ => return,
        }
        self.provider_codex_confirm = None;
        if let Err(error) = crate::ai_providers::save(&self.provider_store) {
            self.provider_store.providers[index] = old;
            self.provider_status = Some(ProviderStatus::Error(error.to_string()));
        }
        cx.notify();
    }
}
