use super::*;
use crate::gpui_shell::completion_history::{self, ClearState};
use crate::i18n::Message;

impl SettingsPane {
    pub(super) fn command_history_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let busy = completion_history::running(cx);
        let status = match cx.try_global::<ClearState>() {
            Some(ClearState::Finished(Ok(()))) => Some((
                language.text(Message::SettingsCompletionHistoryCleared).to_owned(),
                cx.theme().muted_foreground,
            )),
            Some(ClearState::Finished(Err(error))) => Some((
                language
                    .format(Message::SettingsCompletionHistoryFailed, &[("error", error.as_str())]),
                cx.theme().danger,
            )),
            _ => None,
        };
        v_flex()
            .w_full()
            .child(
                self.row(
                    language.text(Message::SettingsCompletionHistoryTitle),
                    language.text(Message::SettingsCompletionHistoryDescription),
                    NebulaButton::new("command-history-clear")
                        .label(language.text(if busy {
                            Message::SettingsCompletionHistoryClearing
                        } else {
                            Message::SettingsCompletionHistoryClear
                        }))
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.confirm_clear_command_history(window, cx)
                        })),
                    cx,
                ),
            )
            .when_some(status, |row, (message, color)| {
                row.child(
                    div()
                        .id("command-history-status")
                        .debug_selector(|| "command-history-status".to_owned())
                        .px_4()
                        .pb_3()
                        .text_sm()
                        .text_color(color)
                        .child(message),
                )
            })
    }

    fn confirm_clear_command_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if completion_history::running(cx) {
            return;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        window.open_dialog(cx, move |dialog, window, _cx| {
            confirm_dialog(
                dialog,
                window,
                language.text(Message::SettingsCompletionHistoryTitle),
                language.text(Message::SettingsCompletionHistoryConfirm),
                language.text(Message::SettingsCompletionHistoryClear),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
            .on_ok(|_, _, cx| {
                completion_history::clear(cx);
                true
            })
        });
        cx.notify();
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
