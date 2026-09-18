use super::*;

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

/// A cached image is not proof that a QR is still valid or the connection is ready.
pub(super) fn qr_available(
    snapshot: Option<&connection::Snapshot>,
    expires: Option<u64>,
    now: u64,
) -> bool {
    snapshot.is_some_and(|snapshot| {
        matches!(snapshot.status, Status::Waiting | Status::Connected)
            && !snapshot.invitation.is_empty()
            && expires.is_none_or(|expiry| now < expiry)
    })
}

impl SettingsPane {
    pub(super) fn mobile_pairing_board(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = |message| language.text(message);
        let current = self.mobile.snapshot.as_ref();
        let busy = self.mobile.operation.is_some() || self.mobile.server_loading;
        let now = now();
        let valid_qr =
            qr_available(current, self.mobile.expires_at, now) && self.mobile.qr.is_some();
        let failed = self.mobile.failure.is_some()
            || current.is_some_and(|value| value.status == Status::Failed);
        let connected = current.is_some_and(|value| value.status == Status::Connected);
        let state = if busy {
            Message::MobileGenerating
        } else if failed {
            Message::MobileConnectionError
        } else if connected {
            Message::MobileConnected
        } else if current.is_some_and(|value| value.status == Status::Reconnecting) {
            Message::MobileReconnecting
        } else if valid_qr {
            Message::MobileWaiting
        } else if self.mobile.expires_at.is_some_and(|expiry| now >= expiry) {
            Message::MobileQrExpired
        } else if current.is_some_and(|value| value.invitation.is_empty()) {
            Message::MobileQrConsumed
        } else {
            Message::MobileQrIdle
        };
        let state_color = if failed {
            cx.theme().danger
        } else if connected || valid_qr {
            cx.theme().success
        } else {
            cx.theme().muted_foreground
        };
        let mut board = view::card(cx)
            .items_center()
            .gap_3()
            .child(
                div()
                    .id("mobile-qr-board")
                    .debug_selector(|| "mobile-qr-board".into())
                    .w_full()
                    .text_center()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(text(Message::MobileScanTitle)),
            )
            .child(
                div()
                    .w_full()
                    .text_center()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(Message::MobilePairHint)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(state_color.opacity(0.1))
                    .child(div().text_xs().text_color(state_color).child(text(state))),
            );
        let qr_box = div()
            .id("mobile-qr-surface")
            .debug_selector(|| "mobile-qr-surface".into())
            .w_full()
            .min_h(px(240.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .bg(if valid_qr { gpui::white() } else { cx.theme().background });
        board = if valid_qr {
            board.child(
                qr_box.child(
                    img(self.mobile.qr.clone().expect("validated QR"))
                        .size(px(self.mobile.qr_side))
                        .flex_shrink_0(),
                ),
            )
        } else {
            board.child(
                qr_box.child(
                    v_flex()
                        .items_center()
                        .gap_3()
                        .p_4()
                        .child(
                            Icon::default()
                                .path(crate::gpui_shell::assets::nav::PHONE)
                                .size(px(32.0))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_center()
                                .text_color(cx.theme().muted_foreground)
                                .child(text(if busy {
                                    Message::MobileGenerating
                                } else if self.mobile.mode == Mode::Relay
                                    && !self.mobile_relay_valid()
                                {
                                    Message::MobileChooseServer
                                } else {
                                    Message::MobileQrEmpty
                                })),
                        ),
                ),
            )
        };
        if let Some(address) =
            current.and_then(|value| value.address).filter(|_| self.mobile.mode == Mode::Lan)
        {
            board = board.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().background)
                    .child(address.to_string()),
            );
        }
        let input_allowed = current.map_or(self.mobile.allow_input, |value| value.allow_input);
        board = board.child(div().text_xs().text_color(cx.theme().muted_foreground).child(text(
            if input_allowed { Message::MobileInputGranted } else { Message::MobileViewOnly },
        )));
        if valid_qr {
            let validity = self
                .mobile
                .expires_at
                .map(|expiry| {
                    let seconds = expiry.saturating_sub(now);
                    format!(
                        "{} {:02}:{:02}",
                        text(Message::MobileQrRemaining),
                        seconds / 60,
                        seconds % 60
                    )
                })
                .unwrap_or_else(|| text(Message::MobileLanValidity).to_owned());
            board = board
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(validity));
        }
        let ready = if self.mobile.mode == Mode::Lan {
            !self.mobile.addresses.is_empty()
        } else {
            self.mobile_relay_valid()
        };
        board = board
            .child(
                Button::new("mobile-generate")
                    .w_full()
                    .primary()
                    .label(text(if busy {
                        Message::MobileGenerating
                    } else if current.is_some() {
                        Message::MobileRefreshQr
                    } else {
                        Message::MobileGenerate
                    }))
                    .disabled(busy || !ready)
                    .on_click(cx.listener(|this, _, window, cx| this.mobile_generate(window, cx))),
            )
            .when(busy || current.is_some(), |board| {
                board.child(
                    Button::new("mobile-stop")
                        .w_full()
                        .outline()
                        .label(text(Message::MobileStop))
                        .on_click(cx.listener(|this, _, _, cx| this.mobile_stop(cx))),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(Message::MobileQrPrivate)),
            );
        if let Some(failure) = self.mobile.failure {
            let message = match failure {
                Failure::Invalid => Message::MobileInvalid,
                Failure::Address => Message::MobileNoAddress,
                Failure::Port => Message::MobilePortBusy,
                Failure::Credentials => Message::MobileCredentialsError,
                Failure::Connection | Failure::Cancelled => Message::MobileConnectionError,
            };
            board = board
                .child(div().w_full().text_sm().text_color(cx.theme().danger).child(text(message)));
        }
        // A real security limitation remains visible, but without protocol jargon.
        if self.mobile.mode == Mode::Relay && self.mobile.relay_version == 1 {
            board = board.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().warning)
                    .child(text(Message::MobileLegacyWarning)),
            );
        }
        board
    }
}
