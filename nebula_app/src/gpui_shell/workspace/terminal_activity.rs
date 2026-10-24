//! Terminal presentation activity and semantic event routing for the workspace.

use super::*;

/// Chrome animation reuses the terminal's painted subtree until its own state,
/// bounds or inherited text style changes. Both pane layouts use this boundary.
pub(super) fn terminal_surface(pane: &TerminalPane) -> gpui::ViewElement<Entity<TerminalView>> {
    pane.view.clone().cached(gpui::StyleRefinement::default().size_full())
}

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
    use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
    use crate::gpui_shell::terminal::element::snapshot_probe::{self, PaintedSnapshot};
    use crate::gpui_shell::terminal::view::TerminalLaunch;
    use gpui::{Modifiers, TestAppContext, VisualTestContext, point};
    use nebula_terminal::event::Event;
    use nebula_terminal::event_loop::Msg;
    use std::cell::Cell;
    use std::sync::mpsc::Receiver;

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
    }

    fn feed(terminal: &mut TerminalView, bytes: &[u8]) {
        let mut term = terminal.session.as_ref().unwrap().term.lock();
        let mut parser = nebula_terminal::vte::ansi::Processor::<
            nebula_terminal::vte::ansi::StdSyncHandler,
        >::default();
        parser.advance(&mut *term, bytes);
    }

    fn painted(terminal: &Entity<TerminalView>, cx: &mut VisualTestContext) -> PaintedSnapshot {
        cx.update(|_, cx| snapshot_probe::last(terminal.entity_id(), cx))
    }

    fn open_painted_workspace(
        pane_counts: &[usize],
        cx: &mut TestAppContext,
    ) -> (Entity<NebulaWorkspace>, Vec<Entity<TerminalView>>, Vec<Receiver<Msg>>, VisualTestContext)
    {
        cx.update(|cx| {
            crate::gpui_shell::init(cx, None);
            windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
            cx.set_reduce_motion(true);
            snapshot_probe::enable(cx);
        });
        let mut workspace = None;
        let mut terminals = Vec::new();
        let mut receivers = Vec::new();
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
                workspace.tabs_position = nebula_settings::TabsPositionName::Sidebar;
                for &pane_count in pane_counts {
                    let panes: Vec<_> = (0..pane_count)
                        .map(|_| {
                            let pane = workspace.new_pane(
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
                            );
                            receivers.push(pane.view.update(cx, |view, _| {
                                let receiver = view.install_completion_test_session();
                                feed(view, b"\x1b[2 qcache-output");
                                view.running_program = Some("cache-fixture-command".into());
                                receiver
                            }));
                            pane
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
                let focus = terminals[0].read(cx).focus_handle.clone();
                window.focus(&focus, cx);
                workspace
            });
            workspace = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let mut window = window.clone();
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        window.update(|window, cx| {
            assert!(window.is_window_active());
            assert!(terminals[0].read(cx).focus_handle.is_focused(window));
        });
        window.simulate_resize(size(px(1280.0), px(900.0)));
        draw(&mut window);
        // Let the real startup viewport deadline land before measuring stable frames.
        window.executor().advance_clock(Duration::from_secs(1));
        window.run_until_parked();
        draw(&mut window);
        draw(&mut window);
        for terminal in terminals.iter().take(pane_counts[0]) {
            assert!(painted(terminal, &mut window).text.contains("cache-output"));
        }
        (workspace.unwrap(), terminals, receivers, window)
    }

    fn assert_spinner_reuses_terminal_paint(
        workspace: &Entity<NebulaWorkspace>,
        terminals: &[Entity<TerminalView>],
        cx: &mut VisualTestContext,
    ) {
        let before: Vec<_> = terminals.iter().map(|terminal| painted(terminal, cx).count).collect();
        let phase = workspace.read_with(cx, |workspace, _| {
            assert!(workspace.spinner_visible.get(), "a real running badge must be painted");
            workspace.spinner_phase
        });
        for _ in 0..8 {
            cx.executor().advance_clock(Duration::from_millis(16));
            cx.update(|window, cx| {
                assert!(window.simulate_next_frame(cx) > 0);
            });
            draw(cx);
        }
        assert_ne!(workspace.read_with(cx, |workspace, _| workspace.spinner_phase), phase);
        let after: Vec<_> = terminals.iter().map(|terminal| painted(terminal, cx).count).collect();
        assert_eq!(after, before, "spinner frames must reuse actual terminal paint");
    }

    fn spinner_tick(cx: &mut VisualTestContext) -> usize {
        cx.executor().advance_clock(Duration::from_millis(16));
        let callbacks = cx.update(|window, cx| window.simulate_next_frame(cx));
        draw(cx);
        callbacks
    }

    #[track_caller]
    fn assert_spinner_running(workspace: &Entity<NebulaWorkspace>, cx: &mut VisualTestContext) {
        let phase = workspace.read_with(cx, |workspace, _| {
            assert!(workspace.spinner_visible.get());
            workspace.spinner_phase
        });
        assert_eq!(spinner_tick(cx), 1, "visible badges share one workspace frame callback");
        assert_ne!(workspace.read_with(cx, |workspace, _| workspace.spinner_phase), phase);
        assert_eq!(spinner_tick(cx), 1, "actual paint must renew frame demand");
    }

    #[track_caller]
    fn assert_spinner_stopped(workspace: &Entity<NebulaWorkspace>, cx: &mut VisualTestContext) {
        let phase = workspace.read_with(cx, |workspace, _| {
            assert!(!workspace.spinner_visible.get());
            workspace.spinner_phase
        });
        // The last visible paint may already have queued one callback. It must
        // expire without advancing phase or renewing itself after the hide.
        assert!(spinner_tick(cx) <= 1);
        assert_eq!(workspace.read_with(cx, |workspace, _| workspace.spinner_phase), phase);
        assert_eq!(spinner_tick(cx), 0, "hidden badges must leave no frame demand");
    }

    fn click_tabs_toggle(cx: &mut VisualTestContext) {
        let position = cx.debug_bounds("sidebar-tabs-toggle").unwrap().center();
        cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
        draw(cx);
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        draw(cx);
    }

    fn scroll_top_tabs(
        position: gpui::Point<Pixels>,
        delta: gpui::Point<Pixels>,
        cx: &mut VisualTestContext,
    ) {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(delta),
            touch_phase: gpui::TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        cx.simulate_mouse_move(point(px(1200.0), px(850.0)), None, Modifiers::default());
        draw(cx);
    }

    #[gpui::test]
    fn spinner_frames_reuse_single_terminal_snapshot(cx: &mut TestAppContext) {
        let (workspace, terminals, _receivers, mut window) = open_painted_workspace(&[1], cx);
        assert_spinner_reuses_terminal_paint(&workspace, &terminals, &mut window);
    }

    #[gpui::test]
    fn sidebar_spinner_follows_real_collapse_and_window_focus(cx: &mut TestAppContext) {
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        // A persisted top layout can queue an extra frame during startup.
        // Pin the fixture's sidebar baseline before any window callbacks run.
        nebula_settings::persist_keys(&[("tabs_position", "sidebar".into())]).unwrap();
        let (workspace, _terminals, _receivers, mut window) = open_painted_workspace(&[1], cx);
        assert_spinner_running(&workspace, &mut window);
        let row = window.debug_bounds("sidebar-tab-0").unwrap();
        window.simulate_mouse_move(row.center(), None, Modifiers::default());
        draw(&mut window);
        assert_spinner_stopped(&workspace, &mut window);
        window.simulate_mouse_move(point(px(800.0), px(600.0)), None, Modifiers::default());
        draw(&mut window);
        assert_spinner_running(&workspace, &mut window);

        click_tabs_toggle(&mut window);
        assert!(workspace.read_with(&window, |workspace, _| workspace.tabs_section_collapsed));
        assert_spinner_stopped(&workspace, &mut window);
        click_tabs_toggle(&mut window);
        assert!(!workspace.read_with(&window, |workspace, _| workspace.tabs_section_collapsed));
        assert_spinner_running(&workspace, &mut window);

        window.dispatch_action(ToggleSidebar);
        draw(&mut window);
        assert!(workspace.read_with(&window, |workspace, _| workspace.sidebar_collapsed));
        assert_spinner_stopped(&workspace, &mut window);
        window.dispatch_action(ToggleSidebar);
        draw(&mut window);
        assert_spinner_running(&workspace, &mut window);

        window.update(|window, _| assert!(window.is_window_active()));
        window.deactivate_window();
        draw(&mut window);
        assert!(!workspace.read_with(&window, |workspace, _| workspace.spinner_window_active));
        assert_spinner_running(&workspace, &mut window);
    }

    #[gpui::test]
    fn top_tab_spinners_share_frames_and_stop_when_running_tab_scrolls_out(
        cx: &mut TestAppContext,
    ) {
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        nebula_settings::persist_keys(&[("tabs_position", "sidebar".into())]).unwrap();
        let (workspace, terminals, _receivers, mut window) = open_painted_workspace(&[1; 10], cx);
        for terminal in &terminals {
            terminal.update(&mut window, |view, cx| {
                view.running_program = None;
                cx.notify();
            });
        }
        draw(&mut window);
        assert_spinner_stopped(&workspace, &mut window);
        workspace.update(&mut window, |workspace, cx| {
            workspace.tabs_position = nebula_settings::TabsPositionName::Top;
            cx.notify();
        });
        draw(&mut window);
        draw(&mut window);
        // GPUI's top-menu Popover requests one frame after first capturing its
        // trigger bounds. Isolate that mount callback with every tab idle.
        assert_eq!(spinner_tick(&mut window), 1, "popover captures its initial bounds once");
        assert_eq!(spinner_tick(&mut window), 0, "settled idle top tabs request no frames");
        for terminal in &terminals {
            terminal.update(&mut window, |view, cx| {
                view.running_program = Some("cache-fixture-command".into());
                cx.notify();
            });
        }
        draw(&mut window);
        assert_spinner_running(&workspace, &mut window);
        for terminal in terminals.iter().skip(1) {
            terminal.update(&mut window, |view, cx| {
                view.running_program = None;
                cx.notify();
            });
        }
        draw(&mut window);
        assert_spinner_running(&workspace, &mut window);
        let first = window.debug_bounds("top-tab-0").unwrap();
        window.simulate_mouse_move(first.center(), None, Modifiers::default());
        draw(&mut window);
        assert_spinner_stopped(&workspace, &mut window);
        window.simulate_mouse_move(point(px(1200.0), px(850.0)), None, Modifiers::default());
        draw(&mut window);
        assert_spinner_running(&workspace, &mut window);

        // Wheel over a real tab, including vertical mouse-wheel input. Each
        // event is applied once by GPUI, without another workspace delta.
        for delta in [point(px(-40.0), px(0.0)), point(px(0.0), px(-40.0))] {
            scroll_top_tabs(first.center(), delta, &mut window);
            assert_eq!(
                workspace.read_with(&window, |workspace, _| workspace.top_tabs_scroll.offset().x),
                px(-40.0),
                "one wheel event must apply exactly one horizontal or vertical delta"
            );
            scroll_top_tabs(first.center(), -delta, &mut window);
            assert_eq!(
                workspace.read_with(&window, |workspace, _| workspace.top_tabs_scroll.offset().x),
                px(0.0)
            );
        }
        assert_spinner_running(&workspace, &mut window);
        scroll_top_tabs(first.center(), point(px(-3000.0), px(0.0)), &mut window);
        assert!(workspace.read_with(&window, |workspace, _| {
            workspace.top_tabs_scroll.offset().x < -first.size.width
        }));
        assert_spinner_stopped(&workspace, &mut window);
        let last = window.debug_bounds("top-tab-9").unwrap();
        scroll_top_tabs(last.center(), point(px(3000.0), px(0.0)), &mut window);
        assert_eq!(
            workspace.read_with(&window, |workspace, _| workspace.top_tabs_scroll.offset().x),
            px(0.0)
        );
        assert_spinner_running(&workspace, &mut window);

        // Keyboard activation must still reveal the actual indexed child in
        // ScrollHandle, including the last tab followed by the first tab.
        window.simulate_keystrokes("ctrl-shift-tab");
        draw(&mut window);
        workspace.read_with(&window, |workspace, _| {
            assert_eq!(workspace.active, 9);
            assert!(workspace.top_tabs_scroll.offset().x < px(0.0));
        });
        let last = window.debug_bounds("top-tab-9").unwrap();
        let viewport =
            workspace.read_with(&window, |workspace, _| workspace.top_tabs_scroll.bounds());
        assert_eq!(last.intersect(&viewport), last, "activated last tab must be fully visible");
        assert_spinner_stopped(&workspace, &mut window);
        window.simulate_keystrokes("ctrl-tab");
        draw(&mut window);
        workspace.read_with(&window, |workspace, _| {
            assert_eq!(workspace.active, 0);
            assert_eq!(workspace.top_tabs_scroll.offset().x, px(0.0));
        });
        let first = window.debug_bounds("top-tab-0").unwrap();
        assert_eq!(first.intersect(&viewport), first, "activated first tab must be fully visible");
        assert_spinner_running(&workspace, &mut window);
    }

    #[gpui::test]
    fn spinner_frames_reuse_split_and_zoomed_terminal_snapshots(cx: &mut TestAppContext) {
        let (workspace, terminals, _receivers, mut window) = open_painted_workspace(&[2], cx);
        assert_spinner_reuses_terminal_paint(&workspace, &terminals, &mut window);
        let split_grid = painted(&terminals[0], &mut window).grid;
        workspace.update(&mut window, |workspace, cx| {
            let WorkspaceTab::Terminal { zoomed, .. } = &mut workspace.tabs[0] else {
                unreachable!();
            };
            *zoomed = true;
            cx.notify();
        });
        draw(&mut window);
        assert!(painted(&terminals[0], &mut window).grid.1 > split_grid.1);
        assert_spinner_reuses_terminal_paint(&workspace, &terminals, &mut window);
    }

    #[gpui::test]
    fn cached_terminal_updates_output_resize_cursor_input_and_selection(cx: &mut TestAppContext) {
        let (_, terminals, receivers, mut window) = open_painted_workspace(&[1], cx);
        let terminal = &terminals[0];
        let initial = painted(terminal, &mut window);
        terminal.update(&mut window, |view, cx| {
            feed(view, b"\r\x1b[2Kcache-updated");
            view.remote_files_test_event(Event::Wakeup, cx);
        });
        draw(&mut window);
        let output = painted(terminal, &mut window);
        assert!(output.count > initial.count);
        assert!(output.text.contains("cache-updated"));
        assert_eq!(output.cursor, Some((0, 13)));

        window.simulate_resize(size(px(1600.0), px(1000.0)));
        draw(&mut window);
        let resized = painted(terminal, &mut window);
        assert!(resized.count > output.count);
        assert!(resized.grid.0 > output.grid.0 && resized.grid.1 > output.grid.1);
        assert!(resized.text.contains("cache-updated"));

        terminal.update(&mut window, |view, cx| {
            feed(view, b"\x1b[1;3H\x1b[1 q");
            view.remote_files_test_event(Event::CursorBlinkingChange, cx);
        });
        draw(&mut window);
        let moved = painted(terminal, &mut window);
        assert!(moved.count > resized.count);
        assert_eq!(moved.cursor, Some((0, 2)));
        assert!(moved.cursor_visible);
        window.executor().advance_clock(Duration::from_millis(750));
        window.run_until_parked();
        draw(&mut window);
        let blinked = painted(terminal, &mut window);
        assert!(blinked.count > moved.count);
        assert!(!blinked.cursor_visible);

        // Dispatch through the cached subtree's real focus/key listeners.
        window.simulate_keystrokes("ctrl-a");
        draw(&mut window);
        let input: Vec<_> = receivers[0]
            .try_iter()
            .filter_map(|message| match message {
                Msg::Input(bytes) => Some(bytes.into_owned()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(input, b"\x01");
        let typed = painted(terminal, &mut window);
        assert!(typed.count > blinked.count);
        assert!(typed.cursor_visible, "input must restart the cursor blink phase");

        let start =
            typed.origin + point(typed.cell_size.width * 0.25, typed.cell_size.height * 0.5);
        let end = typed.origin + point(typed.cell_size.width * 4.75, typed.cell_size.height * 0.5);
        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        draw(&mut window);
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        draw(&mut window);
        let selected = painted(terminal, &mut window);
        assert!(selected.count > typed.count);
        assert_eq!(selected.selection, [(0, 0, 5)]);
        assert_eq!(
            terminal.read_with(&window, |view, _| {
                view.session.as_ref().unwrap().term.lock().selection_to_string()
            }),
            Some("cache".into())
        );
    }

    #[gpui::test]
    fn rendered_tabs_splits_zoom_and_settings_restore_only_presented_panes(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::gpui_shell::init(cx, None);
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
