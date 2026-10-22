use super::*;

impl NebulaWorkspace {
    /// 打开一个 SSH tab（启动器/设置页共用入口）：russh 直连业务层，连接
    /// 阶段/失败原因由业务层回流到 pane（横幅 + grid）。非 config 源的
    /// 目的地自动进保存列表（旧壳 spawn_tab_ssh 同语义）。
    pub fn add_ssh_terminal(
        &mut self,
        destination: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_ssh_terminal_at(destination, None, window, cx);
    }

    pub(super) fn add_ssh_terminal_at(
        &mut self,
        destination: String,
        remote_cwd: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_open {
            self.leave_settings(window, cx);
        }
        // 用保存的 label 当标签名（点「DE」出来的标签就叫 DE），没有 label 才
        // 回落到 pane 的自动标题。
        let label = {
            let mut lists = crate::gpui_shell::ssh_hosts::SshHostLists::load();
            if !lists.is_from_config(&destination) {
                lists.remember(&destination);
                if let Err(err) = lists.persist() {
                    log::warn!("持久化 SSH 主机列表失败: {err}");
                }
            }
            lists.profiles.labels().get(&destination).cloned()
        };
        let grid = self.inherited_grid(cx);
        let launch = crate::gpui_shell::terminal::view::TerminalLaunch::Ssh {
            destination: destination.clone(),
            cwd: remote_cwd,
        };
        let pane = self.new_pane(grid, launch, None, window, cx);
        let focused = pane.id;
        let tab = WorkspaceTab::Terminal {
            tree: SplitTree::leaf(pane.id),
            panes: vec![pane],
            focused,
            zoomed: false,
            broadcast: false,
        };
        let position = nebula_settings::RuntimeSettings::load().new_tab_position;
        let at = new_tab_insert_index(position, self.active, self.tabs.len());
        self.insert_tab_at(
            at,
            tab,
            TabMeta {
                shell_tag: Some("ssh".into()),
                launch: Some(crate::session::LaunchSession::Ssh { host: destination }),
                custom_name: label,
                ..TabMeta::default()
            },
        );
        self.active = at;
        self.reveal_active_tab();
        self.focus_active(window, cx);
        self.sync_side_panel_to_active(true, cx);
        cx.notify();
    }

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
        let (grid, remote_cwd) = {
            let view = old.view.read(cx);
            if view.ssh_destination.as_deref() != Some(destination.as_str()) {
                return;
            }
            (
                (view.grid_cols() as u16, view.grid_rows() as u16),
                (!view.cwd.is_empty()).then(|| view.cwd.clone()),
            )
        };

        let launch = crate::gpui_shell::terminal::view::TerminalLaunch::Ssh {
            destination: destination.clone(),
            cwd: remote_cwd,
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
