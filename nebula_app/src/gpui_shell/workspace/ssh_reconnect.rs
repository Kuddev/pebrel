//! Replace a disconnected SSH pane while preserving its own remote recovery target.
use super::*;

impl NebulaWorkspace {
    /// 在同一 tab、同一分屏位置替换失败的 SSH pane。先把新实体及订阅完整
    /// 建好，再原子替换树叶和 pane 所有权；旧实体的异步泵只会更新旧 Entity，
    /// 因而无法把迟到的 Failed/Ready 写进新连接。
    pub(super) fn retry_ssh_pane(
        &mut self,
        tab_ix: usize,
        pane_id: u64,
        destination: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(tab_ix) else { return };
        let Some(old) = panes.iter().find(|pane| pane.id == pane_id) else { return };
        let custom_name = old.custom_name.clone();
        let name_history = old.name_history.clone();
        let (grid, remote_cwd, resume_target) = {
            let view = old.view.read(cx);
            if view.ssh_destination.as_deref() != Some(destination.as_str()) {
                return;
            }
            (
                (view.grid_cols() as u16, view.grid_rows() as u16),
                (!view.cwd.is_empty()).then(|| view.cwd.clone()),
                view.session
                    .as_ref()
                    .and_then(|session| session.resume_target.lock().ok()?.clone()),
            )
        };

        let launch = crate::gpui_shell::terminal::view::TerminalLaunch::Ssh {
            destination: destination.clone(),
            cwd: remote_cwd,
            resume_target,
        };
        let mut replacement = self.new_pane(grid, launch, None, window, cx);
        replacement.custom_name = custom_name;
        replacement.name_history = name_history;
        self.forget_pane_rename(pane_id);
        let replacement_id = replacement.id;
        let old = {
            let Some(WorkspaceTab::Terminal { panes, tree, focused, .. }) =
                self.tabs.get_mut(tab_ix)
            else {
                replacement.view.read(cx).shutdown();
                return;
            };
            let Some(index) = panes.iter().position(|pane| pane.id == pane_id) else {
                replacement.view.read(cx).shutdown();
                return;
            };
            if !tree.replace_leaf(pane_id, replacement_id) {
                replacement.view.read(cx).shutdown();
                return;
            }
            if *focused == pane_id {
                *focused = replacement_id;
            }
            std::mem::replace(&mut panes[index], replacement)
        };

        self.runtime_hub.record_pane_closed(self.runtime_window_id, pane_id);
        self.pane_bounds.borrow_mut().remove(&pane_id);
        old.view.read(cx).shutdown();
        if tab_ix == self.active {
            self.focus_active(window, cx);
            self.sync_side_panel_to_active(true, cx);
        }
        cx.notify();
    }
}
