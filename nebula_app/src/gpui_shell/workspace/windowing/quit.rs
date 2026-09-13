//! Application quit is a two-phase operation: collect document consent across
//! all windows, then prepare sessions and recheck the approved drafts before exit.

use super::super::documents::{DocumentCloseApproval, approve_file_close};
use super::*;

pub(crate) fn quit_all(cx: &mut App) {
    if cx.global::<WindowRegistry>().quit_pending {
        return;
    }
    prune_entries(cx);
    let entries = cx.global::<WindowRegistry>().entries.clone();
    if entries.iter().any(|entry| {
        entry
            .workspace
            .update(cx, |workspace, cx| {
                workspace.window_close_pending
                    || workspace.window_close_confirm_open
                    || workspace.document_editors(cx).iter().any(|file| file.read(cx).is_saving())
            })
            .unwrap_or(false)
    }) {
        return;
    }
    cx.global_mut::<WindowRegistry>().quit_pending = true;
    for entry in &entries {
        let _ = entry.workspace.update(cx, |workspace, _| {
            workspace.window_close_confirm_open = true;
        });
    }
    cx.spawn(async move |cx| {
        let mut approval = DocumentCloseApproval::default();
        for entry in &entries {
            let Ok(files) =
                entry.workspace.update(cx, |workspace, cx| workspace.document_editors(cx))
            else {
                continue;
            };
            if cx.update(|cx| files.iter().any(|file| file.read(cx).is_dirty())) {
                let _ = entry.handle.update(cx, |_, window, cx| {
                    let _ = entry.workspace.update(cx, |workspace, _| {
                        focus_workspace_window(workspace, window);
                    });
                });
            }
            let Some(accepted) = approve_file_close(&files, entry.handle, cx).await else {
                cx.update(|cx| cancel_quit(&entries, cx));
                return;
            };
            approval.extend(accepted);
        }
        if !cx.update(|cx| all_documents_approved(&approval, cx)) {
            cx.update(|cx| cancel_quit(&entries, cx));
            return;
        }
        let panes = cx.update(|cx| {
            let current = cx.global::<WindowRegistry>().entries.clone();
            current
                .iter()
                .filter(|entry| entry.role == WindowRole::Regular)
                .filter_map(|entry| {
                    entry
                        .workspace
                        .update(cx, |workspace, cx| workspace.prepare_session_save(cx))
                        .ok()
                })
                .flatten()
                .collect::<Vec<_>>()
        });
        super::super::closing::wait_for_session_ids(&panes, cx).await;
        cx.update(|cx| {
            // Other windows remain editable while prompts or session discovery
            // are pending. Consent applies only to the exact discarded text.
            if all_documents_approved(&approval, cx) {
                finish_quit_all(cx);
            } else {
                cancel_quit(&entries, cx);
            }
        });
    })
    .detach();
}

fn all_documents_approved(approval: &DocumentCloseApproval, cx: &mut App) -> bool {
    let entries = cx.global::<WindowRegistry>().entries.clone();
    entries.iter().all(|entry| {
        entry
            .workspace
            .update(cx, |workspace, cx| {
                workspace.document_editors(cx).iter().all(|file| approval.allows(file, cx))
            })
            .unwrap_or(true)
    })
}

fn cancel_quit(entries: &[WindowEntry], cx: &mut App) {
    cx.global_mut::<WindowRegistry>().quit_pending = false;
    for entry in entries {
        let _ = entry.workspace.update(cx, |workspace, cx| {
            workspace.window_close_confirm_open = false;
            cx.notify();
        });
    }
}

fn finish_quit_all(cx: &mut App) {
    save_combined_session(cx, true);
    prune_entries(cx);
    let entries = cx.global::<WindowRegistry>().entries.clone();
    for entry in entries {
        let workspace = entry.workspace.clone();
        let _ = entry.handle.update(cx, move |_, _window, cx| {
            let _ = workspace.update(cx, |workspace, cx| workspace.shutdown_terminal_panes(cx));
        });
    }
    crate::tray::shutdown();
    cx.quit();
}
