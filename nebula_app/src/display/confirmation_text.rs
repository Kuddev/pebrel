//! Localized copy for the legacy renderer's existing confirmation actions.

use super::{NebulaConfirm, truncate_tab_label};
use crate::i18n::{Message, UiLanguage};

pub(super) fn text_for(confirm: &NebulaConfirm, language: UiLanguage) -> (String, String, bool) {
    let text = |message| language.text(message).to_owned();
    match confirm {
        NebulaConfirm::EnableBackgroundImageCoverChrome => {
            (text(Message::ConfirmationCoverTitle), text(Message::ConfirmationCoverBody), false)
        },
        NebulaConfirm::EnablePanelResize => {
            (text(Message::ConfirmationResizeTitle), text(Message::ConfirmationResizeBody), false)
        },
        NebulaConfirm::InstallRequiredFont { .. } => {
            (text(Message::ConfirmationFontTitle), text(Message::ConfirmationFontBody), false)
        },
        NebulaConfirm::ClosePane { process, .. } => (
            text(Message::WorkspaceClosePaneTitle),
            language.format(Message::WorkspaceCloseRunningProcess, &[("process", process)]),
            true,
        ),
        NebulaConfirm::CloseTab { process, .. } => (
            text(Message::WorkspaceCloseTabTitle),
            language.format(Message::WorkspaceCloseRunningProcess, &[("process", process)]),
            true,
        ),
        NebulaConfirm::CloseWindow { process } => (
            text(Message::WorkspaceCloseWindowTitle),
            language.format(Message::WorkspaceCloseRunningWindowProcess, &[("process", process)]),
            true,
        ),
        NebulaConfirm::Paste { lines, .. } => (
            language.format(Message::CommonPasteLines, &[("lines", &lines.to_string())]),
            text(Message::ConfirmationPasteBody),
            false,
        ),
        NebulaConfirm::DeleteSsh { host, from_config } => {
            let host = truncate_tab_label(host, 28);
            let (title, body) = if *from_config {
                (Message::ConfirmationSshHideTitle, Message::ConfirmationSshHideBody)
            } else {
                (Message::ConfirmationSshDeleteTitle, Message::ConfirmationSshDeleteBody)
            };
            (language.format(title, &[("host", &host)]), text(body), true)
        },
        NebulaConfirm::DeleteSftp { entry } => (
            language.format(
                Message::ConfirmationRemoteDeleteTitle,
                &[("name", &truncate_tab_label(&entry.name, 28))],
            ),
            text(if entry.kind == crate::ssh_sftp::SftpEntryKind::Directory {
                Message::ConfirmationRemoteDeleteDirectoryBody
            } else {
                Message::ConfirmationRemoteDeleteFileBody
            }),
            true,
        ),
        NebulaConfirm::DeleteFileTreePath { path, is_dir } => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            (
                language
                    .format(Message::FilesDeleteTitle, &[("name", &truncate_tab_label(&name, 28))]),
                text(if *is_dir {
                    Message::FilesDeleteFolderBody
                } else {
                    Message::FilesDeleteFileBody
                }),
                true,
            )
        },
        NebulaConfirm::BackupPassphrase { restoring } => (
            text(if *restoring {
                Message::ConfirmationRestorePassphraseTitle
            } else {
                Message::ConfirmationBackupPassphraseTitle
            }),
            text(if *restoring {
                Message::ConfirmationRestorePassphraseBody
            } else {
                Message::ConfirmationBackupPassphraseBody
            }),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_warning_resolves_language_and_preserves_the_process_name() {
        let process = "opencode {process} 世界";
        let confirm = NebulaConfirm::CloseWindow { process: process.into() };
        let (title, body, danger) = text_for(&confirm, UiLanguage::EnUs);
        assert_eq!(title, "Close window?");
        assert_eq!(body, format!("{process} is still running. Closing the window will stop it."));
        assert!(danger);
        let (translated_title, translated_body, translated_danger) =
            text_for(&confirm, UiLanguage::ZhCn);
        assert_ne!(translated_title, title);
        assert!(translated_body.starts_with(process));
        assert_eq!(translated_danger, danger);
    }

    #[test]
    fn passphrase_prompts_distinguish_creating_a_backup_from_restoring_one() {
        let (title, body, danger) =
            text_for(&NebulaConfirm::BackupPassphrase { restoring: false }, UiLanguage::EnUs);
        assert!(title.contains("backup"));
        assert!(body.contains("8 characters"));
        assert!(!danger);
        let (title, body, danger) =
            text_for(&NebulaConfirm::BackupPassphrase { restoring: true }, UiLanguage::EnUs);
        assert!(title.contains("restore"));
        assert!(body.contains("authentication succeeds"));
        assert!(!danger);
    }
}
