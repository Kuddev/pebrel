use super::*;
use crate::i18n::Message;
use gpui_component::menu::PopupMenuItem;

pub(super) fn agent_icon(agent: AgentKind) -> Icon {
    use crate::gpui_shell::assets::nav;
    let path = match agent {
        AgentKind::Claude => nav::AGENT_CLAUDE,
        AgentKind::Codex => nav::AGENT_OPENAI,
        AgentKind::Gemini => "icons/nebula-provider-gemini.svg",
        AgentKind::OpenCode => nav::AGENT_OPENCODE,
        AgentKind::Pi => nav::AGENT_PI,
        AgentKind::Grok => nav::AGENT_GROK,
        AgentKind::Kimi => nav::AGENT_KIMI,
        AgentKind::OhMyPi => nav::AGENT_OMP,
        AgentKind::Cursor => nav::AGENT_CURSOR,
        AgentKind::Copilot => nav::AGENT_COPILOT,
        _ => return Icon::new(IconName::Bot),
    };
    Icon::default().path(path)
}

fn provider_icon(kind: ProviderKind) -> Icon {
    use crate::gpui_shell::assets::nav;
    let path = match kind {
        ProviderKind::OpenAi => nav::AGENT_OPENAI,
        ProviderKind::Anthropic => "icons/nebula-provider-anthropic.svg",
        ProviderKind::Google => "icons/nebula-provider-gemini.svg",
        ProviderKind::Ollama => "icons/nebula-provider-ollama.svg",
        ProviderKind::OpenRouter => "icons/nebula-provider-openrouter.svg",
        ProviderKind::Qwen => "icons/nebula-provider-qwen.svg",
        ProviderKind::DeepSeek => "icons/nebula-provider-deepseek.svg",
        ProviderKind::Kimi => nav::AGENT_KIMI,
        ProviderKind::Zhipu => "icons/nebula-provider-zhipu.svg",
        ProviderKind::Doubao => "icons/nebula-provider-doubao.svg",
        ProviderKind::Mimo => "icons/nebula-provider-xiaomimimo.svg",
        ProviderKind::AzureOpenAi => "icons/nebula-provider-azure.svg",
        ProviderKind::Custom => return Icon::new(IconName::Globe),
    };
    Icon::default().path(path)
}

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn section_providers(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.prepare_provider_brands(window, cx);
        self.prepare_prediction_settings(window, cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let available =
            (f32::from(window.viewport_size().width) - SETTINGS_NAV_WIDTH - 24.0).max(240.0);
        let sidebar_width = if available >= 1280.0 {
            280.0
        } else if available >= 1000.0 {
            220.0
        } else {
            0.0
        };
        let table_width = available - sidebar_width - 48.0;
        let show_subscription = table_width >= 900.0;
        let muted = cx.theme().muted_foreground;
        let agent = self.provider_ui.agent;
        let query = self.provider_ui.search.read(cx).value().to_lowercase();
        let providers: Vec<_> = self
            .provider_store
            .providers
            .iter()
            .filter(|p| p.agent_kind() == agent)
            .filter(|p| {
                query.is_empty()
                    || format!("{} {} {}", p.name, p.base_url, p.model)
                        .to_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect();
        let count = providers.len();
        let surface = cx.theme().secondary.opacity(0.45);
        let border = cx.theme().border.opacity(0.6);
        let mut sidebar = v_flex()
            .w(px(sidebar_width))
            .flex_shrink_0()
            .min_h(px(680.0))
            .px_2()
            .py_4()
            .gap_1()
            .border_r_1()
            .border_color(border)
            .child(div().text_size(px(14.0)).px_3().mb_3().child("Agents"));
        for kind in [
            AgentKind::Claude,
            AgentKind::Codex,
            AgentKind::Gemini,
            AgentKind::OpenCode,
            AgentKind::Grok,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Kimi,
        ] {
            let total =
                self.provider_store.providers.iter().filter(|p| p.agent_kind() == kind).count();
            sidebar = sidebar.child(
                Button::new(SharedString::from(format!("provider-agent-{}", kind.slug())))
                    .ghost()
                    .w_full()
                    .h(px(68.0))
                    .px_3()
                    .selected(kind == agent)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_center()
                            .child(
                                div()
                                    .size(px(32.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(self.provider_brand(kind)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .items_start()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(14.0))
                                                    .truncate()
                                                    .child(kind.display_name()),
                                            )
                                            .when(self.provider_needs_install(kind), |row| {
                                                row.child(
                                                    Button::new(SharedString::from(format!(
                                                        "provider-download-{}",
                                                        kind.slug()
                                                    )))
                                                    .ghost()
                                                    .size(px(26.0))
                                                    .rounded_full()
                                                    .icon(
                                                        Icon::default()
                                                            .path("icons/nebula-download.svg")
                                                            .size(px(14.0)),
                                                    )
                                                    .tooltip(
                                                        language.text(Message::ProvidersDownload),
                                                    )
                                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                        cx.stop_propagation()
                                                    })
                                                    .on_click(cx.listener(move |_, _, _, cx| {
                                                        cx.stop_propagation();
                                                        cx.emit(SettingsPaneEvent::InstallAgent(
                                                            kind,
                                                        ));
                                                    })),
                                                )
                                            }),
                                    )
                                    .child(
                                        div().text_size(px(11.0)).text_color(muted).child(
                                            language
                                                .text(Message::ProvidersConfigCount)
                                                .replace("{count}", &total.to_string()),
                                        ),
                                    ),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.provider_ui.agent = kind;
                        this.provider_status = None;
                        cx.notify();
                    })),
            );
        }
        let mut table = v_flex()
            .w_full()
            .min_w(px(if show_subscription { 880.0 } else { 720.0 }))
            .rounded(px(12.0))
            .border_1()
            .border_color(border)
            .overflow_hidden()
            .child(
                h_flex()
                    .h(px(44.0))
                    .px_4()
                    .gap_4()
                    .bg(surface)
                    .border_b_1()
                    .border_color(border)
                    .text_size(px(12.0))
                    .text_color(muted)
                    .child(div().w(px(12.0)).flex_shrink_0())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(180.0))
                            .child(language.text(Message::ProvidersNameUrl)),
                    )
                    .child(
                        div()
                            .w(px(144.0))
                            .flex_shrink_0()
                            .text_center()
                            .child(language.text(Message::ProvidersCurrentModel)),
                    )
                    .child(
                        div()
                            .w(px(100.0))
                            .flex_shrink_0()
                            .text_center()
                            .child(language.text(Message::ProvidersEffort)),
                    )
                    .when(show_subscription, |row| {
                        row.child(
                            div()
                                .w(px(144.0))
                                .flex_shrink_0()
                                .child(language.text(Message::ProvidersSubscription)),
                        )
                    })
                    .child(
                        div()
                            .w(px(56.0))
                            .flex_shrink_0()
                            .text_center()
                            .child(language.text(Message::ProvidersSubagent)),
                    )
                    .child(
                        div()
                            .w(px(112.0))
                            .flex_shrink_0()
                            .text_right()
                            .child(language.text(Message::ProvidersActions)),
                    ),
            );
        for provider in providers {
            table = table.child(self.provider_table_row(provider, show_subscription, cx));
        }
        if count == 0 {
            table = table.child(
                div()
                    .w_full()
                    .min_h(px(120.0))
                    .px_4()
                    .py_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .text_color(muted)
                    .child(language.text(Message::ProvidersEmpty)),
            );
        }
        let mut metrics = h_flex().gap_4().w_full().when(table_width < 450.0, |row| row.flex_col());
        for (label, icon) in [
            (Message::ProvidersBalance, "icons/nebula-wallet.svg"),
            (Message::ProvidersSpent, "icons/nebula-credit-card.svg"),
            (Message::ProvidersExpiry, "icons/nebula-clock.svg"),
        ] {
            metrics = metrics.child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .p_3()
                    .gap_1()
                    .rounded(px(12.0))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .gap_1()
                            .text_size(px(11.0))
                            .text_color(muted)
                            .child(Icon::default().path(icon).size(px(14.0)))
                            .child(language.text(label)),
                    )
                    .child(div().text_size(px(20.0)).child("—")),
            );
        }
        let main = v_flex()
            .flex_1()
            .min_w_0()
            .child(
                h_flex()
                    .min_h(px(64.0))
                    .py_3()
                    .flex_wrap()
                    .px_6()
                    .gap_3()
                    .child(div().text_size(px(18.0)).child(agent.display_name()))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(muted)
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(surface)
                            .child(
                                language
                                    .text(Message::ProvidersConfigCount)
                                    .replace("{count}", &count.to_string()),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div().w(px(192.0)).child(
                            Input::new(&self.provider_ui.search)
                                .h(px(32.0))
                                .text_size(px(14.0))
                                .prefix(IconName::Search),
                        ),
                    )
                    .child(
                        Button::new("provider-add")
                            .primary()
                            .h(px(32.0))
                            .label(language.text(Message::ProvidersAdd))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_provider(window, cx)),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .px_6()
                    .pt_6()
                    .pb_6()
                    .gap_4()
                    .child(self.prediction_settings(cx))
                    .child(metrics)
                    .child(div().w_full().min_w_0().overflow_x_scrollbar().child(table)),
            )
            .when_some(
                self.provider_status.clone().filter(|_| self.provider_ui.editing.is_none()),
                |main, status| {
                    main.child(
                        div()
                            .px_6()
                            .text_size(px(12.0))
                            .text_color(if status.is_error() { cx.theme().danger } else { muted })
                            .child(status.text(language)),
                    )
                },
            );
        let compact_owner = cx.entity().downgrade();
        let compact_agents = Button::new("provider-agent-compact")
            .ghost()
            .w_full()
            .h(px(38.0))
            .child(
                h_flex()
                    .flex_1()
                    .gap_2()
                    .child(agent_icon(agent).size(px(20.0)))
                    .child(agent.display_name()),
            )
            .map(ProviderSelectTrigger::new)
            .dropdown_menu(move |mut menu, _, _| {
                for kind in [
                    AgentKind::Claude,
                    AgentKind::Codex,
                    AgentKind::Gemini,
                    AgentKind::OpenCode,
                    AgentKind::Grok,
                    AgentKind::Pi,
                    AgentKind::Hermes,
                    AgentKind::Kimi,
                ] {
                    let owner = compact_owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(kind.display_name()).checked(agent == kind).on_click(
                            move |_, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.provider_ui.agent = kind;
                                    cx.notify();
                                });
                            },
                        ),
                    );
                }
                menu
            });
        h_flex()
            .w_full()
            .min_w_0()
            .items_start()
            .text_size(px(14.0))
            .when(sidebar_width > 0.0, |row| row.child(sidebar))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .when(sidebar_width == 0.0, |column| {
                        column.child(
                            h_flex()
                                .px_6()
                                .pt_3()
                                .gap_2()
                                .child(div().flex_1().min_w_0().child(compact_agents))
                                .when(self.provider_needs_install(agent), |row| {
                                    row.child(
                                        Button::new("provider-compact-install")
                                            .ghost()
                                            .size(px(32.0))
                                            .icon(
                                                Icon::default()
                                                    .path("icons/nebula-download.svg")
                                                    .size(px(16.0)),
                                            )
                                            .tooltip(language.text(Message::ProvidersDownload))
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.emit(SettingsPaneEvent::InstallAgent(agent))
                                            })),
                                    )
                                }),
                        )
                    })
                    .child(main),
            )
    }

    fn provider_table_row(
        &self,
        provider: AiProvider,
        show_subscription: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let active =
            self.provider_store.cli_active.get(provider.agent_kind().slug()) == Some(&provider.id);
        let owner = cx.entity().downgrade();
        let models = crate::ai_providers::configured_models(&provider);
        let id = provider.id.clone();
        let model_id = id.clone();
        let effort_id = id.clone();
        let sub_id = id.clone();
        let current_model = provider.model.clone();
        let model_choices = models.clone();
        let model_owner = owner.clone();
        let effort_owner = owner.clone();
        let selected_subagent = provider.subagent_model.clone();
        let efforts = REASONING_EFFORTS.map(str::to_owned);
        let effort = provider.reasoning_effort.clone();
        let hover_group: SharedString = format!("provider-hover-{id}").into();
        let switch_id = id.clone();
        let url = provider.base_url.clone();
        let valid_url = url.starts_with("https://") || url.starts_with("http://");
        let link_group: SharedString = format!("provider-url-{id}").into();
        let primary = cx.theme().primary;
        let hover_bg = cx.theme().foreground.opacity(0.025);
        h_flex()
            .id(SharedString::from(format!("provider-row-{id}")))
            .group(hover_group.clone())
            .w_full()
            .min_w_0()
            .px_4()
            .py_3()
            .gap_4()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.35))
            .hover(move |row| row.bg(hover_bg))
            .child(div().w(px(12.0)).flex_shrink_0().flex().justify_center().child(
                div().size(px(8.0)).rounded_full().bg(if active {
                    cx.theme().success
                } else {
                    muted.opacity(0.45)
                }),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(180.0))
                    .gap_1()
                    .child(div().text_size(px(14.0)).truncate().child(provider.name.clone()))
                    .child(
                        Button::new(SharedString::from(format!("provider-url-link-{id}")))
                            .text()
                            .group(link_group.clone())
                            .w_full()
                            .h(px(20.0))
                            .px_0()
                            .disabled(!valid_url)
                            .tooltip(url.clone())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_left()
                                    .text_size(px(11.0))
                                    .text_color(muted)
                                    .truncate()
                                    .group_hover(link_group, move |text| {
                                        text.text_color(primary).underline()
                                    })
                                    .child(
                                        url.trim_start_matches("https://")
                                            .trim_start_matches("http://")
                                            .to_owned(),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                cx.open_url(&url);
                            }),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("provider-model-{id}")))
                    .ghost()
                    .w(px(144.0))
                    .flex_shrink_0()
                    .h(px(32.0))
                    .bg(cx.theme().secondary.opacity(0.55))
                    .rounded(px(6.0))
                    .tooltip(provider.model.clone())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_center()
                            .text_size(px(12.0))
                            .truncate()
                            .font_family(cx.theme().mono_font_family.clone())
                            .py_1()
                            .rounded(px(4.0))
                            .child(if provider.model.is_empty() {
                                "—".to_owned()
                            } else {
                                provider.model.clone()
                            }),
                    )
                    .map(ProviderSelectTrigger::new)
                    .dropdown_menu(move |mut menu, _, _| {
                        for model in &model_choices {
                            let (owner, id, value) =
                                (model_owner.clone(), model_id.clone(), model.clone());
                            menu = menu.item(
                                PopupMenuItem::new(model.clone())
                                    .checked(model == &current_model)
                                    .on_click(move |_, _, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            this.update_provider_value(&id, "model", &value, cx)
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("provider-effort-{id}")))
                    .ghost()
                    .w(px(100.0))
                    .flex_shrink_0()
                    .h(px(32.0))
                    .bg(cx.theme().secondary.opacity(0.6))
                    .rounded(px(6.0))
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_1()
                            .items_center()
                            .child(
                                Icon::default()
                                    .path("icons/nebula-brain.svg")
                                    .size(px(16.0))
                                    .text_color(cx.theme().primary),
                            )
                            .child(div().text_size(px(12.0)).child(if effort.is_empty() {
                                "—".into()
                            } else {
                                effort.clone()
                            })),
                    )
                    .map(ProviderSelectTrigger::new)
                    .dropdown_menu(move |mut menu, _, _| {
                        for value in &efforts {
                            let (owner, id, value) =
                                (effort_owner.clone(), effort_id.clone(), value.clone());
                            menu = menu.item(
                                PopupMenuItem::new(if value.is_empty() {
                                    "—".into()
                                } else {
                                    value.clone()
                                })
                                .checked(value == effort)
                                .on_click(move |_, _, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.update_provider_value(&id, "effort", &value, cx)
                                    });
                                }),
                            );
                        }
                        menu
                    }),
            )
            .when(show_subscription, |row| {
                row.child(
                    div()
                        .w(px(144.0))
                        .flex_shrink_0()
                        .text_size(px(12.0))
                        .text_color(muted)
                        .child("—"),
                )
            })
            .child(
                div().w(px(56.0)).flex_shrink_0().flex().justify_center().child(
                    Button::new(SharedString::from(format!("provider-subagent-{id}")))
                        .ghost()
                        .size(px(32.0))
                        .icon(Icon::default().path("icons/nebula-waypoints.svg").size(px(16.0)))
                        .selected(selected_subagent.is_some())
                        .when(selected_subagent.is_some(), |button| {
                            button.child(
                                div()
                                    .absolute()
                                    .top(px(2.0))
                                    .right(px(2.0))
                                    .size(px(5.0))
                                    .rounded_full()
                                    .bg(cx.theme().primary),
                            )
                        })
                        .tooltip(selected_subagent.clone().unwrap_or_else(|| {
                            language.text(Message::ProvidersSubagent).to_owned()
                        }))
                        .dropdown_menu(move |mut menu, _, _| {
                            menu = menu.label(language.text(Message::ProvidersSameProvider));
                            for value in std::iter::once(String::new()).chain(models.clone()) {
                                let (owner, id) = (owner.clone(), sub_id.clone());
                                menu = menu.item(
                                    PopupMenuItem::new(if value.is_empty() {
                                        language.text(Message::ProvidersInherit).to_owned()
                                    } else {
                                        value.clone()
                                    })
                                    .checked(selected_subagent.as_deref().unwrap_or("") == value)
                                    .on_click(
                                        move |_, _, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                this.update_provider_value(
                                                    &id, "subagent", &value, cx,
                                                )
                                            });
                                        },
                                    ),
                                );
                            }
                            menu
                        }),
                ),
            )
            .child(
                h_flex()
                    .w(px(112.0))
                    .flex_shrink_0()
                    .gap_1()
                    .justify_end()
                    .when(
                        !active || self.provider_codex_confirm.as_deref() == Some(id.as_str()),
                        |row| {
                            row.child(
                                Button::new(SharedString::from(format!("provider-switch-{id}")))
                                    .disabled(self.provider_ui.write_task.is_some())
                                    .outline()
                                    .small()
                                    .h(px(30.0))
                                    .label(language.text(Message::ProvidersSwitch))
                                    .opacity(0.0)
                                    .group_hover(hover_group, |style| style.opacity(1.0))
                                    .focus(|style| style.opacity(1.0))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.switch_provider(&switch_id, cx)
                                    })),
                            )
                        },
                    )
                    .child(
                        Button::new(SharedString::from(format!("provider-edit-{id}")))
                            .ghost()
                            .w(px(56.0))
                            .flex_shrink_0()
                            .h(px(32.0))
                            .label(language.text(Message::ProvidersEditAction))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_provider(id.clone(), window, cx)
                            })),
                    ),
            )
    }

    fn update_provider_value(
        &mut self,
        id: &str,
        field: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        if self.provider_ui.write_task.is_some() {
            return;
        }
        let mut next = self.provider_store.clone();
        let Some(provider) = next.providers.iter_mut().find(|p| p.id == id) else { return };
        match field {
            "model" => {
                // 旧默认模型也是已配置模型，切换主模型不能让子 Agent 的引用失效。
                provider.models = crate::ai_providers::configured_models(provider);
                provider.model = value.to_owned();
            },
            "effort" => provider.reasoning_effort = value.to_owned(),
            "subagent" => {
                if !value.is_empty()
                    && !crate::ai_providers::configured_models(provider).iter().any(|m| m == value)
                {
                    return;
                }
                provider.subagent_model = (!value.is_empty()).then(|| value.to_owned());
            },
            _ => return,
        }
        if self.provider_store.cli_active.get(provider.agent_kind().slug()) == Some(&provider.id) {
            let provider = provider.clone();
            self.apply_provider_switch(next, provider, cx);
            return;
        }
        self.provider_status = Some(match crate::ai_providers::save(&next) {
            Ok(()) => {
                self.provider_store = next;
                ProviderStatus::Saved
            },
            Err(error) => ProviderStatus::Error(error.to_string()),
        });
        cx.notify();
    }
}
