use crate::i18n::{Message, UiLanguage};

pub(super) fn start_failed(is_ssh: bool, error: &str) -> String {
    let message = if is_ssh { Message::TerminalSshStartFailed } else { Message::TerminalPtyStartFailed };
    UiLanguage::current().format(message, &[("error", error)])
}

pub(super) fn process_exited(code: impl std::fmt::Debug) -> String {
    UiLanguage::current().format(Message::TerminalProcessExited, &[("code", &format!("{code:?}"))])
}

pub(super) fn pty_fault(reason: &str) -> String {
    UiLanguage::current().format(Message::TerminalPtyFault, &[("reason", reason)])
}

pub(super) fn session_ended() -> String {
    UiLanguage::current().text(Message::TerminalSessionEnded).to_owned()
}

pub(super) fn answer_label(language: UiLanguage, provider: &str) -> String {
    language.format(Message::TerminalAnswerLabel, &[("provider", provider)])
}
