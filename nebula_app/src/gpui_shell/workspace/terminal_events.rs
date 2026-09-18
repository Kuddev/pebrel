//! Terminal-to-workspace lifecycle and presentation events.
use super::*;

impl NebulaWorkspace {
    pub(super) fn on_terminal_event(
        &mut self,
        view: &Entity<TerminalView>,
        event: &TerminalViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalViewEvent::ScreenChanged => {
                if let Some((_, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.runtime_hub.screens.changed(self.runtime_window_id, pane_id);
                }
            },
            TerminalViewEvent::SessionIdentityChanged => {
                if let Err(error) = windowing::save_current_window_session(
                    self.runtime_window_id,
                    self.snapshot_session(cx),
                    session_persistence::SaveReason::Checkpoint,
                    cx,
                ) {
                    log::warn!("Could not checkpoint native recovery identity: {error}");
                }
            },
            // OSC 7 cwd 与标题共用这条事件。只有当前聚焦 pane 能驱动共享文件树；
            // 后台 pane 的提示符更新不能把前台目录覆盖掉。
            TerminalViewEvent::TitleChanged => {
                let is_active_pane = self
                    .tabs
                    .get(self.active)
                    .and_then(WorkspaceTab::focused_view)
                    .is_some_and(|active| active.entity_id() == view.entity_id());
                if is_active_pane {
                    self.sync_side_panel_to_active(false, cx);
                }
                cx.notify();
            },
            TerminalViewEvent::Exited => {
                if let Some((tab_ix, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.runtime_hub.record_pane_exited(self.runtime_window_id, pane_id);
                    self.close_pane(tab_ix, pane_id, window, cx);
                }
            },
            TerminalViewEvent::FocusRequested => {
                if let Some((tab_ix, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.focus_pane(tab_ix, pane_id, window, cx);
                }
            },
            // SSH 连接卡片的取消/关闭：这个 pane 除了这条连接没有别的
            // 内容（旧壳 TabRequest::Close 同一裁定）。
            TerminalViewEvent::RequestClose => {
                if let Some((tab_ix, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.request_close_pane(tab_ix, pane_id, window, cx);
                }
            },
            TerminalViewEvent::RetrySsh(destination) => {
                if let Some((tab_ix, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.retry_ssh_pane(tab_ix, pane_id, destination.clone(), window, cx);
                }
            },
            TerminalViewEvent::FontSizeChanged => self.apply_runtime_settings(cx),
            // 任务栏是窗口级的，只反映**正被看着的那个 pane**：后台 tab 里的
            // 构建进度投到同一个按钮上只会互相覆盖，读数还不如没有。
            TerminalViewEvent::ProgressChanged(progress) => {
                if let Some((tab_ix, pane_id)) = self.locate_pane(view.entity_id())
                    && tab_ix == self.active
                    && matches!(
                        self.tabs.get(tab_ix),
                        Some(WorkspaceTab::Terminal { focused, .. }) if *focused == pane_id
                    )
                {
                    crate::taskbar::apply(windowing::native_hwnd(window).unwrap_or(0), *progress);
                }
                // 后台 pane 也要刷新自己的 tab badge；一次协议事件只触发一次
                // workspace render，只有 Running 状态会在 render 后续接共享时钟。
                cx.notify();
            },
            TerminalViewEvent::Bell => {
                if let Some((tab_ix, _)) = self.locate_pane(view.entity_id())
                    && tab_ix != self.active
                {
                    if let Some(meta) = self.tab_meta.get_mut(tab_ix) {
                        meta.has_bell = true;
                    }
                    cx.notify();
                }
            },
            // 视图无条件上报用户输入，宿主在事件发生时检查广播开关并扇出。
            TerminalViewEvent::UserInput(input) => {
                if let Some((_, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.fan_out_broadcast(pane_id, input, cx);
                }
            },
            TerminalViewEvent::AiAttention(attention) => {
                if let Some((_, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.deliver_pane_notification(
                        pane_id,
                        crate::notify::Notification::AiTurn {
                            program: attention.source.clone(),
                            message: Some(attention.summary_for_pane(pane_id)),
                            attention: true,
                        },
                        window,
                        cx,
                    );
                }
            },
            TerminalViewEvent::Notification(notification) => {
                if let Some((_, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.deliver_pane_notification(pane_id, notification.clone(), window, cx);
                }
            },
            TerminalViewEvent::SelectionContextMenuRequested { position, text } => {
                if let Some((_, pane_id)) = self.locate_pane(view.entity_id()) {
                    self.open_terminal_selection_context_menu(
                        view.clone(),
                        pane_id,
                        *position,
                        text.clone(),
                        window,
                        cx,
                    );
                }
            },
        }
    }
}
