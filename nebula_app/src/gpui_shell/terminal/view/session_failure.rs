//! Keep failed local launches diagnosable without changing their launch identity.

use super::*;
use crate::gpui_shell::prelude::*;
use crate::i18n::Message;

impl TerminalView {
    pub(super) fn local_startup_unconfirmed(&self) -> bool {
        self.ssh_destination.is_none()
            && self.prompt_input_epoch == 0
            && !self.session.as_ref().is_some_and(|session| {
                session.native_prompt.snapshot().seen || session.term.lock().nebula_prompt_active()
            })
    }

    pub(in crate::gpui_shell) fn local_session_failed(&self) -> bool {
        self.ssh_destination.is_none() && (self.error.is_some() || self.retain_failed_session)
    }

    pub(super) fn session_failure_banner(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let lang = crate::gpui_shell::config::ui_language(cx);
        let choose = crate::gpui_shell::widgets::settings_button(
            Button::new("failed-session-choose-shell")
                .debug_selector(|| "failed-session-choose-shell".into())
                .label(lang.text(Message::TerminalChooseShell))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(TerminalViewEvent::ChooseShell))),
            true,
            cx,
        );
        let close = crate::gpui_shell::widgets::settings_button(
            Button::new("failed-session-close")
                .debug_selector(|| "failed-session-close".into())
                .label(lang.text(Message::CommonClose))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(TerminalViewEvent::RequestClose))),
            false,
            cx,
        );
        self.recovery_banner("failed-session", cx)
            .children(self.exited.clone())
            .child(lang.text(Message::TerminalFailedSessionHelp))
            .child(h_flex().flex_wrap().gap_2().child(choose).child(close))
            .into_any_element()
    }

    pub(super) fn rule_failure_banner(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(blocked) = self.recovery.blocked_rule.as_ref() else {
            return div().into_any_element();
        };
        let lang = crate::gpui_shell::config::ui_language(cx);
        let retry = crate::gpui_shell::widgets::settings_button(
            Button::new("session-retry-without-rule")
                .debug_selector(|| "session-retry-without-rule".into())
                .label(lang.text(Message::SessionRetryWithoutRule))
                .on_click(cx.listener(|this, _, _, cx| this.retry_without_rule(cx))),
            true,
            cx,
        );
        let close = crate::gpui_shell::widgets::settings_button(
            Button::new("session-rule-dismiss")
                .debug_selector(|| "session-rule-dismiss".into())
                .label(lang.text(Message::CommonClose))
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_blocked_rule(cx))),
            false,
            cx,
        );
        self.recovery_banner("blocked-resume-rule", cx)
            .child(blocked.message(lang))
            .child(h_flex().flex_wrap().gap_2().child(retry).child(close))
            .into_any_element()
    }

    fn recovery_banner(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        v_flex()
            .id(id)
            .debug_selector(move || id.into())
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
    }
}
