//! Application shutdown: approve drafts, capture durable state, then stop PTYs.
use super::*;
use crate::i18n::Message;

pub(crate) fn quit_all(cx: &mut App) {
    request_quit(None, cx);
}

pub(crate) fn quit_for_update(asset: crate::update_check::UpdateAsset, cx: &mut App) {
    request_quit(Some(asset), cx);
}

fn request_quit(update: Option<crate::update_check::UpdateAsset>, cx: &mut App) {
    if cx.global::<WindowRegistry>().quit_pending {
        return;
    }
    cx.global_mut::<WindowRegistry>().quit_pending = true;
    cx.spawn(async move |cx| {
        let Some(approved) = approve_documents(cx).await else {
            cx.update(|cx| cx.global_mut::<WindowRegistry>().quit_pending = false);
            return;
        };
        let panes = cx.update(|cx| {
            cx.global::<WindowRegistry>()
                .entries
                .clone()
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
        let ready = super::super::closing::wait_for_session_ids(&panes, cx).await;
        let mut prepared = if ready && let Some(asset) = update {
            let result = cx
                .background_executor()
                .spawn(async move { crate::update_download::handoff::prepare(&asset) })
                .await;
            match result {
                Ok(prepared) => Some(prepared),
                Err(error) => {
                    log::warn!("Could not prepare update: {error}");
                    cx.update(|cx| abort_update(&error, cx));
                    return;
                },
            }
        } else {
            None
        };
        cx.update(|cx| {
            // Recheck the current pane set after asynchronous preparation. A
            // window/pane opened during the handshake also needs a native target.
            let identities_ready = cx
                .global::<WindowRegistry>()
                .entries
                .iter()
                .filter_map(|entry| entry.workspace.upgrade())
                .all(|workspace| {
                    workspace.read(cx).tabs.iter().all(|tab| match tab {
                        WorkspaceTab::Terminal { panes, .. } => {
                            panes.iter().all(|pane| !pane.view.read(cx).ai_session_save_pending())
                        },
                        _ => true,
                    })
                });
            if !ready || !identities_ready {
                abort_quit(Message::SessionIdentityPending, cx);
                return;
            }
            if !documents_unchanged(&approved, cx) {
                abort_quit(Message::UpdateDraftChanged, cx);
                return;
            }
            if let Err(error) = save_combined_session(cx, true) {
                log::warn!("Shutdown cancelled because session save failed: {error}");
                abort_quit(Message::SessionSaveFailed, cx);
                return;
            }
            if let Some(prepared) = prepared.as_mut() {
                // Use the same durable snapshot as a scheduled update, including
                // the last window closed to the tray and the active-window order.
                let result = cx
                    .global::<WindowRegistry>()
                    .session_persistence
                    .update_windows()
                    .map_err(|error| error.to_string())
                    .and_then(|snapshots| prepared.commit(&snapshots));
                if let Err(error) = result {
                    log::warn!("Update commit failed: {error}");
                    cx.global_mut::<WindowRegistry>().session_persistence.cancel_quit();
                    abort_update(&error, cx);
                    return;
                }
            }
            finish_quit_all(cx);
        });
    })
    .detach();
}

fn abort_update(error: &str, cx: &mut App) {
    cx.global_mut::<WindowRegistry>().quit_pending = false;
    for entry in cx.global::<WindowRegistry>().entries.clone() {
        let _ = entry.handle.update(cx, |_, window, cx| {
            let language = crate::gpui_shell::config::ui_language(cx);
            let text = language.format(Message::UpdatePrepareFailed, &[("error", error)]);
            crate::gpui_shell::toast::banner(window, cx, crate::display::ToastKind::Warning, text);
        });
    }
}

fn abort_quit(message: Message, cx: &mut App) {
    cx.global_mut::<WindowRegistry>().quit_pending = false;
    for entry in cx.global::<WindowRegistry>().entries.clone() {
        let _ = entry.handle.update(cx, |_, window, cx| {
            let language = crate::gpui_shell::config::ui_language(cx);
            crate::gpui_shell::toast::banner(
                window,
                cx,
                crate::display::ToastKind::Warning,
                language.text(message),
            );
        });
    }
}

type ApprovedDrafts = Vec<(Entity<crate::gpui_shell::file_editor::TextFileView>, SharedString)>;

async fn approve_documents(cx: &mut gpui::AsyncApp) -> Option<ApprovedDrafts> {
    let files = cx.update(|cx| {
        cx.global::<WindowRegistry>()
            .entries
            .iter()
            .flat_map(|entry| {
                entry
                    .workspace
                    .upgrade()
                    .map(|workspace| {
                        workspace
                            .read(cx)
                            .tabs
                            .iter()
                            .filter_map(|tab| tab.file_editor(cx))
                            .map(|file| (entry.handle, file))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
    });
    let mut approved = Vec::new();
    for (handle, file) in &files {
        let check = cx.update(|cx| (file.read(cx).is_dirty(), file.read(cx).is_saving()));
        if check.1 {
            return None;
        }
        if !check.0 {
            continue;
        }
        let prompt = handle.update(cx, |_, window, cx| {
            let language = crate::gpui_shell::config::ui_language(cx);
            let draft = file.read(cx).draft(cx);
            let prompt = window.prompt(
                gpui::PromptLevel::Warning,
                language.text(Message::EditorCloseTitle),
                Some(&file.read(cx).source_label()),
                &[
                    language.text(Message::EditorSave),
                    language.text(Message::EditorDiscard),
                    language.text(Message::EditorCancel),
                ],
                cx,
            );
            (prompt, draft)
        });
        let Ok((prompt, draft)) = prompt else {
            return None;
        };
        match prompt.await {
            Ok(0) => {
                let save = file.update(cx, |file, cx| file.save(cx));
                if !save.await {
                    return None;
                }
            },
            Ok(1) => approved.push((file.clone(), draft)),
            _ => return None,
        }
    }
    cx.update(|cx| documents_unchanged(&approved, cx)).then_some(approved)
}

fn documents_unchanged(approved: &ApprovedDrafts, cx: &App) -> bool {
    cx.global::<WindowRegistry>().entries.iter().filter_map(|entry| entry.workspace.upgrade()).all(
        |workspace| {
            workspace.read(cx).tabs.iter().filter_map(|tab| tab.file_editor(cx)).all(|file| {
                let view = file.read(cx);
                !view.is_saving()
                    && (!view.is_dirty()
                        || approved
                            .iter()
                            .any(|(accepted, draft)| accepted == &file && *draft == view.draft(cx)))
            })
        },
    )
}

fn finish_quit_all(cx: &mut App) {
    prune_entries(cx);
    for entry in cx.global::<WindowRegistry>().entries.clone() {
        let workspace = entry.workspace.clone();
        let _ = entry.handle.update(cx, move |_, _, cx| {
            let _ = workspace.update(cx, |workspace, cx| workspace.shutdown_terminal_panes(cx));
        });
    }
    crate::tray::shutdown();
    cx.quit();
}
