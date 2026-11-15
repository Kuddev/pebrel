//! Confirmation and completion for the remote file selection.

use super::*;
use crate::gpui_shell::toast::{ToastKind, banner};
use crate::ssh_sftp::SftpDeleteError;

impl NebulaWorkspace {
    pub(super) fn request_remote_deletion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.current_remote_transfer_target() else { return };
        let entries = self.selected_remote_entries();
        if entries.is_empty()
            || self.remote_browser.needs_refresh
            || self.remote_file_operation_busy()
        {
            return;
        }

        let language = crate::gpui_shell::config::ui_language(cx);
        let title: SharedString = if let [entry] = entries.as_slice() {
            language.format(Message::ConfirmationRemoteDeleteTitle, &[("name", &entry.name)]).into()
        } else {
            language
                .format(Message::RemoteDeleteTitle, &[("count", &entries.len().to_string())])
                .into()
        };
        let body = if entries.iter().any(|entry| entry.kind == SftpEntryKind::Directory) {
            Message::RemoteDeleteFolderBody
        } else {
            Message::RemoteDeleteBody
        };
        let workspace = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            let workspace = workspace.clone();
            let target = target.clone();
            let entries = entries.clone();
            confirm_dialog(
                dialog,
                window,
                title.clone(),
                language.text(body),
                language.text(Message::CommonDelete),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| {
                    this.start_remote_deletion(target.clone(), entries.clone(), window, cx);
                });
                true
            })
        });
    }

    fn start_remote_deletion(
        &mut self,
        target: RemoteTransferTarget,
        entries: Vec<SftpEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_remote_browser_to_active(cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        if self.remote_browser.needs_refresh || !self.remote_transfer_target_matches(&target) {
            banner(window, cx, ToastKind::Warning, language.text(Message::TransferTargetChanged));
            return;
        }
        if self.remote_file_operation_busy() {
            banner(window, cx, ToastKind::Warning, language.text(Message::TransferBusy));
            return;
        }
        let Some(session) = self.remote_browser.browse_sessions.get(&target.pane).cloned() else {
            banner(
                window,
                cx,
                ToastKind::Warning,
                language.text(Message::FilesRemoteSessionUnavailable),
            );
            return;
        };
        self.remote_browser.last_outcome = None;
        self.remote_browser.deletion = Some(cx.spawn_in(window, async move |this, cx| {
            let path = target.path.clone();
            let result =
                remote_call(move || async move { session.delete_entries(&path, entries).await })
                    .await;
            let _ = this.update_in(cx, |workspace, window, cx| {
                workspace.remote_browser.deletion = None;
                workspace.refresh_after_remote_deletion(&target, cx);
                if let Some(message) =
                    deletion_failure(result, crate::gpui_shell::config::ui_language(cx))
                {
                    banner(window, cx, ToastKind::Warning, message);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn refresh_after_remote_deletion(
        &mut self,
        target: &RemoteTransferTarget,
        cx: &mut Context<Self>,
    ) {
        // A batch can stop after some removals, or the reply can be lost.
        // Relist instead of guessing the remote state or repeating REMOVE.
        let browser = &mut self.remote_browser;
        for snapshot in browser.snapshots.values_mut() {
            if snapshot.destination == target.destination && snapshot.path == target.path {
                snapshot.needs_refresh = true;
            }
        }
        if browser.destination == target.destination && browser.path == target.path {
            browser.needs_refresh = true;
            let staying = browser.pending_path.as_deref().is_none_or(|path| path == target.path);
            if staying {
                self.navigate_remote_with_mode(
                    target.path.clone(),
                    SftpBrowseMode::RecoverMissing,
                    cx,
                );
            }
        }
    }
}

fn deletion_failure(
    result: Option<Result<(), SftpDeleteError>>,
    language: crate::display::UiLanguage,
) -> Option<String> {
    Some(match result {
        Some(Ok(())) => return None,
        Some(Err(SftpDeleteError::Connection(error))) => error,
        Some(Err(SftpDeleteError::Changed(name))) => {
            language.format(Message::RemoteDeleteChanged, &[("name", &name)])
        },
        Some(Err(SftpDeleteError::Remote { name, error })) => language
            .format(Message::RemoteDeleteFailed, &[("name", &name), ("error", &error.to_string())]),
        None => language.text(Message::FilesRemoteConnectionUnavailable).to_owned(),
    })
}
