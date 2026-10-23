//! Disconnected-session actions share the existing workspace retry lifecycle.

use gpui::{AnyElement, prelude::FluentBuilder as _};

use super::*;
use crate::gpui_shell::prelude::*;
use crate::i18n::Message;
use crate::ssh_session::SshStage;

impl TerminalView {
    pub(super) fn ssh_failed(&self) -> bool {
        matches!(self.ssh_stage, Some(SshStage::Failed(_)))
    }

    pub(super) fn ssh_disconnected(&self) -> bool {
        self.ssh_destination.is_some() && self.ssh_connect.is_none() && self.ssh_failed()
    }

    pub(in crate::gpui_shell::terminal) fn retry_ssh(
        &mut self,
        destination: &str,
        cx: &mut Context<Self>,
    ) {
        if self.ssh_retry_pending
            || !self.ssh_failed()
            || self.ssh_destination.as_deref() != Some(destination)
        {
            return;
        }
        // A held key or a second click must not create another connection.
        // The workspace replaces this entity, isolating all late session events.
        self.ssh_retry_pending = true;
        cx.emit(TerminalViewEvent::RetrySsh(destination.to_owned()));
        cx.notify();
    }

    pub(super) fn handle_ssh_reconnect_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = &event.keystroke;
        let mods = &key.modifiers;
        if !self.ssh_disconnected()
            || !key.key.eq_ignore_ascii_case("r")
            || mods.control
            || mods.alt
            || mods.platform
            || mods.function
        {
            return false;
        }
        if !event.is_held {
            let destination = self.ssh_destination.clone().expect("disconnected SSH destination");
            self.retry_ssh(&destination, cx);
        }
        true
    }

    pub(super) fn ssh_disconnect_banner(&self, cx: &mut Context<Self>) -> AnyElement {
        let lang = crate::gpui_shell::config::ui_language(cx);
        let destination = self.ssh_destination.clone().expect("disconnected SSH destination");
        let reason = match &self.ssh_stage {
            Some(SshStage::Failed(reason)) => reason.clone(),
            _ => String::new(),
        };
        let retry = crate::gpui_shell::widgets::settings_button(
            Button::new("ssh-disconnected-retry")
                .debug_selector(|| "ssh-disconnected-retry".into())
                .label(lang.text(Message::TerminalSshReconnect))
                .disabled(self.ssh_retry_pending)
                .on_click(cx.listener(move |this, _, _, cx| this.retry_ssh(&destination, cx))),
            true,
            cx,
        );
        let close = crate::gpui_shell::widgets::settings_button(
            Button::new("ssh-disconnected-close")
                .debug_selector(|| "ssh-disconnected-close".into())
                .label(lang.text(Message::CommonClose))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(TerminalViewEvent::RequestClose))),
            false,
            cx,
        );

        v_flex()
            .id("ssh-disconnected")
            .debug_selector(|| "ssh-disconnected".into())
            .absolute()
            .bottom(px(8.0))
            .left(px(12.0))
            .right(px(12.0))
            .p_2()
            .gap_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.focus_handle.focus(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(lang.text(Message::TerminalSshDisconnected))
            .when(!reason.is_empty(), |banner| {
                banner.child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(reason),
                )
            })
            .child(h_flex().flex_wrap().gap_2().child(retry).child(close))
            .into_any_element()
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
