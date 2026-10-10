//! pane 原位换启动身份：先建好新 pane 与订阅，再换树叶与所有权，旧 pane 的
//! 异步泵因此无法把迟到事件写进新会话。
//!
//! 两个调用方共用这一份实现：SSH 直连失败后的"重试"（`retry_ssh_pane`）与
//! 提示符 ssh 标签拉起的远程 Claude 会话（`workspace::remote_claude`）。

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
        let remote_cwd = {
            let view = old.view.read(cx);
            if view.ssh_destination.as_deref() != Some(destination.as_str()) {
                return;
            }
            (!view.cwd.is_empty()).then(|| view.cwd.clone())
        };

        let launch = crate::gpui_shell::terminal::view::TerminalLaunch::Ssh {
            destination: destination.clone(),
            cwd: remote_cwd,
        };
        self.swap_pane_launch(tab_ix, pane_id, launch, None, None, window, cx);
    }

    /// 在同一个 tab、同一个分屏位置把 pane 换成另一份启动身份：先建好新 pane
    /// 与订阅，再原子换叶与所有权（旧 pane 的异步泵无法把迟到事件写进新会话）。
    /// `session_launch` 同时写进 tab meta（冷恢复按它重建）；`shell_tag` 为
    /// `None` 时保留原短标。
    pub(super) fn swap_pane_launch(
        &mut self,
        tab_ix: usize,
        pane_id: u64,
        launch: crate::gpui_shell::terminal::view::TerminalLaunch,
        session_launch: Option<crate::session::LaunchSession>,
        shell_tag: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(tab_ix) else { return };
        let Some(old) = panes.iter().find(|pane| pane.id == pane_id) else { return };
        let custom_name = old.custom_name.clone();
        let name_history = old.name_history.clone();
        let grid = {
            let view = old.view.read(cx);
            (view.grid_cols() as u16, view.grid_rows() as u16)
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
        // 这个 tab 的启动身份也换成会话本身：冷恢复按它重建，侧栏短标跟着对
        // （与 SSH tab 的 "ssh" 同一语义）。
        if let Some(session_launch) = session_launch
            && let Some(meta) = self.tab_meta.get_mut(tab_ix)
        {
            if let Some(shell_tag) = shell_tag {
                meta.shell_tag = Some(shell_tag);
            }
            meta.launch = Some(session_launch);
        }
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
