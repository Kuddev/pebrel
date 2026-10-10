use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use crate::gpui_shell::workspace::tab_drag::{TabDrag, TabDragAxis};
use gpui::TestAppContext;

fn initialize_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        initialize(cx, crate::runtime_api::RuntimeHub::new());
    });
}

fn open_test_window(cx: &mut App, count: usize) -> (u64, Entity<NebulaWorkspace>) {
    let (id, workspace) =
        open_workspace_window(cx, WorkspaceStartup::Empty, None, None, false, WindowRole::Regular)
            .unwrap();
    let entry = entry_by_id(id, cx).unwrap();
    entry
        .handle
        .update(cx, |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                for index in 0..count {
                    let pane = workspace.new_pane(
                        (80, 24),
                        TerminalLaunch::Local {
                            cwd: Some(PathBuf::from(format!("test-project-{index}"))),
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
                    workspace.insert_tab_at(
                        index,
                        WorkspaceTab::Terminal {
                            tree: SplitTree::leaf(pane.id),
                            focused: pane.id,
                            panes: vec![pane],
                            zoomed: false,
                            broadcast: false,
                        },
                        TabMeta::default(),
                    );
                }
            });
        })
        .unwrap();
    (id, workspace)
}

#[gpui::test]
fn moving_last_tab_closes_only_source_after_transfer(cx: &mut TestAppContext) {
    initialize_test(cx);
    let (source_id, source, other_id, other, moved_view) = cx.update(|cx| {
        let (source_id, source) = open_test_window(cx, 1);
        let (other_id, other) = open_test_window(cx, 1);
        let moved_view = source.read(cx).tabs[0].focused_view().unwrap().clone();
        source.update(cx, |source, cx| source.schedule_move_tab_to_new_window(0, cx));
        (source_id, source, other_id, other, moved_view)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_none());
        assert!(source.read(cx).tabs.is_empty());
        assert_eq!(other.read(cx).tabs.len(), 1);
        assert!(entry_by_id(other_id, cx).is_some());
        let entries = &cx.global::<WindowRegistry>().entries;
        assert_eq!(entries.len(), 2);
        let target = entries
            .iter()
            .find(|entry| entry.runtime_window_id != other_id)
            .unwrap()
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}

#[gpui::test]
fn moving_one_of_two_tabs_preserves_source_and_identity(cx: &mut TestAppContext) {
    initialize_test(cx);
    let (source_id, source, moved_view) = cx.update(|cx| {
        let (id, source) = open_test_window(cx, 2);
        let moved_view = source.read(cx).tabs[1].focused_view().unwrap().clone();
        source.update(cx, |source, cx| source.schedule_move_tab_to_new_window(1, cx));
        (id, source, moved_view)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_some());
        assert_eq!(source.read(cx).tabs.len(), 1);
        let target = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id != source_id)
            .unwrap()
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}

#[gpui::test]
fn alt_release_tears_out_tab_from_within_the_window(cx: &mut TestAppContext) {
    // 最大化/铺满单屏时光标无法移出 viewport，Alt 强制撕出必须在窗口内部的
    // 松手位置也能把标签撕成独立新窗口——这正是 issue #572 的核心缺陷。
    // 本用例复刻真实侧栏手势：mouse-down 冻结真实跨窗 payload，带 Alt 的
    // move 激活；松手时 Alt 已松开，仅靠拖拽期间锁存的撕出意图完成撕出。
    initialize_test(cx);
    let (source_id, source, moved_view) = cx.update(|cx| {
        let (id, source) = open_test_window(cx, 2);
        let moved_view = source.read(cx).tabs[1].focused_view().unwrap().clone();
        let entry = entry_by_id(id, cx).unwrap();
        entry
            .handle
            .update(cx, |_, window, cx| {
                source.update(cx, |source, cx| {
                    let payload = source.cross_window_drag_payload(1, cx);
                    assert!(payload.is_some(), "terminal tab must freeze a cross-window payload");
                    source.tab_drag = Some(TabDrag {
                        source: 1,
                        cross_window: payload,
                        cross_window_target: None,
                        press_x: 100.0,
                        press_y: 100.0,
                        axis: TabDragAxis::Vertical,
                        pitch: 36.0,
                        offset: 0.0,
                        active: false,
                        force_tear_out: false,
                        dock: None,
                    });
                    // 按住 Alt 拖过 4px 阈值：这一步把撕出意图锁存下来。
                    let alt = gpui::Modifiers { alt: true, ..Default::default() };
                    source.update_tab_drag(
                        &gpui::MouseMoveEvent {
                            position: gpui::Point::new(gpui::px(100.0), gpui::px(120.0)),
                            pressed_button: Some(gpui::MouseButton::Left),
                            modifiers: alt,
                        },
                        window,
                        cx,
                    );
                    assert!(source.tab_drag.as_ref().is_some_and(|drag| drag.active));
                    // 松手位置在窗口内部（非 viewport 之外）且 Alt 已经松开，
                    // 只有锁存的撕出意图能触发。
                    let inside = gpui::Point::new(gpui::px(100.0), gpui::px(60.0));
                    assert!(source.release_tab_drag_at(inside, false, window, cx));
                });
            })
            .unwrap();
        (id, source, moved_view)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_some());
        assert_eq!(source.read(cx).tabs.len(), 1);
        let target = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id != source_id)
            .unwrap()
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}

#[gpui::test]
fn nc_move_without_pressed_button_keeps_active_alt_drag_alive(cx: &mut TestAppContext) {
    // 真机 #572：拖拽掠过窗口非客户区（边框/标题栏条带）时，gpui_windows 把
    // WM_NCMOUSEMOVE 合成为 pressed_button=None 的 MouseMoveEvent。已激活的
    // 拖拽必须无视它继续存活，否则锁存的撕出意图在松手前就被静默清掉。
    initialize_test(cx);
    let (source_id, source, moved_view) = cx.update(|cx| {
        let (id, source) = open_test_window(cx, 2);
        let moved_view = source.read(cx).tabs[1].focused_view().unwrap().clone();
        let entry = entry_by_id(id, cx).unwrap();
        entry
            .handle
            .update(cx, |_, window, cx| {
                source.update(cx, |source, cx| {
                    let payload = source.cross_window_drag_payload(1, cx);
                    assert!(payload.is_some(), "terminal tab must freeze a cross-window payload");
                    source.tab_drag = Some(TabDrag {
                        source: 1,
                        cross_window: payload,
                        cross_window_target: None,
                        press_x: 100.0,
                        press_y: 100.0,
                        axis: TabDragAxis::Vertical,
                        pitch: 36.0,
                        offset: 0.0,
                        active: false,
                        force_tear_out: false,
                        dock: None,
                    });
                    let alt = gpui::Modifiers { alt: true, ..Default::default() };
                    source.update_tab_drag(
                        &gpui::MouseMoveEvent {
                            position: gpui::Point::new(gpui::px(100.0), gpui::px(120.0)),
                            pressed_button: Some(gpui::MouseButton::Left),
                            modifiers: alt,
                        },
                        window,
                        cx,
                    );
                    // 非客户区移动形状：不带按键的 move 不得杀掉已激活的拖拽。
                    source.update_tab_drag(
                        &gpui::MouseMoveEvent {
                            position: gpui::Point::new(gpui::px(100.0), gpui::px(128.0)),
                            pressed_button: None,
                            modifiers: alt,
                        },
                        window,
                        cx,
                    );
                    let drag = source.tab_drag.as_ref().expect("拖拽状态必须存活");
                    assert!(drag.active, "非客户区移动不得把已激活拖拽打回未激活");
                    assert!(drag.force_tear_out, "锁存的撕出意图必须保留");
                    // 窗口内部松手且 Alt 已松开：只靠锁存完成撕出。
                    let inside = gpui::Point::new(gpui::px(100.0), gpui::px(60.0));
                    assert!(source.release_tab_drag_at(inside, false, window, cx));
                });
            })
            .unwrap();
        (id, source, moved_view)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_some());
        assert_eq!(source.read(cx).tabs.len(), 1);
        let target = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id != source_id)
            .unwrap()
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}

#[gpui::test]
fn nc_move_still_clears_pending_drag_without_committing(cx: &mut TestAppContext) {
    // 未激活的待命拖拽遇到非客户区移动仍然清理（防止 up 丢失后残留待命状态），
    // 且不得提交任何重排、不得开新窗口。
    initialize_test(cx);
    let (source_id, source) = cx.update(|cx| {
        let (id, source) = open_test_window(cx, 2);
        let entry = entry_by_id(id, cx).unwrap();
        entry
            .handle
            .update(cx, |_, window, cx| {
                source.update(cx, |source, cx| {
                    source.tab_drag = Some(TabDrag {
                        source: 1,
                        cross_window: source.cross_window_drag_payload(1, cx),
                        cross_window_target: None,
                        press_x: 100.0,
                        press_y: 100.0,
                        axis: TabDragAxis::Vertical,
                        pitch: 36.0,
                        offset: 0.0,
                        active: false,
                        force_tear_out: false,
                        dock: None,
                    });
                    source.update_tab_drag(
                        &gpui::MouseMoveEvent {
                            position: gpui::Point::new(gpui::px(100.0), gpui::px(140.0)),
                            pressed_button: None,
                            modifiers: gpui::Modifiers { alt: true, ..Default::default() },
                        },
                        window,
                        cx,
                    );
                    assert!(source.tab_drag.is_none(), "待命拖拽遇非客户区移动应被清理");
                });
            })
            .unwrap();
        (id, source)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_some());
        assert_eq!(source.read(cx).tabs.len(), 2, "待命清理不得提交重排");
        assert_eq!(cx.global::<WindowRegistry>().entries.len(), 1, "不得开新窗口");
    });
}

#[gpui::test]
fn alt_drag_survives_nc_move_and_tears_out_via_real_events(cx: &mut TestAppContext) {
    // 整链路复刻（真实派发，含 GPUI 原生 on_drag 机制）：按住 Alt 从侧栏标签行
    // 拖出，中途插入一个 WM_NCMOUSEMOVE 形状的 None move；修复前该事件会在
    // 激活后静默清掉手势，松手不产生新窗口，这正是 #572 真机 trace 的形状。
    initialize_test(cx);
    let (source_id, source) = cx.update(|cx| open_test_window(cx, 2));
    let handle = cx.update(|cx| entry_by_id(source_id, cx).unwrap().handle);
    let moved_view = cx.update(|cx| source.read(cx).tabs[1].focused_view().unwrap().clone());
    let mut visual = gpui::VisualTestContext::from_window(handle, cx);
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let row = visual.debug_bounds("sidebar-tab-1").expect("sidebar tab row must render");
    let start = row.center();
    let at = |dy: f32| gpui::Point::new(start.x, start.y + gpui::px(dy));
    let alt = gpui::Modifiers { alt: true, ..Default::default() };
    let plain = gpui::Modifiers::default();
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, alt);
    // 越过 GPUI 原生拖拽阈值（2px）。
    visual.simulate_mouse_move(at(3.0), Some(gpui::MouseButton::Left), alt);
    // 越过我们 4px 激活阈值并锁存 Alt 撕出意图。
    visual.simulate_mouse_move(at(10.0), Some(gpui::MouseButton::Left), alt);
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    // 非客户区移动形状：不带按键的 move——修复前在这里静默清掉手势。
    visual.simulate_mouse_move(at(14.0), None::<gpui::MouseButton>, alt);
    visual.simulate_mouse_move(at(20.0), Some(gpui::MouseButton::Left), alt);
    // 窗口内部松手且 Alt 已松开：只有锁存的意图能撕出。
    visual.simulate_mouse_up(at(24.0), gpui::MouseButton::Left, plain);
    drop(visual);
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_some());
        assert_eq!(source.read(cx).tabs.len(), 1);
        let target = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id != source_id)
            .expect("撕出应创建接收窗口")
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}

#[gpui::test]
fn release_outside_source_window_tears_out_without_alt(cx: &mut TestAppContext) {
    // #572 合并堵点的最小复刻：拖出源窗口后在窗口外松手，事件坐标在 viewport
    // 之外，所有带 hover 门控的监听器（根 capture、罩层 on_mouse_up、侧栏
    // on_mouse_move）都不触发，释放被静默吞掉——真机 trace 里「activated 后
    // 无任何 release 行」即此形状。修复后由 paint 期注册的窗口级裸监听兜底，
    // 走 release_tab_drag_at 的 outside 分支撕出新窗口（原「拖出窗口外」语义）。
    // 全程不用 Alt，证明这条不是靠锁存意图走通的。
    initialize_test(cx);
    let (source_id, source) = cx.update(|cx| open_test_window(cx, 2));
    let handle = cx.update(|cx| entry_by_id(source_id, cx).unwrap().handle);
    let moved_view = cx.update(|cx| source.read(cx).tabs[1].focused_view().unwrap().clone());
    let mut visual = gpui::VisualTestContext::from_window(handle, cx);
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let row = visual.debug_bounds("sidebar-tab-1").expect("sidebar tab row must render");
    let start = row.center();
    let plain = gpui::Modifiers::default();
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, plain);
    // 显式重绘：让根节点的裸监听（canvas paint 期注册）进入本帧监听表。
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    // 一直拖到窗口外（远超出任何测试窗口尺寸）：门控监听器全部失效。
    let outside = gpui::Point::new(start.x + gpui::px(40000.0), start.y + gpui::px(80.0));
    visual.simulate_mouse_move(outside, Some(gpui::MouseButton::Left), plain);
    visual.simulate_mouse_move(
        gpui::Point::new(outside.x, start.y + gpui::px(120.0)),
        Some(gpui::MouseButton::Left),
        plain,
    );
    visual.simulate_mouse_up(outside, gpui::MouseButton::Left, plain);
    drop(visual);
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(entry_by_id(source_id, cx).is_some());
        assert_eq!(source.read(cx).tabs.len(), 1);
        let target = cx
            .global::<WindowRegistry>()
            .entries
            .iter()
            .find(|entry| entry.runtime_window_id != source_id)
            .expect("窗口外松手应撕出新窗口（裸监听兜底）")
            .workspace
            .upgrade()
            .unwrap();
        assert_eq!(target.read(cx).tabs[0].focused_view().unwrap(), &moved_view);
        assert_eq!(combined_session(None, cx).unwrap().tabs.len(), 2);
    });
}
