//! A terminal-free session chooser; workspace startup owns the launch transitions.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window,
    div, px,
};

use crate::gpui_shell::prelude::*;
use crate::i18n::Message;

#[derive(Clone, Copy)]
pub(super) enum WelcomeEvent {
    Local,
    Ssh,
}

pub(super) struct WelcomeView {
    focus: FocusHandle,
}

impl WelcomeView {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self { focus: cx.focus_handle().tab_stop(false) }
    }
}

impl EventEmitter<WelcomeEvent> for WelcomeView {}

impl Focusable for WelcomeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for WelcomeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        div()
            .id("terminal-welcome")
            .debug_selector(|| "terminal-welcome".into())
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.0))
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(420.0))
                    .gap(px(24.0))
                    .child(
                        v_flex()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(28.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(language.text(Message::WelcomeTitle)),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(language.text(Message::WelcomeDescription)),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap(px(12.0))
                            .child(
                                Button::new("welcome-local")
                                    .debug_selector(|| "welcome-local".into())
                                    .w_full()
                                    .h(px(48.0))
                                    .primary()
                                    .icon(IconName::SquareTerminal)
                                    .label(language.text(Message::WelcomeLocal))
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.emit(WelcomeEvent::Local);
                                    })),
                            )
                            .child(
                                Button::new("welcome-ssh")
                                    .debug_selector(|| "welcome-ssh".into())
                                    .w_full()
                                    .h(px(48.0))
                                    .outline()
                                    .icon(
                                        Icon::new(Icon::empty())
                                            .path(crate::gpui_shell::assets::backup::SERVER),
                                    )
                                    .label(language.text(Message::WelcomeSsh))
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.emit(WelcomeEvent::Ssh);
                                    })),
                            ),
                    ),
            )
    }
}
