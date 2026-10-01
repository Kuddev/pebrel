//! Process-level shell events select live windows and defer installed notes until visible.
use super::*;
use super::super::{ssh_dialog, update_dialog};

pub(crate) fn dispatch_shell_events(events: Vec<GpuiShellEvent>, cx: &mut App) {
    for event in events {
        match event {
            GpuiShellEvent::NotificationFocus(pane_id) => focus_notification(pane_id, cx),
            GpuiShellEvent::NotificationChoice { pane_id, request_id, choice } => {
                let feedback = entry_with_pane(pane_id, cx).map(|entry| entry.handle);
                crate::gpui_shell::toast::reply_to_choice(
                    pane_id, request_id, choice, feedback, cx,
                );
            },
            GpuiShellEvent::TrayFocus(pane_id) => {
                let target =
                    pane_id.and_then(|pane_id| entry_with_pane(pane_id, cx)).or_else(|| {
                        select_mru_window(
                            nebula_settings::RuntimeSettings::load().windowing_behavior,
                            cx,
                        )
                    });
                if let Some(entry) = target {
                    focus_entry(&entry, pane_id, cx);
                }
            },
            GpuiShellEvent::TrayQuit => {
                quit_all(cx);
                return;
            },
            GpuiShellEvent::MuxAttach => {
                if let Some(entry) = entries_by_mru(cx).into_iter().next() {
                    focus_entry(&entry, None, cx);
                }
            },
            GpuiShellEvent::RuntimeControl(dispatch) => dispatch_runtime(dispatch, cx),
            GpuiShellEvent::UpdateInstalled(notes) => {
                update_dialog::queue_installed_notes(notes, cx);
            },
            event @ GpuiShellEvent::UpdateAvailable(_) => {
                let Some(entry) = entries_by_mru(cx).into_iter().next() else { continue };
                let _ = entry.handle.update(cx, move |_, window, cx| {
                    update_dialog::show_update_event(event, window, cx);
                });
            },
            GpuiShellEvent::SshPrompt(request) => {
                let Some(entry) = entries_by_mru(cx).into_iter().next() else {
                    request.respond(crate::ssh_prompt::PromptResponse::Cancel);
                    continue;
                };
                let pending = request.clone();
                if entry
                    .handle
                    .update(cx, move |_, window, cx| {
                        ssh_dialog::show(pending, window, cx);
                    })
                    .is_err()
                {
                    request.respond(crate::ssh_prompt::PromptResponse::Cancel);
                }
            },
            GpuiShellEvent::OpenDirectories(urls) => {
                for url in urls {
                    let Some(path) = crate::file_uri::file_uri_to_local_path(&url) else {
                        continue;
                    };
                    if let Err(error) = open_new_window(cx, Some(path)) {
                        log::warn!("Could not open a desktop folder: {error}");
                    }
                }
            },
        }
    }
    if update_dialog::has_pending_installed_notes(cx)
        && let Some(entry) = entries_by_mru(cx).into_iter().find(|entry| {
            entry.workspace.upgrade().is_some_and(|workspace| !workspace.read(cx).window_hidden)
        })
    {
        let _ = entry.handle.update(cx, |_, window, cx| {
            update_dialog::show_pending_installed_notes(window, cx);
        });
    }
    publish_runtime_snapshot(cx);
}
