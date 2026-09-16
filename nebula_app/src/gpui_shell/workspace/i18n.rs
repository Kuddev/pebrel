use std::path::Path;

use crate::display::UiLanguage;

pub(super) fn close_dialog_text(
    language: UiLanguage,
    process: &str,
    tab: bool,
) -> (String, &'static str, &'static str, &'static str) {
    let body = format!("{process}{}", language.tr("workspace.close.running_process_body"));
    let title = if tab {
        language.tr("workspace.close_tab.title")
    } else {
        language.tr("workspace.close_pane.title")
    };
    let close = language.text(crate::i18n::Message::CommonClose);
    let cancel = language.text(crate::i18n::Message::CommonCancel);
    (body, title, close, cancel)
}

pub(super) fn restore_repeated_failure(language: UiLanguage, path: &Path) -> String {
    language.tr_args(
        "workspace.restore.skipped_repeated_failures",
        &[("path", &path.display().to_string())],
    )
}

pub(super) fn restore_notice(language: UiLanguage, crashed: bool, restored: usize) -> String {
    let key = if crashed { "workspace.restore.after_crash" } else { "workspace.restore.success" };
    language.tr_args(key, &[("restored", &restored.to_string())])
}
