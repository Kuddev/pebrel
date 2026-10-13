//! Unix 快速终端仍使用共享窗口注册、会话归属和大小持久化。
use super::*;

pub(in crate::gpui_shell::workspace) fn observe_window_bounds(
    id: u64,
    window: &mut Window,
    cx: &mut Context<NebulaWorkspace>,
) {
    cx.observe_window_bounds(window, move |_, window, cx| {
        quick_terminal_bounds_changed(id, window, cx);
    })
    .detach();
}

pub(crate) fn toggle_quick_terminal_window(cx: &mut App) {
    prune_entries(cx);
    if nebula_settings::RuntimeSettings::load().quick_terminal_mode
        == nebula_settings::QuickTerminalMode::Existing
    {
        if let Some(entry) = entries_by_mru(cx).into_iter().next() {
            focus_entry(&entry, None, cx);
        } else {
            let _ = open_workspace_window(
                cx,
                WorkspaceStartup::NewTerminal { cwd: None },
                None,
                None,
                true,
                WindowRole::Regular,
            );
        }
        return;
    }
    let quick = cx.global::<WindowRegistry>().quick_terminal.as_ref().map(|quick| quick.handle);
    let Some(handle) = quick else {
        persist_quick_size(cx);
        let opened = open_workspace_window(
            cx,
            WorkspaceStartup::NewTerminal { cwd: None },
            None,
            None,
            true,
            WindowRole::QuickTerminal,
        );
        let Ok((runtime_window_id, _)) = opened else {
            log::warn!("Quick terminal creation failed: {}", opened.unwrap_err());
            return;
        };
        let Some(entry) = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id == runtime_window_id)
            .cloned()
        else {
            return;
        };
        cx.global_mut::<WindowRegistry>().quick_terminal = Some(QuickTerminalWindow {
            runtime_window_id,
            handle: entry.handle,
            target_visible: true,
        });
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        let workspace = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.handle == handle)
            .map(|entry| entry.workspace.clone());
        let mut hidden = None;
        if window.is_window_active() {
            if crate::gpui_shell::hide_native_window(window) {
                hidden = Some(true);
                if let Some(quick) = cx.global_mut::<WindowRegistry>().quick_terminal.as_mut() {
                    quick.target_visible = false;
                }
            }
        } else {
            if !crate::gpui_shell::reveal_native_window(window) {
                return;
            }
            window.activate_window();
            hidden = Some(false);
            if let Some(quick) = cx.global_mut::<WindowRegistry>().quick_terminal.as_mut() {
                quick.target_visible = true;
            }
        }
        if let (Some(workspace), Some(hidden)) = (workspace, hidden) {
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.window_hidden = hidden;
                cx.notify();
            });
        }
    });
}

pub(in crate::gpui_shell::workspace) fn quick_terminal_bounds_changed(
    id: u64,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(quick) = cx.global::<WindowRegistry>().quick_terminal.as_ref() else { return };
    if quick.runtime_window_id != id
        || (!quick.target_visible && !window.is_window_active())
        || window.is_maximized()
        || window.is_fullscreen()
    {
        return;
    }
    let bounds = window.window_bounds().get_bounds();
    cx.global_mut::<WindowRegistry>().quick_size_dirty = nebula_settings::QuickTerminalSize::new(
        bounds.size.width.into(),
        bounds.size.height.into(),
    );
}
