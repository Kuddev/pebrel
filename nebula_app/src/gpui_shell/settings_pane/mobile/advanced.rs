//! Infrequent diagnostics and manual credentials stay out of the pairing task.
use super::*;

impl SettingsPane {
    pub(super) fn mobile_advanced(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        let busy = self.mobile.operation.is_some() || self.mobile.server_loading;
        let mut card = view::card(cx).child(
            Button::new("mobile-advanced")
                .debug_selector(|| "mobile-advanced".into())
                .ghost()
                .w_full()
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
        if !self.mobile.advanced {
            return card;
        }
        if self.mobile.mode == Mode::Relay {
            card = card
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(text(
                    if self.mobile.relay_version == 2 {
                        Message::MobileRelaySecure
                    } else {
                        Message::MobileRelaySecurity
                    },
                )))
                .child(
                    Button::new("mobile-import-file")
                        .outline()
                        .label(text(Message::MobileImportFile))
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.mobile_import_file(window, cx)),
                        ),
                )
                .child(
                    Button::new("mobile-import-relay")
                        .outline()
                        .label(text(Message::MobileImport))
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(data) =
                                cx.read_from_clipboard().and_then(|item| item.text())
                            {
                                this.mobile_invalidate(cx);
                                this.mobile.edit_sequence =
                                    this.mobile.edit_sequence.wrapping_add(1);
                                this.mobile_import(&data, window, cx);
                                this.mobile_generate_if_ready(window, cx);
                            } else {
                                this.mobile.failure = Some(Failure::Invalid);
                                cx.notify();
                            }
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text(Message::MobileRelaySetupHint)),
                );
        }
        let fields: &[(usize, Message)] = match (self.mobile.mode, self.mobile.relay_version) {
            (Mode::Lan, _) => &[(4, Message::MobilePort)],
            (Mode::Relay, 2) => &[
                (0, Message::MobileServer),
                (1, Message::MobileDeviceId),
                (2, Message::MobileDesktopToken),
                (3, Message::MobilePhoneToken),
                (5, Message::MobileTlsPin),
            ],
            _ => &[
                (0, Message::MobileServer),
                (1, Message::MobileDeviceId),
                (2, Message::MobileDesktopToken),
                (3, Message::MobilePhoneToken),
            ],
        };
        for (index, label) in fields {
            card = card.child(
                v_flex()
                    .id(("mobile-advanced-field", *index))
                    .debug_selector(|| format!("mobile-advanced-field-{index}"))
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div().text_xs().text_color(cx.theme().muted_foreground).child(text(*label)),
                    )
                    .child(Input::new(&self.mobile.inputs[*index]).disabled(busy)),
            );
        }
        let can_copy = pairing_board::qr_available(
            self.mobile.snapshot.as_ref(),
            self.mobile.expires_at,
            pairing_board::now(),
        );
        card.child(
            Button::new("mobile-copy-invitation")
                .outline()
                .disabled(!can_copy)
                .label(text(if self.mobile.copy_feedback.read(cx).is_copied() {
                    Message::MobileCopied
                } else {
                    Message::MobileCopy
                }))
                .on_click(cx.listener(|this, _, _, cx| {
                    if pairing_board::qr_available(
                        this.mobile.snapshot.as_ref(),
                        this.mobile.expires_at,
                        pairing_board::now(),
                    ) {
                        if let Some(snapshot) = &this.mobile.snapshot {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                snapshot.invitation.clone(),
                            ));
                            this.mobile
                                .copy_feedback
                                .update(cx, |feedback, cx| feedback.mark_copied(cx));
                        }
                    }
                })),
        )
    }
}
