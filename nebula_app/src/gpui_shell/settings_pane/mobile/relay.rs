//! 中转配置抽屉：草稿与运行连接分离，校验通过后才保存和启用。

use super::*;

impl SettingsPane {
    pub(super) fn mobile_sync_servers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hosts: Vec<_> = self
            .ssh_hosts
            .merged()
            .into_iter()
            .map(|destination| {
                let label = self
                    .ssh_hosts
                    .profiles
                    .for_destination(&destination)
                    .label
                    .filter(|label| !label.is_empty())
                    .unwrap_or_else(|| destination.clone());
                (destination, label)
            })
            .collect();
        let labels = hosts
            .iter()
            .map(|(destination, label)| {
                SharedString::from(if label == destination {
                    destination.clone()
                } else {
                    format!("{label} · {destination}")
                })
            })
            .collect();
        let selected = (!hosts.is_empty()).then_some(IndexPath::default().row(0));
        self.mobile.server_hosts = hosts;
        self.mobile.server_select.update(cx, |select, cx| {
            select.set_items(labels, window, cx);
            select.set_selected_index(selected, window, cx);
        });
    }

    pub(super) fn mobile_open_relay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mobile_sync_servers(window, cx);
        self.mobile.relay_open = true;
        self.mobile.relay_result = None;
        self.mobile.relay_edit = self.mobile.relay_edit.wrapping_add(1);
        if let Some(saved) = self.mobile.saved_relay.clone() {
            self.mobile_import_relay(&saved, window, cx);
        }
        window.focus(&self.mobile.relay_inputs[0].read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    fn mobile_close_relay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mobile.relay_open = false;
        self.mobile.relay_loading = false;
        self.mobile.relay_edit = self.mobile.relay_edit.wrapping_add(1);
        if let Some(generation) = self.mobile.generation.take() {
            connection::cancel(generation);
            self.mobile.sequence = self.mobile.sequence.wrapping_add(1);
            self.mobile.operation = false;
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(super) fn mobile_relay_json(&self, cx: &App) -> Result<String, Failure> {
        let values: Vec<_> =
            self.mobile.relay_inputs.iter().map(|i| i.read(cx).value().trim().to_owned()).collect();
        let json = serde_json::json!({"version":2,"url":values[0],"room":values[1],"desktopToken":values[2],"mobileToken":values[3],"tlsPin":values[4]}).to_string();
        RelayAccess::parse(json.as_bytes()).map_err(|_| Failure::Invalid)?;
        Ok(json)
    }

    fn mobile_import_relay(&mut self, json: &str, window: &mut Window, cx: &mut Context<Self>) {
        match RelayAccess::parse(json.as_bytes()) {
            Ok(access) => {
                self.mobile.syncing = true;
                for (input, value) in self.mobile.relay_inputs.iter().zip([
                    &access.url,
                    &access.room,
                    &access.desktop_token,
                    &access.mobile_token,
                    &access.tls_pin,
                ]) {
                    input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                }
                self.mobile.syncing = false;
                self.mobile.relay_edit = self.mobile.relay_edit.wrapping_add(1);
                self.mobile.relay_result = Some(Message::MobileRelayImported);
                self.mobile.failure = None;
            },
            Err(_) => self.mobile.relay_result = Some(Message::MobileRelayInvalid),
        }
        cx.notify();
    }

    fn mobile_read_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let destination = self
            .mobile
            .server_select
            .read(cx)
            .selected_index(cx)
            .and_then(|i| self.mobile.server_hosts.get(i.row))
            .map(|h| h.0.clone());
        let Some(destination) = destination else { return };
        self.mobile.relay_loading = true;
        self.mobile.relay_result = None;
        let sequence = self.mobile.relay_edit;
        let task =
            cx.background_executor().spawn(async move { connection::relay_from_ssh(&destination) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.mobile.relay_loading = false;
                if !this.mobile.relay_open || this.mobile.relay_edit != sequence {
                    return;
                }
                match result {
                    Ok(json) => this.mobile_import_relay(&json, window, cx),
                    Err(_) => this.mobile.relay_result = Some(Message::MobileRelayReadFailed),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mobile_import_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                crate::gpui_shell::config::ui_language(cx).text(Message::MobileImportFile).into(),
            ),
        });
        let sequence = self.mobile.relay_edit;
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let read = cx
                .background_executor()
                .spawn(async move {
                    use std::io::Read;
                    let mut data = String::new();
                    std::fs::File::open(path)?.take(8193).read_to_string(&mut data)?;
                    if data.len() > 8192 {
                        return Err(std::io::Error::other("configuration_too_large"));
                    }
                    Ok::<_, std::io::Error>(data)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.mobile.relay_open || this.mobile.relay_edit != sequence {
                    return;
                }
                match read {
                    Ok(json) => this.mobile_import_relay(&json, window, cx),
                    Err(_) => this.mobile.relay_result = Some(Message::MobileRelayInvalid),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn mobile_save_relay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let json = match self.mobile_relay_json(cx) {
            Ok(json) => json,
            Err(_) => {
                self.mobile.relay_result = Some(Message::MobileRelayInvalid);
                cx.notify();
                return;
            },
        };
        let mut preferences = self.mobile.preferences();
        preferences.enabled = true;
        preferences.relay_enabled = true;
        self.mobile.mode = Mode::Relay;
        self.mobile_apply(preferences, Some(json), Some(Mode::Relay), true, window, cx);
    }

    pub(in crate::gpui_shell::settings_pane) fn mobile_relay_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if self.active_section != MOBILE_SECTION || !self.mobile.relay_open {
            return None;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        let busy = self.mobile.operation || self.mobile.relay_loading;
        let mut fields = v_flex().w_full().gap(px(18.0));
        for (index, label) in [
            Message::MobileRelayUrl,
            Message::MobileRelayRoom,
            Message::MobileDesktopToken,
            Message::MobilePhoneToken,
            Message::MobileTlsPin,
        ]
        .into_iter()
        .enumerate()
        {
            let mut field = v_flex()
                .w_full()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(text(label)),
                )
                .child(Input::new(&self.mobile.relay_inputs[index]).w_full().disabled(busy));
            if index == 1 || index == 3 || index == 4 {
                field = field.child(view::description(
                    text(match index {
                        1 => Message::MobileRelayRoomHint,
                        3 => Message::MobileRelayTokensHint,
                        _ => Message::MobileRelayPinHint,
                    }),
                    cx,
                ));
            }
            fields = fields.child(field);
        }
        let drawer = v_flex()
            .id("mobile-relay-drawer")
            .debug_selector(|| "mobile-relay-drawer".into())
            .track_focus(&self.mobile.relay_focus)
            .w(px(520.0))
            .max_w_full()
            .h_full()
            .bg(crate::gpui_shell::theme::settings_panel_bg(cx))
            .border_l_1()
            .border_color(cx.theme().border)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_3()
                    .px(px(28.0))
                    .py(px(24.0))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(text(Message::MobileRelay)),
                            )
                            .child(view::description(text(Message::MobileRelayDrawerSubtitle), cx)),
                    )
                    .child(
                        Button::new("mobile-relay-close")
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip(text(Message::MobileClose))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.mobile_close_relay(window, cx)
                            })),
                    ),
            )
            .child(
                v_flex().flex_1().min_h_0().overflow_y_scrollbar().child(
                    v_flex()
                        .w_full()
                        .px(px(28.0))
                        .py(px(24.0))
                        .gap(px(28.0))
                        .child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .child(view::group_heading(text(Message::MobileReadServer), cx))
                                .child(view::description(text(Message::MobileReadServerHint), cx))
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_2()
                                        .child(
                                            Select::new(&self.mobile.server_select)
                                                .flex_1()
                                                .min_w_0()
                                                .disabled(busy),
                                        )
                                        .child(
                                            Button::new("mobile-relay-read")
                                                .outline()
                                                .label(text(if self.mobile.relay_loading {
                                                    Message::MobileReading
                                                } else {
                                                    Message::MobileRead
                                                }))
                                                .disabled(
                                                    busy || self.mobile.server_hosts.is_empty(),
                                                )
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.mobile_read_server(window, cx)
                                                })),
                                        ),
                                ),
                        )
                        .child(
                            v_flex()
                                .w_full()
                                .gap_4()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .flex_wrap()
                                        .gap_2()
                                        .child(view::group_heading(
                                            text(Message::MobileConnectionInfo),
                                            cx,
                                        ))
                                        .child(div().flex_1())
                                        .child(
                                            Button::new("mobile-relay-paste")
                                                .ghost()
                                                .small()
                                                .label(text(Message::MobileImportClipboard))
                                                .disabled(busy)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    if let Some(data) = cx
                                                        .read_from_clipboard()
                                                        .and_then(|item| item.text())
                                                    {
                                                        this.mobile_import_relay(&data, window, cx);
                                                    } else {
                                                        this.mobile.relay_result =
                                                            Some(Message::MobileRelayInvalid);
                                                        cx.notify();
                                                    }
                                                })),
                                        )
                                        .child(
                                            Button::new("mobile-relay-file")
                                                .ghost()
                                                .small()
                                                .label(text(Message::MobileImportFile))
                                                .disabled(busy)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.mobile_import_file(window, cx)
                                                })),
                                        ),
                                )
                                .child(fields),
                        )
                        .when_some(self.mobile.relay_result, |body, result| {
                            body.child(view::description(text(result), cx))
                        })
                        .when_some(self.mobile.failure, |body, failure| {
                            body.child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(cx.theme().danger)
                                    .child(text(view::failure_message(failure))),
                            )
                        })
                        .child(view::description(text(Message::MobileEncryptedHint), cx)),
                ),
            )
            .child(
                h_flex()
                    .w_full()
                    .px(px(28.0))
                    .py(px(18.0))
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("mobile-relay-disable")
                            .ghost()
                            .label(text(Message::MobileDisableRelay))
                            .disabled(busy || !self.mobile.preferences().relay_enabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                let mut preferences = this.mobile.preferences();
                                preferences.relay_enabled = false;
                                this.mobile_apply(
                                    preferences,
                                    None,
                                    Some(Mode::Relay),
                                    true,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("mobile-relay-cancel")
                            .ghost()
                            .label(text(Message::CommonCancel))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.mobile_close_relay(window, cx)
                            })),
                    )
                    .child(
                        Button::new("mobile-relay-save")
                            .primary()
                            .label(text(if self.mobile.operation {
                                Message::MobileChecking
                            } else {
                                Message::MobileCheckSave
                            }))
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.mobile_save_relay(window, cx)
                            })),
                    ),
            );
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_end()
                .bg(cx.theme().background.opacity(0.5))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.mobile_close_relay(window, cx);
                    }),
                )
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key.eq_ignore_ascii_case("escape") {
                        cx.stop_propagation();
                        this.mobile_close_relay(window, cx);
                    }
                }))
                .child(drawer)
                .into_any_element(),
        )
    }
}
