//! Update recovery owns its target-window identities and partial creation retries.
use super::*;

pub(super) fn ready_update_windows(cx: &App) -> Option<usize> {
    let registry = cx.global::<WindowRegistry>();
    let windows = registry.update_restore_windows.as_ref()?;
    for id in windows {
        let id = (*id)?;
        // 已创建后被用户关闭的窗口视为已处理；未创建的槽位不能由无关窗口补足。
        let Some(workspace) = registry
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id == id)
            .and_then(|entry| entry.workspace.upgrade())
        else {
            continue;
        };
        if !workspace.read(cx).tabs.iter().all(|tab| match tab {
            WorkspaceTab::Terminal { panes, .. } => {
                panes.iter().all(|pane| pane.view.read(cx).recovery_ready())
            },
            _ => true,
        }) {
            return None;
        }
    }
    Some(windows.len())
}

pub(super) fn open_update_windows(
    sessions: Vec<crate::session::Session>,
    mut ai_events: Option<std::sync::mpsc::Receiver<crate::ai_hook::AiHookEvent>>,
    mut shell_events: Option<std::sync::mpsc::Receiver<GpuiShellEvent>>,
    cx: &mut App,
) -> Result<(u64, u64), ApiError> {
    if sessions.is_empty() {
        return Err(ApiError::new("invalid_update_ticket", "Empty update workspace"));
    }
    let windows = cx
        .global_mut::<WindowRegistry>()
        .update_restore_windows
        .get_or_insert_with(|| vec![None; sessions.len()])
        .clone();
    if windows.len() != sessions.len() {
        return Err(ApiError::new(
            "invalid_update_ticket",
            "Update workspace changed during recovery",
        ));
    }
    let mut first = None;
    let mut failure = None;
    for (index, session) in sessions.into_iter().enumerate() {
        if let Some(id) = windows[index] {
            first.get_or_insert((id, 0));
            continue;
        }
        match open_workspace_window(
            cx,
            WorkspaceStartup::RestoreUpdate(session),
            ai_events.take(),
            shell_events.take(),
            true,
            WindowRole::Regular,
        ) {
            Ok((id, workspace)) => {
                cx.global_mut::<WindowRegistry>().update_restore_windows.as_mut().unwrap()[index] =
                    Some(id);
                first.get_or_insert((
                    id,
                    workspace.read(cx).active_terminal_pane_id().unwrap_or_default(),
                ));
            },
            Err(error) => {
                log::warn!("Could not reopen an updated window: {error}");
                failure = Some(ApiError::new("window_create_failed", error.to_string()));
                if first.is_none() {
                    break;
                }
            },
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    first.ok_or_else(|| ApiError::new("window_create_failed", "No update window was restored"))
}
