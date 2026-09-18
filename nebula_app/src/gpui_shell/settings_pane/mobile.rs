//! Native mobile pairing card. Network/persistence work belongs to
//! mobile_connection; this view owns form, QR visibility and operation feedback.

use super::*;
use crate::gpui_shell::{copy_feedback::CopyFeedback, widgets::NebulaSwitch};
use crate::{
    i18n::Message,
    mobile_connection::{self as connection, Failure, Mode, Status},
};
use pebrel_mobile_link::{endpoint::RelayAccess, preview::RelaySettings, qr::PairingQr};

pub(super) struct MobileState {
    mode: Mode,
    addresses: Vec<connection::LanAddress>,
    address_select: SharedSelect,
    inputs: Vec<Entity<InputState>>,
    form_values: Vec<String>,
    relay_version: u32,
    edit_sequence: u64,
    allow_input: bool,
    advanced: bool,
    initialized: bool,
    loading_addresses: bool,
    operation: Option<u64>,
    failure: Option<Failure>,
    snapshot: Option<connection::Snapshot>,
    qr: Option<Arc<RenderImage>>,
    qr_side: f32,
    copy_feedback: Entity<CopyFeedback>,
    monitor: Option<Task<()>>,
}

impl MobileState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SettingsPane>) -> Self {
        let address_select =
            cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let inputs = ["wss://", "", "", "", "0", ""]
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                cx.new(|cx| {
                    InputState::new(window, cx).default_value(value).masked(matches!(index, 2 | 3))
                })
            })
            .collect();
        Self {
            mode: Mode::Lan,
            addresses: Vec::new(),
            address_select,
            inputs,
            form_values: ["wss://", "", "", "", "0", ""].into_iter().map(str::to_owned).collect(),
            relay_version: 2,
            edit_sequence: 0,
            allow_input: false,
            advanced: false,
            initialized: false,
            loading_addresses: false,
            operation: None,
            failure: None,
            snapshot: None,
            qr: None,
            qr_side: 240.0,
            copy_feedback: cx.new(|_| CopyFeedback::new()),
            monitor: None,
        }
    }

    fn display_snapshot(&mut self, snapshot: Option<connection::Snapshot>) {
        if let Some(snapshot) = &snapshot {
            self.mode = snapshot.mode;
            self.allow_input = snapshot.allow_input;
        }
        let changed = self.snapshot.as_ref().map(|s| &s.invitation)
            != snapshot.as_ref().map(|s| &s.invitation);
        if changed {
            self.qr = snapshot.as_ref().and_then(|snapshot| {
                if snapshot.invitation.is_empty() {
                    return None;
                }
                let qr = PairingQr::encode(snapshot.invitation.as_bytes()).ok()?;
                let scale = (280 / qr.width()).clamp(2, 4);
                let side = (qr.width() * scale) as u32;
                self.qr_side = side as f32;
                let pixels = image::RgbaImage::from_raw(side, side, qr.rgba(scale).ok()?)?;
                Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
            });
        }
        self.snapshot = snapshot;
    }
}

impl Drop for MobileState {
    fn drop(&mut self) {
        if let Some(generation) = self.operation {
            connection::cancel(generation);
        }
    }
}

impl SettingsPane {
    fn mobile_invalidate(&mut self, cx: &mut Context<Self>) {
        if self.mobile.snapshot.is_some() || self.mobile.operation.is_some() {
            connection::stop();
        }
        self.mobile.operation = None;
        self.mobile.display_snapshot(None);
        self.mobile.failure = None;
        self.mobile.copy_feedback.update(cx, |state, cx| state.clear(cx));
        cx.notify();
    }

    fn mobile_initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.initialized {
            return;
        }
        self.mobile.initialized = true;
        if let Some(snapshot) = connection::snapshot() {
            self.mobile.mode = snapshot.mode;
            self.mobile.display_snapshot(Some(snapshot));
        }
        let select = self.mobile.address_select.clone();
        self._subscriptions.push(cx.subscribe_in(&select, window, |this, _, event, _, cx| {
            if matches!(event, SelectEvent::Confirm(_)) {
                this.mobile_invalidate(cx);
            }
        }));
        for input in self.mobile.inputs.clone() {
            self._subscriptions.push(cx.subscribe_in(&input, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let values: Vec<_> = this
                        .mobile
                        .inputs
                        .iter()
                        .map(|input| input.read(cx).value().to_string())
                        .collect();
                    if values != this.mobile.form_values {
                        this.mobile.form_values = values;
                        this.mobile.edit_sequence = this.mobile.edit_sequence.wrapping_add(1);
                        this.mobile_invalidate(cx);
                    }
                }
            }));
        }
        let feedback = self.mobile.copy_feedback.clone();
        self._subscriptions.push(cx.observe(&feedback, |_, _, cx| cx.notify()));
        self.mobile_refresh_addresses(window, cx);
        let load = cx.background_executor().spawn(async { connection::saved_relay() });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Some(saved)) = load.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.mobile.edit_sequence == 0 {
                        this.mobile_import(&saved, window, cx);
                    }
                });
            }
        })
        .detach();
        self.mobile.monitor = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this
                    .update(cx, |this, cx| {
                        let snapshot = connection::snapshot();
                        if snapshot != this.mobile.snapshot {
                            this.mobile.display_snapshot(snapshot);
                            if this.active_section == 10 {
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn mobile_refresh_addresses(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.loading_addresses {
            return;
        }
        self.mobile.loading_addresses = true;
        let previous = self
            .mobile
            .address_select
            .read(cx)
            .selected_index(cx)
            .and_then(|index| self.mobile.addresses.get(index.row))
            .map(|entry| entry.address)
            .or_else(|| self.mobile.snapshot.as_ref().and_then(|snapshot| snapshot.address));
        let task = cx.background_executor().spawn(async { connection::addresses() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.mobile.loading_addresses = false;
                match result {
                    Ok(addresses) => {
                        let selected = previous
                            .and_then(|ip| addresses.iter().position(|entry| entry.address == ip))
                            .or_else(|| (!addresses.is_empty()).then_some(0));
                        let items = addresses
                            .iter()
                            .map(|entry| {
                                SharedString::from(format!("{} ({})", entry.address, entry.name))
                            })
                            .collect();
                        this.mobile.addresses = addresses;
                        this.mobile.address_select.update(cx, |state, cx| {
                            state.set_items(items, window, cx);
                            state.set_selected_index(
                                selected.map(|row| IndexPath::default().row(row)),
                                window,
                                cx,
                            );
                        });
                        if previous.is_some()
                            && !this
                                .mobile
                                .addresses
                                .iter()
                                .any(|entry| Some(entry.address) == previous)
                        {
                            this.mobile_invalidate(cx);
                        }
                    },
                    Err(_) => this.mobile.failure = Some(Failure::Address),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mobile_import(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let version = serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| v["version"].as_u64());
        if version == Some(2) {
            match RelayAccess::parse(text.as_bytes()) {
                Ok(access) => {
                    self.mobile.relay_version = 2;
                    for (index, value) in [
                        (0, &access.url),
                        (1, &access.room),
                        (2, &access.desktop_token),
                        (3, &access.mobile_token),
                        (5, &access.tls_pin),
                    ] {
                        self.mobile.form_values[index] = value.clone();
                        self.mobile.inputs[index]
                            .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                    }
                    self.mobile.failure = None;
                },
                Err(_) => {
                    self.mobile.failure = Some(Failure::Invalid);
                    self.mobile.advanced = true;
                },
            }
            cx.notify();
            return;
        }
        match RelaySettings::parse(text.as_bytes()) {
            Ok(settings) => {
                self.mobile.relay_version = 1;
                for (index, (input, value)) in self
                    .mobile
                    .inputs
                    .iter()
                    .zip([
                        settings.url,
                        settings.device,
                        settings.desktop_token,
                        settings.mobile_token,
                    ])
                    .enumerate()
                {
                    self.mobile.form_values[index] = value.clone();
                    input.update(cx, |input, cx| input.set_value(value, window, cx));
                }
                self.mobile.failure = None;
            },
            Err(_) => {
                self.mobile.failure = Some(Failure::Invalid);
                self.mobile.advanced = true;
            },
        }
        cx.notify();
    }

    fn mobile_generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.operation.is_some() {
            return;
        }
        // A late credential load must not overwrite a form already submitted
        // by the user, including an invalid submission that needs correction.
        self.mobile.edit_sequence = self.mobile.edit_sequence.wrapping_add(1);
        let mode = self.mobile.mode;
        let address = self
            .mobile
            .address_select
            .read(cx)
            .selected_index(cx)
            .and_then(|index| self.mobile.addresses.get(index.row))
            .map(|entry| entry.address);
        let Ok(port) = self.mobile.inputs[4].read(cx).value().trim().parse::<u16>() else {
            self.mobile.failure = Some(Failure::Invalid);
            self.mobile.advanced = true;
            cx.notify();
            return;
        };
        let values: Vec<_> = self
            .mobile
            .inputs
            .iter()
            .take(4)
            .map(|input| input.read(cx).value().trim().to_owned())
            .collect();
        let relay = if self.mobile.relay_version == 2 {
            serde_json::json!({"version":2,"url":values[0],"room":values[1],
                "desktopToken":values[2],"mobileToken":values[3],"tlsPin":self.mobile.inputs[5].read(cx).value().trim()}).to_string()
        } else {
            serde_json::json!({"version":1,"url":values[0],"device":values[1],
                "desktopToken":values[2],"mobileToken":values[3],"name":"Pebrel PC"})
            .to_string()
        };
        let valid = if self.mobile.relay_version == 2 {
            RelayAccess::parse(relay.as_bytes()).is_ok()
        } else {
            RelaySettings::parse(relay.as_bytes()).is_ok()
        };
        if mode == Mode::Relay && !valid {
            self.mobile.failure = Some(Failure::Invalid);
            self.mobile.advanced = true;
            cx.notify();
            return;
        }
        self.mobile_invalidate(cx);
        let generation = connection::begin();
        self.mobile.operation = Some(generation);
        let allow_input = self.mobile.allow_input;
        let task = cx.background_executor().spawn(async move {
            connection::start(generation, mode, address, port, allow_input, relay)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.mobile.operation != Some(generation) {
                    return;
                }
                this.mobile.operation = None;
                match result {
                    Ok(snapshot) => this.mobile.display_snapshot(Some(snapshot)),
                    Err(Failure::Cancelled) => {},
                    Err(error) => this.mobile.failure = Some(error),
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
        let sequence = self.mobile.edit_sequence;
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
                if sequence != this.mobile.edit_sequence || this.mobile.operation.is_some() {
                    return;
                }
                this.mobile_invalidate(cx);
                this.mobile.edit_sequence = this.mobile.edit_sequence.wrapping_add(1);
                match read {
                    Ok(data) => this.mobile_import(&data, window, cx),
                    Err(_) => {
                        this.mobile.failure = Some(Failure::Invalid);
                        cx.notify();
                    },
                }
            });
        })
        .detach();
    }

    pub(super) fn section_mobile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.mobile_initialize(window, cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let selected_bg = cx.theme().list_active;
        let busy = self.mobile.operation.is_some();
        let mut choices =
            v_flex().w_full().rounded_md().border_1().border_color(border).overflow_hidden();
        for (index, mode, title, hint) in [
            (0, Mode::Lan, Message::MobileLan, Message::MobileLanHint),
            (1, Mode::Relay, Message::MobileRelay, Message::MobileRelayHint),
        ] {
            let selected = self.mobile.mode == mode;
            choices = choices.child(
                Button::new(("mobile-mode", index as usize))
                    .w_full()
                    .h_auto()
                    .py_3()
                    .px_3()
                    .tooltip(text(title))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_start()
                            .child(div().child(if selected { "●" } else { "○" }))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .gap_1()
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(text(title)),
                                    )
                                    .child(div().text_sm().text_color(muted).child(text(hint))),
                            ),
                    )
                    .when(selected, |button| button.bg(selected_bg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.mobile.mode != mode {
                            this.mobile_invalidate(cx);
                            this.mobile.edit_sequence = this.mobile.edit_sequence.wrapping_add(1);
                            this.mobile.mode = mode;
                            cx.notify();
                        }
                    })),
            );
        }
        let mut card = v_flex()
            .w_full()
            .p_5()
            .gap_4()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .child(
                div().font_weight(gpui::FontWeight::SEMIBOLD).child(text(Message::MobilePairTitle)),
            )
            .child(div().text_sm().text_color(muted).child(text(Message::MobilePairHint)))
            .child(choices);
        if self.mobile.mode == Mode::Lan {
            card = card
                .child(div().text_sm().child(text(Message::MobileAddress)))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(
                            div()
                                .w(px(340.0))
                                .max_w_full()
                                .child(Select::new(&self.mobile.address_select).disabled(busy)),
                        )
                        .child(
                            Button::new("mobile-refresh-addresses")
                                .label(text(Message::MobileRefresh))
                                .disabled(busy || self.mobile.loading_addresses)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.mobile_refresh_addresses(window, cx)
                                })),
                        ),
                )
                .child(div().text_sm().text_color(muted).child(text(
                    if self.mobile.addresses.is_empty() {
                        Message::MobileNoAddress
                    } else {
                        Message::MobileAddressHint
                    },
                )));
        } else {
            card = card
                .child(
                    Button::new("mobile-import-file")
                        .label(text(Message::MobileImportFile))
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.mobile_import_file(window, cx)),
                        ),
                )
                .child(
                    Button::new("mobile-import-relay")
                        .label(text(Message::MobileImport))
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            let data = cx.read_from_clipboard().and_then(|item| item.text());
                            if let Some(data) = data {
                                this.mobile_invalidate(cx);
                                this.mobile.edit_sequence =
                                    this.mobile.edit_sequence.wrapping_add(1);
                                this.mobile_import(&data, window, cx);
                            } else {
                                this.mobile.failure = Some(Failure::Invalid);
                                cx.notify();
                            }
                        })),
                )
                .child(div().text_sm().text_color(muted).child(text(Message::MobileRelaySetupHint)))
                .child(div().text_sm().text_color(cx.theme().warning).child(text(
                    if self.mobile.relay_version == 2 {
                        Message::MobileRelaySecure
                    } else {
                        Message::MobileRelaySecurity
                    },
                )));
        }
        card = card
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .child(v_flex().gap_1().child(text(Message::MobileAllowInput)).child(
                        div().text_sm().text_color(muted).child(text(Message::MobileReadOnlyHint)),
                    ))
                    .child(
                        NebulaSwitch::new("mobile-allow-input")
                            .checked(self.mobile.allow_input)
                            .on_click(cx.listener(|this, enabled: &bool, _, cx| {
                                this.mobile_invalidate(cx);
                                this.mobile.allow_input = *enabled;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                Button::new("mobile-advanced")
                    .label(text(if self.mobile.advanced {
                        Message::MobileAdvancedClose
                    } else {
                        Message::MobileAdvanced
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mobile.advanced = !this.mobile.advanced;
                        cx.notify();
                    })),
            );
        if self.mobile.advanced {
            let fields: &[(usize, Message)] = if self.mobile.mode == Mode::Lan {
                &[(4, Message::MobilePort)]
            } else if self.mobile.relay_version == 2 {
                &[
                    (0, Message::MobileServer),
                    (1, Message::MobileDeviceId),
                    (2, Message::MobileDesktopToken),
                    (3, Message::MobilePhoneToken),
                    (5, Message::MobileTlsPin),
                ]
            } else {
                &[
                    (0, Message::MobileServer),
                    (1, Message::MobileDeviceId),
                    (2, Message::MobileDesktopToken),
                    (3, Message::MobilePhoneToken),
                ]
            };
            let mut advanced = v_flex().gap_3().p_3().border_1().border_color(border).rounded_md();
            for (index, label) in fields {
                advanced = advanced.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().child(text(*label)))
                        .child(Input::new(&self.mobile.inputs[*index]).disabled(busy)),
                );
            }
            card = card.child(advanced);
        }
        card = card.child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(
                    NebulaButton::new("mobile-generate")
                        .primary()
                        .label(text(if busy {
                            Message::MobileGenerating
                        } else {
                            Message::MobileGenerate
                        }))
                        .disabled(
                            busy || (self.mobile.mode == Mode::Lan
                                && self.mobile.addresses.is_empty()),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.mobile_generate(window, cx)),
                        ),
                )
                .when(busy || self.mobile.snapshot.is_some(), |row| {
                    row.child(
                        Button::new("mobile-stop")
                            .label(text(Message::MobileStop))
                            .on_click(cx.listener(|this, _, _, cx| this.mobile_invalidate(cx))),
                    )
                }),
        );
        if let Some(failure) = self.mobile.failure {
            let message = match failure {
                Failure::Invalid => Message::MobileInvalid,
                Failure::Address => Message::MobileNoAddress,
                Failure::Port => Message::MobilePortBusy,
                Failure::Credentials => Message::MobileCredentialsError,
                Failure::Connection | Failure::Cancelled => Message::MobileConnectionError,
            };
            card = card.child(div().text_sm().text_color(cx.theme().danger).child(text(message)));
        }
        if let Some(snapshot) = &self.mobile.snapshot {
            let status = match snapshot.status {
                Status::Starting => Message::MobileGenerating,
                Status::Waiting => Message::MobileWaiting,
                Status::Connected => Message::MobileConnected,
                Status::Reconnecting => Message::MobileReconnecting,
                Status::Failed => Message::MobileConnectionError,
                Status::Stopped => Message::MobileStopped,
            };
            let copied = self.mobile.copy_feedback.read(cx).is_copied();
            card = card
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(text(status)))
                .when(snapshot.invitation.is_empty(), |card| {
                    card.child(
                        div().text_sm().text_color(muted).child(text(Message::MobileQrConsumed)),
                    )
                })
                .when_some(self.mobile.qr.clone(), |card, qr| {
                    card.child(img(qr).size(px(self.mobile.qr_side)).flex_shrink_0())
                })
                .child(div().text_sm().text_color(muted).child(text(Message::MobileQrPrivate)))
                .child(
                    Button::new("mobile-copy-invitation")
                        .disabled(snapshot.invitation.is_empty())
                        .label(text(if copied {
                            Message::MobileCopied
                        } else {
                            Message::MobileCopy
                        }))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(snapshot) = &this.mobile.snapshot {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    snapshot.invitation.clone(),
                                ));
                                this.mobile
                                    .copy_feedback
                                    .update(cx, |feedback, cx| feedback.mark_copied(cx));
                            }
                        })),
                );
        }
        v_flex()
            .w_full()
            .gap_4()
            .child(div().text_sm().text_color(muted).child(text(Message::MobileOverview)))
            .child(card)
    }
}
