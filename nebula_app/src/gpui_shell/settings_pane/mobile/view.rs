//! Task-first layout: configuration stays beside an independently visible QR board.
use super::*;

pub(super) fn card(cx: &App) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w_0()
        .gap_3()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
}

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn section_mobile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.mobile_initialize(window, cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        let wide = window.viewport_size().width >= px(1040.0);
        let header = h_flex()
            .w_full()
            .gap_4()
            .flex_wrap()
            .items_start()
            .justify_between()
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(20.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(text(Message::MobilePairTitle)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(text(Message::MobileOverview)),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(div().text_xs().child(text(Message::MobileShortcut)))
                    .child(
                        NebulaSwitch::new("mobile-sidebar-shortcut")
                            .checked(self.runtime.mobile_shortcut)
                            .on_click(cx.listener(|this, enabled: &bool, _, cx| {
                                this.mobile.shortcut_failure = this
                                    .try_persist(
                                        &[(
                                            "mobile_shortcut",
                                            if *enabled { "1" } else { "0" }.to_owned(),
                                        )],
                                        cx,
                                    )
                                    .is_err();
                                cx.notify();
                            })),
                    ),
            );
        let main = v_flex()
            .w_full()
            .min_w_0()
            .gap_5()
            .child(self.mobile_modes(cx))
            .child(self.mobile_devices(cx))
            .child(self.mobile_policy(cx))
            .child(self.mobile_advanced(cx));
        let board = self.mobile_pairing_board(cx);
        let body = if wide {
            h_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .gap_6()
                .items_start()
                .child(
                    div()
                        .id("mobile-config-scroll")
                        .debug_selector(|| "mobile-config-scroll".into())
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .pr_1()
                        .child(main),
                )
                .child(
                    div()
                        .id("mobile-pairing-scroll")
                        .w(px(300.0))
                        .flex_shrink_0()
                        .max_h_full()
                        .overflow_y_scroll()
                        .child(board),
                )
                .into_any_element()
        } else {
            // On narrow windows the actual task comes first, not a long form.
            div()
                .id("mobile-pairing-scroll")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .child(v_flex().w_full().gap_5().child(board).child(main))
                .into_any_element()
        };
        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .gap_5()
            .child(header)
            .when(self.mobile.shortcut_failure, |page| {
                page.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(text(Message::MobilePreferenceError)),
                )
            })
            .child(body)
    }

    fn mobile_modes(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        let busy = self.mobile.operation.is_some() || self.mobile.server_loading;
        let mut modes = card(cx).child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(text(Message::MobileConnectionMode)),
        );
        for (index, mode, title, hint, icon) in [
            (0_usize, Mode::Lan, Message::MobileLan, Message::MobileLanHint, IconName::Globe),
            (1, Mode::Relay, Message::MobileRelay, Message::MobileRelayHint, IconName::HardDrive),
        ] {
            let selected = self.mobile.mode == mode;
            let mut choice = v_flex()
                .w_full()
                .min_w_0()
                .rounded_md()
                .border_1()
                .border_color(if selected { cx.theme().primary } else { cx.theme().border })
                .bg(if selected { cx.theme().primary.opacity(0.08) } else { cx.theme().background })
                .child(
                    Button::new(("mobile-mode", index))
                        .w_full()
                        .h_auto()
                        .ghost()
                        .px_3()
                        .py_3()
                        .child(
                            v_flex()
                                .w_full()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_2()
                                        .child(Icon::new(icon).size_4())
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .child(text(title)),
                                        )
                                        .child(
                                            div()
                                                .size(px(14.0))
                                                .rounded_full()
                                                .border_1()
                                                .border_color(if selected {
                                                    cx.theme().primary
                                                } else {
                                                    cx.theme().muted_foreground
                                                })
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .when(selected, |radio| {
                                                    radio.child(
                                                        div()
                                                            .size(px(8.0))
                                                            .rounded_full()
                                                            .bg(cx.theme().primary),
                                                    )
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .pl_6()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(text(hint)),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.mobile.mode != mode {
                                this.mobile_invalidate(cx);
                                this.mobile.edit_sequence =
                                    this.mobile.edit_sequence.wrapping_add(1);
                                this.mobile.mode = mode;
                                this.mobile_generate_if_ready(window, cx);
                            }
                        })),
                );
            if selected {
                let mut configuration = v_flex()
                    .min_w_0()
                    .gap_2()
                    .mx_3()
                    .mb_3()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border);
                if mode == Mode::Lan {
                    configuration =
                        configuration
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text(Message::MobileAddress)),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .child(div().flex_1().min_w_0().child(
                                        Select::new(&self.mobile.address_select).disabled(busy),
                                    ))
                                    .child(
                                        Button::new("mobile-refresh-addresses")
                                            .outline()
                                            .icon(
                                                Icon::default()
                                                    .path(crate::gpui_shell::assets::nav::REFRESH),
                                            )
                                            .tooltip(text(Message::MobileRefresh))
                                            .disabled(busy || self.mobile.loading_addresses)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.mobile_refresh_addresses(window, cx)
                                            })),
                                    ),
                            )
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                                text(if self.mobile.addresses.is_empty() {
                                    Message::MobileNoAddress
                                } else {
                                    Message::MobileAddressHint
                                }),
                            ));
                } else {
                    configuration =
                        configuration
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text(Message::MobileSelectServer)),
                            )
                            .when(!self.mobile.server_hosts.is_empty(), |group| {
                                group.child(Select::new(&self.mobile.server_select).disabled(busy))
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .child(div().flex_1().min_w_0().child(
                                        Input::new(&self.mobile.server_input).disabled(busy),
                                    ))
                                    .child(
                                        Button::new("mobile-use-server")
                                            .outline()
                                            .label(text(if self.mobile.server_loading {
                                                Message::MobileReadingServer
                                            } else {
                                                Message::MobileUseServer
                                            }))
                                            .disabled(busy)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.mobile_use_server(window, cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text(Message::MobileSelectServerHint)),
                            )
                            .when(self.mobile.server_failure, |group| {
                                group.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().danger)
                                        .child(text(Message::MobileServerReadError)),
                                )
                            });
                }
                choice = choice.child(configuration);
            }
            modes = modes.child(choice);
        }
        modes
    }

    fn mobile_policy(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        card(cx)
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(text(Message::MobilePolicy)))
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .justify_between()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_sm().child(text(Message::MobileAllowInput)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text(Message::MobileReadOnlyHint)),
                            ),
                    )
                    .child(
                        NebulaSwitch::new("mobile-allow-input")
                            .checked(self.mobile.allow_input)
                            .on_click(cx.listener(|this, enabled: &bool, window, cx| {
                                this.mobile_invalidate(cx);
                                this.mobile.allow_input = *enabled;
                                this.mobile_generate_if_ready(window, cx);
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(div().text_sm().child(text(Message::MobileBackgroundTitle)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text(Message::MobileBackgroundHint)),
                    ),
            )
    }

    fn mobile_devices(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        let mut devices = card(cx).child(
            h_flex()
                .w_full()
                .justify_between()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(Icon::default().path(crate::gpui_shell::assets::nav::PHONE).size_4())
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(text(Message::MobileDevices)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.mobile.devices.len().to_string()),
                        ),
                )
                .child(
                    Button::new("mobile-refresh-devices")
                        .ghost()
                        .icon(Icon::default().path(crate::gpui_shell::assets::nav::REFRESH))
                        .tooltip(text(Message::MobileRefreshDevices))
                        .disabled(self.mobile.devices_loading || self.mobile.revoking)
                        .on_click(cx.listener(|this, _, _, cx| this.mobile_refresh_devices(cx))),
                ),
        );
        if self.mobile.devices_failure {
            devices = devices.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(text(Message::MobileDeviceError)),
            );
        }
        if self.mobile.devices.is_empty() {
            devices = devices.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(Message::MobileNoDevices)),
            );
        }
        for device in &self.mobile.devices {
            let id = device.id.clone();
            let confirm_id = id.clone();
            let selected = self.mobile.revoke_pending.as_ref() == Some(&id);
            let mut row = v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .p_3()
                .rounded_md()
                .bg(cx.theme().background)
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .items_center()
                        .child(crate::gpui_shell::widgets::device_icon_anchor(
                            Icon::default().path(crate::gpui_shell::assets::nav::PHONE),
                            cx,
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(div().text_sm().truncate().child(device.name.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(if device.connected {
                                            cx.theme().success
                                        } else {
                                            cx.theme().muted_foreground
                                        })
                                        .child(text(if device.connected {
                                            Message::MobileDeviceOnline
                                        } else {
                                            Message::MobileDeviceOffline
                                        })),
                                )
                                .child(
                                    div().text_xs().text_color(cx.theme().muted_foreground).child(
                                        text(if device.allow_input {
                                            Message::MobileInputGranted
                                        } else {
                                            Message::MobileViewOnly
                                        }),
                                    ),
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("mobile-revoke-{id}")))
                                .ghost()
                                .icon(Icon::default().path(crate::gpui_shell::assets::nav::TRASH))
                                .tooltip(text(Message::MobileRevoke))
                                .disabled(self.mobile.revoking)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mobile.revoke_pending = Some(id.clone());
                                    cx.notify();
                                })),
                        ),
                );
            if selected {
                row = row.child(div().text_xs().child(text(Message::MobileRevokeHint))).child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("mobile-confirm-revoke")
                                .danger()
                                .label(text(Message::MobileRevoke))
                                .disabled(self.mobile.revoking)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mobile_revoke_device(confirm_id.clone(), cx)
                                })),
                        )
                        .child(
                            Button::new("mobile-cancel-revoke")
                                .ghost()
                                .label(text(Message::CommonCancel))
                                .disabled(self.mobile.revoking)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.mobile.revoke_pending = None;
                                    cx.notify();
                                })),
                        ),
                );
            }
            devices = devices.child(row);
        }
        devices.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text(Message::MobileDevicesHint)),
        )
    }
}
