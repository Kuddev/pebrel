//! Ended-session presentation leaves terminal history available for selection.

use super::*;
use crate::gpui_shell::widgets::NebulaButton;
use crate::i18n::Message;

impl TerminalView {
    pub(super) fn session_input_alive(&self) -> bool {
        self.exited.is_none()
            && !matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
    }

    pub(super) fn clear_session_progress(&mut self, cx: &mut Context<Self>) {
        if self.progress != crate::taskbar::TaskProgress::None {
            self.progress = crate::taskbar::TaskProgress::None;
            cx.emit(TerminalViewEvent::ProgressChanged(self.progress));
        }
    }

    pub(super) fn ended_session_status(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let exited = self.exited.as_ref()?;
        let Some(destination) = self.ssh_destination.clone() else {
            return Some(
                div()
                    .absolute()
                    .bottom_2()
                    .left_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(gpui::opaque_grey(0.2, 0.9))
                    .text_sm()
                    .child(exited.clone()),
            );
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let status = div()
            .absolute()
            .bottom_2()
            .left_2()
            .right_2()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme.background.opacity(0.95))
            .text_color(theme.foreground)
            .text_sm();
        let status = status.child(language.text(Message::SettingsSessionEnded)).child(
            NebulaButton::new("ssh-session-reconnect")
                .label(language.text(Message::SettingsSessionReconnect))
                .outline()
                .on_click(cx.listener(move |view, _, _, cx| {
                    if view.exited.is_some()
                        && view.ssh_destination.as_deref() == Some(destination.as_str())
                    {
                        cx.emit(TerminalViewEvent::RetrySsh(destination.clone()));
                    }
                })),
        );
        Some(status)
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
impl TerminalView {
    pub(crate) fn lifecycle_test_ssh_failed(&self) -> bool {
        matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
    }

    pub(crate) fn lifecycle_test_grid_contains(&self, text: &str) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session
                .term
                .lock()
                .grid()
                .display_iter()
                .map(|cell| cell.cell.c)
                .collect::<String>()
                .contains(text)
        })
    }

    pub(crate) fn lifecycle_test_ssh_ready(&mut self, cx: &mut Context<Self>) {
        self.apply_ssh_stage(crate::ssh_session::SshStage::Ready, cx);
        super::startup_tests::feed(self, b"retained remote output\r\n");
    }

    pub(crate) fn lifecycle_test_event(&mut self, event: TermEvent, cx: &mut Context<Self>) {
        self.process_event(event, cx);
    }
}
