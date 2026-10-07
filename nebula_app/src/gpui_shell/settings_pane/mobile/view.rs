//! 三态共享同一份运行快照；二维码、在线状态和批准请求均来自实际连接。

use super::*;
use crate::gpui_shell::widgets::NebulaSwitch;
use gpui_component::menu::PopupMenuItem;

pub(super) fn description(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .text_size(px(13.0))
        .line_height(px(21.0))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub(super) fn group_heading(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    super::super::design::group_heading(text, 14.0, cx)
}

pub(super) fn failure_message(failure: Failure) -> Message {
    match failure {
        Failure::Invalid => Message::MobileInvalid,
        Failure::Address => Message::MobileAddressFailed,
        Failure::Port => Message::MobilePortFailed,
        Failure::Credentials => Message::MobileCredentialsFailed,
        Failure::Connection => Message::MobileConnectionFailed,
        Failure::Cancelled => Message::MobileCancelled,
    }
}

fn status_message(status: Status) -> Message {
    match status {
        Status::Starting => Message::MobileStarting,
        Status::Waiting => Message::MobileWaiting,
        Status::Connected => Message::MobileConnected,
        Status::Reconnecting => Message::MobileReconnecting,
        Status::Stopped => Message::MobileStopped,
        Status::Failed => Message::MobileConnectionFailed,
    }
}

fn row(
    id: &'static str,
    title: &'static str,
    hint: impl Into<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    // GPUI 需用稳定 ID 保存 hover 状态，否则颜色只能等后台状态轮询触发重绘。
    h_flex()
        .id(id)
        .debug_selector(move || id.into())
        .w_full()
        .gap(px(24.0))
        .px(px(13.0))
        .py(px(12.0))
        .rounded(px(8.0))
        .hover(|style| style.bg(crate::gpui_shell::theme::settings_hover_bg(cx, false)))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(3.0))
                .child(div().text_size(px(14.0)).child(title))
                .child(description(hint, cx)),
        )
        .child(h_flex().flex_shrink_0().gap_2().child(control))
}

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn section_mobile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.mobile_initialize(window, cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let phase = self.mobile.phase();
        // 原型的 812 包含两侧 56px 留白；窄窗收紧页边，不缩小二维码或按钮。
        let narrow = f32::from(window.viewport_size().width) < 1000.0;
        let mut page = v_flex()
            .id("mobile-settings")
            .debug_selector(|| "mobile-settings".into())
            .w_full()
            .max_w(px(812.0))
            .px(px(if narrow { 24.0 } else { 56.0 }))
            .pt(px(44.0))
            .pb(px(140.0))
            .text_size(px(14.0))
            .line_height(px(21.0))
            .child(
                v_flex()
                    .w_full()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(22.0))
                            .line_height(px(30.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::MobileTitle)),
                    )
                    .child(description(language.text(Message::MobileSubtitle), cx)),
            );
        if let Some(failure) = self.mobile.failure {
            page = page.child(
                div()
                    .id("mobile-error")
                    .mt(px(18.0))
                    .text_size(px(13.0))
                    .text_color(cx.theme().danger)
                    .child(language.text(failure_message(failure))),
            );
        }
        page = match phase {
            Phase::Off => page.child(self.mobile_off(cx)),
            Phase::Pairing => page.child(self.mobile_pairing(narrow, cx)),
            Phase::Paired => page,
        };
        if phase != Phase::Off {
            if let Some(snapshot) = &self.mobile.snapshot {
                for request in &snapshot.requests {
                    page = page.child(self.mobile_request(request, cx));
                }
            }
            page = page.child(self.mobile_devices(cx));
        }
        if phase == Phase::Paired {
            page = page.child(self.mobile_connections(cx)).child(self.mobile_permissions(cx));
        }
        div().w_full().flex().justify_center().child(page)
    }

    fn mobile_off(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let busy = self.mobile.operation || self.mobile.loading;
        v_flex()
            .id("mobile-off")
            .debug_selector(|| "mobile-off".into())
            .w_full()
            .mt(px(28.0))
            .p(px(24.0))
            .border_1()
            .border_color(cx.theme().border)
            .rounded(px(10.0))
            .gap_1()
            .child(
                div()
                    .text_size(px(18.0))
                    .line_height(px(26.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(language.text(Message::MobileOffTitle)),
            )
            .child(description(language.text(Message::MobileOffDescription), cx))
            .child(
                h_flex().mt(px(20.0)).child(
                    Button::new("mobile-enable")
                        .debug_selector(|| "mobile-enable".into())
                        .primary()
                        .label(language.text(if busy {
                            Message::MobileStarting
                        } else {
                            Message::MobileEnable
                        }))
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.mobile_enable(window, cx)),
                        ),
                ),
            )
    }

    fn mobile_pairing(&self, narrow: bool, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let busy = self.mobile.operation;
        let mode = self.mobile.mode;
        let mut modes = h_flex()
            .id("mobile-pairing-modes")
            .debug_selector(|| "mobile-pairing-modes".into())
            .mt(px(18.0))
            .p(px(2.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary);
        for (route, label) in [(Mode::Lan, Message::MobileLan), (Mode::Relay, Message::MobileRelay)]
        {
            modes = modes.child(
                Button::new(if route == Mode::Lan {
                    "mobile-mode-lan"
                } else {
                    "mobile-mode-relay"
                })
                .ghost()
                .small()
                .h(px(28.0))
                .rounded(px(6.0))
                .selected(mode == route)
                .when(mode == route, |button| {
                    button.bg(crate::gpui_shell::theme::settings_panel_bg(cx))
                })
                .label(language.text(label))
                .disabled(busy)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.mobile_mode(route, window, cx)),
                ),
            );
        }
        let mut steps = v_flex()
            .id("mobile-pairing-steps")
            .debug_selector(|| "mobile-pairing-steps".into())
            .w_full()
            .mt(px(18.0))
            .gap(px(10.0));
        let messages = match mode {
            Mode::Lan => {
                [Message::MobileStepOpen, Message::MobileStepScan, Message::MobileStepApprove]
            },
            Mode::Relay => {
                [Message::MobileStepRelay, Message::MobileStepScan, Message::MobileStepApprove]
            },
        };
        for (index, message) in messages.into_iter().enumerate() {
            let mut body = v_flex().flex_1().min_w_0().child(language.text(message));
            if index == 0 {
                body = body.child(description(
                    language.text(if mode == Mode::Lan {
                        Message::MobileLanHint
                    } else {
                        Message::MobileRelayOutbound
                    }),
                    cx,
                ));
            }
            steps = steps.child(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap(px(10.0))
                    .child(
                        div()
                            .size(px(20.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_full()
                            .text_size(px(11.5))
                            .child((index + 1).to_string()),
                    )
                    .child(body),
            );
        }
        let valid = self.mobile.valid_qr();
        let mut qr = v_flex().w(px(232.0)).flex_shrink_0().items_center().gap(px(10.0));
        let code = div()
            .id("mobile-qr")
            .debug_selector(|| "mobile-qr".into())
            .size(px(196.0))
            .p(px(12.0))
            .rounded(px(10.0))
            .bg(gpui::rgb(0xffffff))
            .flex()
            .items_center()
            .justify_center();
        qr = qr.child(if valid {
            code.child(img(self.mobile.qr.as_ref().expect("valid QR").clone()).size_full())
        } else {
            code.child(div().text_size(px(13.0)).text_color(gpui::rgb(0x333333)).child(
                language.text(if busy {
                    Message::MobileStarting
                } else {
                    Message::MobileQrUnavailable
                }),
            ))
        });
        let remaining = self.mobile.expires_at.unwrap_or(0).saturating_sub(now());
        let timer = format!("{:02}:{:02}", remaining / 60, remaining % 60);
        qr = qr.child(
            h_flex()
                .gap_1()
                .child(description(
                    if valid {
                        language.format(Message::MobileExpires, &[("time", &timer)])
                    } else {
                        language.text(Message::MobileExpired).to_owned()
                    },
                    cx,
                ))
                .child(
                    Button::new("mobile-refresh-qr")
                        .ghost()
                        .small()
                        .label(language.text(Message::MobileRefreshQr))
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.mobile_pair(window, cx))),
                ),
        );
        if valid && mode == Mode::Lan {
            if let Some(connection) = self.mobile.snapshot.as_ref().and_then(|s| s.connection(mode))
            {
                if let Some(code) =
                    connection.pairing_code.as_deref().filter(|code| code.len() == 8)
                {
                    qr = qr.child(
                        h_flex()
                            .gap_1()
                            .child(description(language.text(Message::MobileShortCodePrompt), cx))
                            .child(
                                div()
                                    .id("mobile-short-code")
                                    .debug_selector(|| "mobile-short-code".into())
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(format!("{} {}", &code[..4], &code[4..])),
                            ),
                    );
                    if !connection.discoverable {
                        qr = qr.child(description(
                            language.text(Message::MobileDiscoveryUnavailable),
                            cx,
                        ));
                    }
                } else {
                    qr = qr.child(description(language.text(Message::MobileShortCodeUsed), cx));
                }
            }
        }
        let copied = self.mobile.copy_feedback.read(cx).is_copied();
        qr = qr.child(
            Button::new("mobile-copy-invitation")
                .debug_selector(|| "mobile-copy-invitation".into())
                .ghost()
                .small()
                .h(px(32.0))
                .icon(if copied { IconName::Check } else { IconName::Copy })
                .label(language.text(if copied {
                    Message::MobileCopied
                } else {
                    Message::MobileManualPair
                }))
                .disabled(!valid)
                .on_click(cx.listener(|this, _, _, cx| this.mobile_copy(cx))),
        );
        let mut left = v_flex()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .text_size(px(18.0))
                    .line_height(px(26.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(language.text(Message::MobilePairTitle)),
            )
            .child(description(language.text(Message::MobilePairDescription), cx))
            .child(h_flex().child(modes))
            // 网卡决定当前邀请的地址，应和连接类型、扫码步骤放在同一配对卡片里。
            .when(mode == Mode::Lan, |left| {
                left.child(self.mobile_lan_options(true, cx).mt(px(14.0)))
            })
            .child(steps);
        if let Some(connection) = self.mobile.snapshot.as_ref().and_then(|s| s.connection(mode)) {
            left = left.child(
                description(
                    format!(
                        "{} · {}",
                        language.text(status_message(connection.status)),
                        connection.address
                    ),
                    cx,
                )
                .mt(px(14.0)),
            );
        } else if mode == Mode::Relay {
            left = left.child(
                Button::new("mobile-pair-configure")
                    .ghost()
                    .label(language.text(Message::MobileConfigureRelay))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.mobile_open_relay(window, cx)),
                    ),
            );
        }
        h_flex()
            .id("mobile-pairing")
            .debug_selector(|| "mobile-pairing".into())
            .w_full()
            .mt(px(28.0))
            .p(px(24.0))
            .gap(px(32.0))
            .items_start()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(px(10.0))
            .when(narrow, |hero| hero.flex_col())
            .child(left)
            .child(qr)
    }

    fn mobile_request(
        &self,
        request: &connection::PairingRequest,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let approve_id = request.id.clone();
        let deny_id = request.id.clone();
        let busy = self.mobile.operation || request.approving;
        let route = language.text(if request.route == Mode::Lan {
            Message::MobileLan
        } else {
            Message::MobileRelay
        });
        let detail = language.format(
            Message::MobileRequestDescription,
            &[
                ("route", route),
                ("peer", request.peer.as_deref().unwrap_or("")),
                ("code", &request.verification_code),
            ],
        );
        h_flex()
            .id(SharedString::from(format!("mobile-request-{}", request.id)))
            .debug_selector(|| "mobile-pairing-request".into())
            .w_full()
            .mt(px(16.0))
            .px(px(16.0))
            .py(px(14.0))
            .gap(px(14.0))
            .border_1()
            .border_color(cx.theme().border)
            .rounded(px(10.0))
            .flex_wrap()
            .child(Icon::default().path(crate::gpui_shell::assets::nav::PHONE).size(px(20.0)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(160.0))
                    .gap_1()
                    .child(div().text_size(px(14.0)).font_weight(gpui::FontWeight::SEMIBOLD).child(
                        language.format(Message::MobileRequestTitle, &[("name", &request.name)]),
                    ))
                    .child(description(detail, cx)),
            )
            .child(
                Button::new(SharedString::from(format!("mobile-deny-{}", request.id)))
                    .ghost()
                    .label(language.text(Message::MobileDeny))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let id = deny_id.clone();
                        this.mobile_run(
                            None,
                            false,
                            move || connection::decide_pairing(&id, None),
                            window,
                            cx,
                        );
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("mobile-approve-{}", request.id)))
                    .primary()
                    .label(language.text(if request.approving {
                        Message::MobileApproving
                    } else {
                        Message::MobileApprove
                    }))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let id = approve_id.clone();
                        let permission = this.mobile.preferences().default_input;
                        this.mobile_run(
                            None,
                            false,
                            move || connection::decide_pairing(&id, Some(permission)),
                            window,
                            cx,
                        );
                    })),
            )
    }

    fn mobile_permission_menu(
        &self,
        device: Option<&connection::DeviceSummary>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let selected =
            device.map(|d| d.allow_input).unwrap_or(self.mobile.preferences().default_input);
        let device_id = device.map(|d| d.id.clone());
        let owner = cx.entity().downgrade();
        Button::new(SharedString::from(format!(
            "mobile-permission-{}",
            device_id.as_deref().unwrap_or("default")
        )))
        .outline()
        .small()
        .w(px(132.0))
        .h(px(32.0))
        .justify_between()
        .label(language.text(if selected {
            Message::MobileAllowInput
        } else {
            Message::MobileReadOnly
        }))
        .dropdown_caret(true)
        .disabled(self.mobile.operation)
        .dropdown_menu(move |mut menu, _, _| {
            for (value, label) in
                [(false, Message::MobileReadOnly), (true, Message::MobileAllowInput)]
            {
                let owner = owner.clone();
                let device_id = device_id.clone();
                menu = menu.item(
                    PopupMenuItem::new(language.text(label)).checked(value == selected).on_click(
                        move |_, window, cx| {
                            let id = device_id.clone();
                            let _ = owner.update(cx, |this, cx| {
                                if let Some(id) = id {
                                    this.mobile_run(
                                        None,
                                        false,
                                        move || connection::set_permission(&id, value),
                                        window,
                                        cx,
                                    );
                                } else {
                                    let mut preferences = this.mobile.preferences();
                                    preferences.default_input = value;
                                    this.mobile_apply(preferences, None, None, false, window, cx);
                                }
                            });
                        },
                    ),
                );
            }
            menu
        })
    }

    fn mobile_devices(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let mut group = v_flex().id("mobile-devices").w_full().mt(px(36.0)).child(
            h_flex()
                .w_full()
                .justify_between()
                .pb(px(6.0))
                .child(group_heading(language.text(Message::MobileDevices), cx))
                .when(self.mobile.phase() == Phase::Paired, |head| {
                    head.child(
                        Button::new("mobile-add-phone")
                            .debug_selector(|| "mobile-add-phone".into())
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .label(language.text(Message::MobileAddPhone))
                            .disabled(self.mobile.operation)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.mobile_pair(window, cx)),
                            ),
                    )
                }),
        );
        let devices =
            self.mobile.snapshot.as_ref().map(|s| s.devices.as_slice()).unwrap_or_default();
        if devices.is_empty() {
            return group.child(
                description(language.text(Message::MobileNoDevices), cx).px(px(13.0)).py(px(12.0)),
            );
        }
        for device in devices {
            let id = device.id.clone();
            let name = device.name.clone();
            let status = language.text(if device.connected {
                Message::MobileOnline
            } else {
                Message::MobileOffline
            });
            let detail = if let Some(route) = device.route.filter(|_| device.connected) {
                format!(
                    "{status} · {}",
                    language.text(if route == Mode::Lan {
                        Message::MobileLan
                    } else {
                        Message::MobileRelay
                    })
                )
            } else {
                status.to_owned()
            };
            group = group.child(
                h_flex()
                    .id(SharedString::from(format!("mobile-device-{id}")))
                    .debug_selector(|| "mobile-device-row".into())
                    .w_full()
                    .px(px(13.0))
                    .py(px(12.0))
                    .gap(px(14.0))
                    .rounded(px(8.0))
                    .hover(|style| style.bg(crate::gpui_shell::theme::settings_hover_bg(cx, false)))
                    .child(
                        Icon::default()
                            .path(crate::gpui_shell::assets::nav::PHONE)
                            .size(px(20.0))
                            .flex_shrink_0(),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(name.clone()),
                            )
                            .child(description(detail, cx)),
                    )
                    .child(self.mobile_permission_menu(Some(device), cx))
                    .child(
                        Button::new(SharedString::from(format!("mobile-remove-{id}")))
                            .ghost()
                            .small()
                            .label(language.text(Message::MobileRemove))
                            .disabled(self.mobile.operation)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.mobile_remove(id.clone(), name.clone(), window, cx)
                            })),
                    ),
            );
        }
        group
    }

    fn mobile_lan_options(&self, compact: bool, cx: &Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let busy = self.mobile.operation;
        v_flex()
            .w_full()
            .when(compact, |fields| fields.gap(px(12.0)))
            .child(
                row(
                    "mobile-interface-row",
                    language.text(Message::MobileInterface),
                    language.text(Message::MobileInterfaceHint),
                    h_flex()
                        .gap_2()
                        .child(
                            gpui::Styled::h(
                                Select::new(&self.mobile.address_select).small(),
                                px(32.0),
                            )
                            .w(px(204.0))
                            .disabled(busy),
                        )
                        .child(
                            Button::new("mobile-refresh-addresses")
                                .ghost()
                                .small()
                                .icon(IconName::Redo)
                                .tooltip(language.text(Message::MobileRefreshAddresses))
                                .disabled(busy || self.mobile.loading)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.mobile_refresh_addresses(window, cx)
                                })),
                        ),
                    cx,
                )
                .when(compact, |field| {
                    field.flex_col().items_stretch().gap(px(6.0)).px(px(0.0)).py(px(0.0))
                }),
            )
            .child(
                row(
                    "mobile-port-row",
                    language.text(Message::MobilePort),
                    language.text(Message::MobilePortHint),
                    gpui::Styled::h(Input::new(&self.mobile.port_input).small(), px(32.0))
                        .w(px(120.0))
                        .disabled(busy),
                    cx,
                )
                .when(compact, |field| {
                    field.flex_col().items_stretch().gap(px(6.0)).px(px(0.0)).py(px(0.0))
                }),
            )
    }

    fn mobile_connections(&self, cx: &Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let preferences = self.mobile.preferences();
        let busy = self.mobile.operation;
        let mut group = v_flex()
            .w_full()
            .mt(px(36.0))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_3()
                    .pb(px(6.0))
                    .child(group_heading(language.text(Message::MobileConnections), cx))
                    .child(description(language.text(Message::MobileConnectionsHint), cx)),
            )
            .child(row(
                "mobile-lan-row",
                language.text(Message::MobileLanDirect),
                language.text(Message::MobileLanHint),
                NebulaSwitch::new("mobile-lan-enabled")
                    .checked(preferences.lan_enabled)
                    .disabled(busy)
                    .on_click(cx.listener(|this, enabled: &bool, window, cx| {
                        let mut preferences = this.mobile.preferences();
                        preferences.lan_enabled = *enabled;
                        this.mobile_apply(preferences, None, Some(Mode::Lan), false, window, cx);
                    })),
                cx,
            ));
        if preferences.lan_enabled {
            group = group.child(self.mobile_lan_options(false, cx));
        }
        let relay = self.mobile.snapshot.as_ref().and_then(|s| s.relay.as_ref());
        let hint = relay
            .map(|relay| {
                format!("{} · {}", language.text(status_message(relay.status)), relay.address)
            })
            .unwrap_or_else(|| language.text(Message::MobileRelayHint).to_owned());
        group.child(row(
            "mobile-relay-row",
            language.text(Message::MobileRelay),
            hint,
            Button::new("mobile-configure-relay")
                .outline()
                .small()
                .label(language.text(if preferences.relay_enabled {
                    Message::MobileManageRelay
                } else {
                    Message::MobileConfigureRelay
                }))
                .disabled(busy)
                .on_click(cx.listener(|this, _, window, cx| this.mobile_open_relay(window, cx))),
            cx,
        ))
    }

    fn mobile_permissions(&self, cx: &Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        v_flex()
            .w_full()
            .mt(px(36.0))
            .child(group_heading(language.text(Message::MobilePermissions), cx).pb(px(6.0)))
            .child(row(
                "mobile-default-permission-row",
                language.text(Message::MobileDefaultPermission),
                language.text(Message::MobileDefaultPermissionHint),
                self.mobile_permission_menu(None, cx),
                cx,
            ))
            .child(row(
                "mobile-notifications-row",
                language.text(Message::MobileNotifications),
                language.text(Message::MobileNotificationsHint),
                NebulaSwitch::new("mobile-notifications")
                    .checked(self.mobile.preferences().notifications)
                    .disabled(self.mobile.operation)
                    .on_click(cx.listener(|this, enabled: &bool, window, cx| {
                        let mut preferences = this.mobile.preferences();
                        preferences.notifications = *enabled;
                        this.mobile_apply(preferences, None, None, false, window, cx);
                    })),
                cx,
            ))
            .child(row(
                "mobile-pause-row",
                language.text(Message::MobilePauseTitle),
                language.text(Message::MobilePauseHint),
                Button::new("mobile-pause")
                    .debug_selector(|| "mobile-pause".into())
                    .outline()
                    .small()
                    .label(language.text(Message::MobilePause))
                    .disabled(self.mobile.operation)
                    .on_click(cx.listener(|this, _, window, cx| this.mobile_pause(window, cx))),
                cx,
            ))
    }
}
