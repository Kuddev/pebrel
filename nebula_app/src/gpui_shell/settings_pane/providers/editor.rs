use super::*;
use crate::i18n::Message;
use gpui::{Animation, AnimationExt as _};
use gpui_component::FocusTrapElement as _;
use gpui_component::menu::PopupMenuItem;

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn close_provider_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.provider_ui.editing.is_none()
            || self.provider_ui.closing
            || self.provider_ui.write_task.is_some()
        {
            return;
        }
        if cx.reduce_motion() {
            self.finish_provider_close(window, cx);
            return;
        }
        self.provider_ui.closing = true;
        self.provider_ui.sequence = self.provider_ui.sequence.wrapping_add(1);
        self.provider_ui.task = None;
        self.provider_ui.close_generation = self.provider_ui.close_generation.wrapping_add(1);
        let generation = self.provider_ui.close_generation;
        let timer = cx.background_executor().timer(Duration::from_millis(180));
        self.provider_ui.close_task = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.provider_ui.closing && this.provider_ui.close_generation == generation {
                    this.finish_provider_close(window, cx);
                }
            });
        }));
        cx.notify();
    }

    fn finish_provider_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.provider_ui.auth_task = None;
        self.provider_ui.loading_auth = false;
        self.provider_ui.closing = false;
        self.provider_ui.auth.update(cx, |input, cx| input.set_value("", window, cx));
        self.provider_ui.editing = None;
        self.provider_ui.api_key.update(cx, |input, cx| input.set_value("", window, cx));
        self.provider_ui.sequence = self.provider_ui.sequence.wrapping_add(1);
        self.provider_ui.task = None;
        self.provider_ui.fetching = false;
        self.provider_ui.model_picker_open = false;
        self.provider_test_seq = self.provider_test_seq.wrapping_add(1);
        self.provider_test_running = false;
        self.provider_codex_confirm = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn fetch_provider_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_ui.fetching
            || self.provider_ui.write_task.is_some()
            || self.provider_ui.loading_auth
        {
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
        let key = match crate::ai_providers::parse_auth_draft(
            self.provider_ui.auth.read(cx).value().as_ref(),
            &provider.api_key_hint,
        ) {
            Ok(key) => key,
            Err(error) => {
                self.provider_status = Some(ProviderStatus::Error(error));
                cx.notify();
                return;
            },
        };
        let id = provider.id.clone();
        self.provider_ui.sequence = self.provider_ui.sequence.wrapping_add(1);
        let sequence = self.provider_ui.sequence;
        self.provider_ui.fetching = true;
        let task = cx
            .background_executor()
            .spawn(async move { crate::ai_providers::fetch_models(&provider, key) });
        self.provider_ui.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.provider_ui.sequence != sequence
                    || this.provider_ui.editing.as_ref() != Some(&id)
                {
                    return;
                }
                this.provider_ui.fetching = false;
                match result {
                    Ok(models) => {
                        // 拉取只更新草稿，用户点保存后才替换持久化目录。
                        this.provider_ui
                            .models
                            .update(cx, |input, cx| input.set_value(models.join("\n"), window, cx));
                        this.provider_status = Some(ProviderStatus::ModelsFetched(models.len()));
                        this.provider_ui.model_picker_open = true;
                    },
                    Err(error) => this.provider_status = Some(ProviderStatus::Error(error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(in crate::gpui_shell::settings_pane) fn provider_drawer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if self.active_section != 3 || self.provider_ui.editing.is_none() {
            return None;
        }
        let index = self.active_provider_index()?;
        let provider = self.provider_store.providers[index].clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let busy = self.provider_ui.write_task.is_some() || self.provider_ui.loading_auth;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border.opacity(0.6);
        let surface = theme.secondary.opacity(0.4);
        let viewport = window.viewport_size();
        let width = (f32::from(viewport.width) - 24.0).min(580.0);
        let field = |label: SharedString, input: &Entity<InputState>| {
            v_flex()
                .w_full()
                .flex_none()
                .min_w_0()
                .gap(px(6.0))
                .child(div().text_size(px(12.0)).text_color(muted).child(label))
                .child(
                    gpui::Styled::h(Input::new(input).disabled(busy), px(38.0))
                        .text_size(px(14.0))
                        .rounded(px(8.0))
                        .bg(surface)
                        .border_1()
                        .border_color(border),
                )
        };
        let model_menu = self.provider_ui.model_picker_open.then(|| {
            let models = crate::ai_providers::parse_model_list(
                self.provider_ui.models.read(cx).value().as_ref(),
            );
            let mut menu = v_flex().w_full().gap_1().p_1();
            for (index, model) in models.into_iter().enumerate() {
                menu = menu.child(
                    Button::new(("provider-fetched-model", index))
                        .ghost()
                        .w_full()
                        .h(px(30.0))
                        .child(div().w_full().text_left().text_size(px(12.0)).child(model.clone()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.provider_inputs[4]
                                .update(cx, |input, cx| input.set_value(model.clone(), window, cx));
                            this.provider_ui.model_picker_open = false;
                            this.sync_provider_config_fields(4, window, cx);
                            cx.notify();
                        })),
                );
            }
            div()
                .absolute()
                .top(px(66.0))
                .left_0()
                .w_full()
                .max_h(px(210.0))
                .rounded(px(8.0))
                .bg(cx.theme().popover)
                .border_1()
                .border_color(border)
                .shadow_md()
                .occlude()
                .overflow_y_scrollbar()
                .child(menu)
        });
        let basic = v_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(muted)
                    .child(language.text(Message::ProvidersBasic)),
            )
            .child(field(language.text(Message::ProvidersName).into(), &self.provider_inputs[0]))
            .child(field("Base URL".into(), &self.provider_inputs[3]))
            .child(
                v_flex()
                    .w_full()
                    .gap(px(6.0))
                    .child(div().text_size(px(12.0)).text_color(muted).child("API Key"))
                    .child(
                        gpui::Styled::h(
                            Input::new(&self.provider_ui.api_key).disabled(busy),
                            px(38.0),
                        )
                        .mask_toggle()
                        .text_size(px(14.0))
                        .rounded(px(8.0))
                        .bg(surface)
                        .border_1()
                        .border_color(border),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .relative()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(muted)
                            .child(language.text(Message::ProvidersDefaultModel)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                div().flex_1().min_w_0().child(
                                    gpui::Styled::h(
                                        Input::new(&self.provider_inputs[4]).disabled(busy),
                                        px(38.0),
                                    )
                                    .text_size(px(14.0))
                                    .rounded(px(8.0))
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border),
                                ),
                            )
                            .child(
                                Button::new("provider-fetch-models")
                                    .outline()
                                    .w(px(38.0))
                                    .h(px(38.0))
                                    .icon(
                                        Icon::default()
                                            .path("icons/nebula-download.svg")
                                            .size(px(16.0)),
                                    )
                                    .tooltip(language.text(Message::ProvidersFetch))
                                    .loading(self.provider_ui.fetching)
                                    .disabled(self.provider_ui.fetching || busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.fetch_provider_models(window, cx)
                                    })),
                            ),
                    )
                    .when_some(model_menu, |column, menu| column.child(menu)),
            );
        let owner = cx.entity().downgrade();
        let kind = self.provider_ui.draft_kind.unwrap_or(provider.kind);
        let default_protocol = match kind {
            ProviderKind::Anthropic => "Claude Messages",
            ProviderKind::Google => "Gemini API",
            _ => "Responses",
        };
        let protocol = self.provider_ui.draft_wire_api.unwrap_or(if provider.wire_api.is_empty() {
            default_protocol
        } else {
            &provider.wire_api
        });
        let advanced = v_flex()
            .gap_3()
            .child(
                Button::new("provider-advanced")
                    .text()
                    .group("provider-advanced-hover")
                    .h(px(24.0))
                    .px_0()
                    .w_full()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                Icon::new(if self.provider_ui.advanced_open {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(14.0)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(muted)
                                    .group_hover("provider-advanced-hover", |text| {
                                        text.text_color(cx.theme().foreground)
                                    })
                                    .child(language.text(Message::ProvidersAdvanced)),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.provider_ui.advanced_open = !this.provider_ui.advanced_open;
                        cx.notify();
                    })),
            )
            .when(self.provider_ui.advanced_open, |section| {
                section.child(
                    v_flex()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(muted)
                                .child(language.text(Message::ProvidersUpstreamFormat)),
                        )
                        .child(
                            Button::new("provider-protocol")
                                .disabled(busy)
                                .outline()
                                .w_full()
                                .h(px(38.0))
                                .bg(surface)
                                .rounded(px(8.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_left()
                                        .text_size(px(14.0))
                                        .child(protocol.to_owned()),
                                )
                                .map(ProviderSelectTrigger::new)
                                .dropdown_menu(move |mut menu, _, _| {
                                    for (label, selected_kind) in [
                                        ("Responses", ProviderKind::Custom),
                                        ("Chat Completions", ProviderKind::OpenAi),
                                        ("Claude Messages", ProviderKind::Anthropic),
                                        ("Gemini API", ProviderKind::Google),
                                    ] {
                                        let owner = owner.clone();
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _, cx| {
                                                let _ = owner.update(cx, |this, cx| {
                                                    this.provider_ui.draft_kind =
                                                        Some(selected_kind);
                                                    this.provider_ui.draft_wire_api = Some(label);
                                                    this.provider_codex_confirm = None;
                                                    this.provider_ui.sequence =
                                                        this.provider_ui.sequence.wrapping_add(1);
                                                    this.provider_ui.task = None;
                                                    this.provider_ui.fetching = false;
                                                    this.provider_test_seq =
                                                        this.provider_test_seq.wrapping_add(1);
                                                    this.provider_test_running = false;
                                                    cx.notify();
                                                });
                                            },
                                        ));
                                    }
                                    menu
                                }),
                        ),
                )
            });
        let files = v_flex()
            .gap_4()
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(muted)
                    .child(language.text(Message::ProvidersFiles)),
            )
            .child(
                v_flex()
                    .gap(px(6.0))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_size(px(12.0)).child("auth.json (JSON) *"))
                            .child(
                                Button::new("provider-format-auth")
                                    .disabled(busy)
                                    .ghost()
                                    .h(px(24.0))
                                    .label(language.text(Message::ProvidersFormat))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let text =
                                            this.provider_ui.auth.read(cx).value().to_string();
                                        match serde_json::from_str::<serde_json::Value>(&text)
                                            .and_then(|value| serde_json::to_string_pretty(&value))
                                        {
                                            Ok(text) => {
                                                this.provider_ui.auth.update(cx, |input, cx| {
                                                    input.set_value(text, window, cx)
                                                })
                                            },
                                            Err(error) => {
                                                this.provider_status = Some(ProviderStatus::Error(
                                                    format!("auth.json: {error}"),
                                                ))
                                            },
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        Input::new(&self.provider_ui.auth)
                            .disabled(busy)
                            .h(px(104.0))
                            .text_size(px(12.0))
                            .rounded(px(8.0))
                            .bg(surface)
                            .border_1()
                            .border_color(border),
                    ),
            )
            .child(
                v_flex()
                    .gap(px(6.0))
                    .child(div().text_size(px(12.0)).child("config.toml (TOML)"))
                    .child(
                        Input::new(&self.provider_ui.config)
                            .disabled(busy)
                            .h(px(220.0))
                            .text_size(px(12.0))
                            .rounded(px(8.0))
                            .bg(surface)
                            .border_1()
                            .border_color(border),
                    ),
            );
        let drawer = v_flex()
            .id("provider-editor")
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .bg(theme.background)
            .border_l_1()
            .border_color(border)
            .track_focus(&self.provider_ui.focus)
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.close_provider_editor(window, cx);
                }
            }))
            .child(
                h_flex()
                    .px_6()
                    .py_5()
                    .justify_between()
                    .flex_shrink_0()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .child(language.text(Message::ProvidersEdit)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(muted)
                                    .child(language.text(Message::ProvidersEditorHint)),
                            ),
                    )
                    .child(
                        Button::new("provider-close")
                            .disabled(busy)
                            .ghost()
                            .size(px(32.0))
                            .icon(IconName::Close)
                            .tooltip(language.text(Message::ProvidersClose))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_provider_editor(window, cx)
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_y_scrollbar().child(
                v_flex().w_full().px_6().pb_6().gap_8().child(basic).child(advanced).child(files),
            ))
            .when_some(self.provider_status.clone(), |drawer, status| {
                drawer.child(
                    div()
                        .px_6()
                        .pb_3()
                        .text_size(px(12.0))
                        .text_color(if status.is_error() { cx.theme().danger } else { muted })
                        .child(status.text(language)),
                )
            })
            .child(
                h_flex()
                    .p_4()
                    .border_t_1()
                    .border_color(border)
                    .justify_between()
                    .flex_shrink_0()
                    .child(
                        Button::new("provider-delete")
                            .disabled(busy)
                            .ghost()
                            .text_color(theme.danger)
                            .label(language.text(Message::ProvidersDeleteNode))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.delete_provider(window, cx)),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                Button::new("provider-cancel")
                                    .disabled(busy)
                                    .ghost()
                                    .label(language.text(Message::ProvidersCancel))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_provider_editor(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("provider-save")
                                    .disabled(busy)
                                    .loading(self.provider_ui.write_task.is_some())
                                    .primary()
                                    .label(language.text(Message::ProvidersSave))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_provider_metadata(window, cx);
                                    })),
                            ),
                    ),
            )
            .focus_trap("provider-drawer-trap", &self.provider_ui.focus);
        let closing = self.provider_ui.closing;
        let generation = self.provider_ui.close_generation;
        let drawer = if closing {
            div()
                .relative()
                .w(px(width))
                .h_full()
                .child(drawer)
                .with_animation(
                    ("provider-exit-slide", generation),
                    Animation::new(Duration::from_millis(180))
                        .with_easing(crate::gpui_shell::motion::ease),
                    move |drawer, t| drawer.right(px(-width * t)),
                )
                .into_any_element()
        } else {
            drawer.into_any_element()
        };
        let overlay = deferred(
            anchored().anchor(gpui::Anchor::TopLeft).position(gpui::point(px(0.0), px(0.0))).child(
                div()
                    .id("provider-overlay")
                    .w(viewport.width)
                    .h(viewport.height)
                    .flex()
                    .justify_end()
                    .occlude()
                    .bg(gpui::black().opacity(0.12))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_provider_editor(window, cx);
                    }))
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(drawer),
            ),
        )
        .with_priority(8);
        Some(if closing {
            div()
                .child(overlay)
                .with_animation(
                    ("provider-exit-fade", generation),
                    Animation::new(Duration::from_millis(180)),
                    |overlay, t| overlay.opacity(1.0 - t),
                )
                .into_any_element()
        } else {
            overlay.into_any_element()
        })
    }
}
