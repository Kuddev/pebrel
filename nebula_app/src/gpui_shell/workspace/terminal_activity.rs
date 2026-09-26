//! Terminal presentation activity and semantic event routing for the workspace.

use super::*;

impl WorkspaceTab {
    /// Shared with the zoomed/single-pane renderer, including its empty-focus fallback.
    pub(super) fn primary_terminal_pane(&self) -> Option<&TerminalPane> {
        let Self::Terminal { panes, focused, .. } = self else { return None };
        panes.iter().find(|pane| pane.id == *focused).or_else(|| panes.first())
    }
}

impl NebulaWorkspace {
    pub(super) fn on_terminal_event(
        &mut self,
        view: &Entity<TerminalView>,
        event: &TerminalViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
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

    pub(super) fn sync_terminal_activity(&self, cx: &mut Context<Self>) {
        for (index, tab) in self.tabs.iter().enumerate() {
            let WorkspaceTab::Terminal { panes, tree, zoomed, .. } = tab else { continue };
            let active = index == self.active && !self.settings_open && !self.window_hidden;
            let primary = tab.primary_terminal_pane().map(|pane| pane.id);
            for pane in panes {
                let visible = active && (!*zoomed && !tree.is_leaf() || primary == Some(pane.id));
                pane.view.update(cx, |view, cx| view.set_output_visible(visible, cx));
            }
        }
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use crate::gpui_shell::terminal::view::TerminalLaunch;
    use gpui::{TestAppContext, VisualTestContext};
    use std::cell::Cell;

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn rendered_tabs_splits_zoom_and_settings_restore_only_presented_panes(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::gpui_shell::init(cx);
            windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
            cx.set_reduce_motion(true);
        });
        let mut workspace = None;
        let mut terminals = Vec::new();
        let (_, window) = cx.add_window_view(|window, cx| {
            let entity = cx.new(|cx| {
                let mut workspace = NebulaWorkspace::new(
                    window,
                    None,
                    None,
                    1,
                    crate::runtime_api::RuntimeHub::new(),
                    windowing::WorkspaceStartup::Empty,
                    windowing::WindowRole::Regular,
                    cx,
                );
                for count in [2, 1] {
                    let panes: Vec<_> = (0..count)
                        .map(|_| {
                            workspace.new_pane(
                                (80, 24),
                                TerminalLaunch::Local {
                                    cwd: None,
                                    shell: Some(nebula_terminal::tty::Shell::new(
                                        "pebrel-test-missing-shell-executable".into(),
                                        vec![],
                                    )),
                                    shell_name: None,
                                },
                                None,
                                window,
                                cx,
                            )
                        })
                        .collect();
                    terminals.extend(panes.iter().map(|pane| pane.view.clone()));
                    let focused = panes[0].id;
                    let mut tree = SplitTree::Leaf(focused);
                    if panes.len() == 2 {
                        tree.split_leaf(focused, panes[1].id, SplitDirection::LeftRight, 0.5);
                    }
                    workspace.tabs.push(WorkspaceTab::Terminal {
                        panes,
                        tree,
                        focused,
                        zoomed: false,
                        broadcast: false,
                    });
                }
                workspace
            });
            workspace = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let mut window = window.clone();
        let workspace = workspace.unwrap();
        draw(&mut window);
        let counts: Vec<_> = (0..3).map(|_| Rc::new(Cell::new(0))).collect();
        let _subscriptions: Vec<_> = window.update(|_, cx| {
            terminals
                .iter()
                .zip(&counts)
                .map(|(terminal, count)| {
                    let count = count.clone();
                    cx.observe(terminal, move |_, _| count.set(count.get() + 1))
                })
                .collect()
        });
        let mut check = |active, zoomed, settings_open, hidden, expected: [usize; 3]| {
            for count in &counts {
                count.set(0);
            }
            workspace.update(&mut window, |workspace, cx| {
                workspace.active = active;
                workspace.settings_open = settings_open;
                workspace.window_hidden = hidden;
                if let WorkspaceTab::Terminal { zoomed: value, .. } = &mut workspace.tabs[0] {
                    *value = zoomed;
                }
                cx.notify();
            });
            draw(&mut window);
            assert_eq!(counts.iter().map(|count| count.get()).collect::<Vec<_>>(), expected);
        };
        check(1, false, false, false, [0, 0, 1]);
        check(0, false, false, false, [1, 1, 0]);
        check(0, true, false, false, [0, 0, 0]);
        check(0, false, false, false, [0, 1, 0]);
        check(0, false, true, false, [0, 0, 0]);
        check(0, false, false, false, [1, 1, 0]);
        check(0, false, false, true, [0, 0, 0]);
        check(0, false, false, false, [1, 1, 0]);
    }
}
