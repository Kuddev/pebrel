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
            || !(key.key.eq_ignore_ascii_case("r") || key.key == "enter")
            || mods.control
            || mods.alt
            || mods.platform
            || mods.function
        {
            return false;
        }
        if !event.is_held {
            if key.key == "enter" {
                cx.emit(TerminalViewEvent::RequestClose);
            } else {
                let destination =
                    self.ssh_destination.clone().expect("disconnected SSH destination");
                self.retry_ssh(&destination, cx);
            }
        }
        true
    }

    pub(super) fn ssh_disconnect_notice(&self, cx: &mut Context<Self>) -> AnyElement {
        let lang = crate::gpui_shell::config::ui_language(cx);
        let reason = match &self.ssh_stage {
            Some(SshStage::Failed(reason)) => reason.clone(),
            _ => String::new(),
        };
        let status = if self.ssh_retry_pending {
            Message::TerminalSshReconnecting
        } else {
            Message::TerminalSshSessionStopped
        };

        v_flex()
            .id("ssh-disconnected")
            .debug_selector(|| "ssh-disconnected".into())
            .absolute()
            .bottom(px(8.0))
            .left(px(12.0))
            .right(px(12.0))
            .py_2()
            .gap_1()
            .font_family(self.font.family.clone())
            .text_size(self.font_size)
            .bg(self.palette.background)
            .text_color(self.palette.foreground)
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.focus_handle.focus(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .min_w_0()
                    .w_full()
                    .debug_selector(|| "ssh-disconnected-status".into())
                    .child(lang.text(status)),
            )
            .when(!reason.is_empty(), |banner| {
                banner.child(div().truncate().text_color(self.palette.dim_foreground).child(reason))
            })
            .child(
                div()
                    .min_w_0()
                    .w_full()
                    .debug_selector(|| "ssh-disconnected-keys".into())
                    .child(lang.text(Message::TerminalSshSessionKeys)),
            )
            .into_any_element()
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
