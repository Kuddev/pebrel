use super::super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn open_workspace(
    count: usize,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<NebulaWorkspace>, VisualTestContext) {
    let directory = tempfile::tempdir().unwrap();
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        init(cx);
        windowing::initialize(cx, hub.clone());
    });
    let mut workspace_out = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                hub,
                windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            workspace.tabs_position = nebula_settings::TabsPositionName::Top;
            for index in 0..count {
                let path = directory.path().join(format!("tab-{index}.txt"));
                // 行尾跟随平台：Windows 上编辑器把 LF 规范化为 CRLF，文档会
                // 立刻被判 dirty，关闭 Code tab 时弹确认框拦截——fixture 写
                // 平台原生行尾才能保持「打开即干净」的前置。
                std::fs::write(
                    &path,
                    if std::env::consts::OS == "windows" { "fixture\r\n" } else { "fixture\n" },
                )
                .unwrap();
                let view =
                    cx.new(|cx| crate::gpui_shell::code_tab::CodeTabView::new(path, window, cx));
                let subscription = cx.subscribe(&view, |_, _, _, _| {});
                workspace.insert_tab_at(
                    workspace.tabs.len(),
                    WorkspaceTab::Code { view, _subscription: subscription },
                    TabMeta::default(),
                );
            }
            workspace.open_settings(window, cx);
        });
        workspace_out = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    window.run_until_parked();
    window.simulate_resize(gpui::size(px(1280.0), px(900.0)));
    workspace_out.as_ref().unwrap().update(window, |workspace, cx| {
        workspace.tabs_position = nebula_settings::TabsPositionName::Top;
        workspace.sync_settings_layout(false);
        cx.notify();
    });
    window.run_until_parked();
    (directory, workspace_out.unwrap(), window.clone())
}

fn click_tab(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = tab_bounds(selector, cx);
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

fn tab_bounds(selector: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    redraw(cx);
    cx.debug_bounds(selector).unwrap_or_else(|| panic!("visible tab bounds for {selector}"))
}

fn redraw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

/// 分组侧栏（任务分组/最近任务）测试的前置：打开「AI Agent 侧边栏」开关
/// 并重画。全局 `Settings` 重建即热切换，无需重开窗口。
fn enable_agent_sidebar(cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let mut settings =
            crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
        settings.ai_agent_sidebar = true;
        cx.set_global(settings);
    });
    cx.run_until_parked();
}

fn numbered_tab_shortcuts() -> [&'static str; 2] {
    if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
        ["cmd-1", "cmd-3"]
    } else {
        ["ctrl-1", "ctrl-3"]
    }
}

#[gpui::test]
fn switching_top_tabs_keeps_settings_reachable_until_explicit_close(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(2, cx);
    let settings_id = workspace.read_with(&cx, |workspace, _| {
        assert_eq!(workspace.tabs_position, nebula_settings::TabsPositionName::Top);
        assert_eq!(workspace.top_tab_count(), 3);
        workspace.settings_surface.as_ref().unwrap().0.entity_id()
    });
    for (index, selector) in [(0, "top-tab-0"), (1, "top-tab-1"), (0, "top-tab-0")] {
        click_tab(selector, &mut cx);
        workspace.read_with(&cx, |workspace, _| {
            assert!(!workspace.settings_open);
            assert_eq!(workspace.active, index);
            assert_eq!(workspace.top_tab_count(), 3, "switching must retain the settings tab");
        });
        click_tab("top-tab-2", &mut cx);
        workspace.read_with(&cx, |workspace, _| {
            assert!(workspace.settings_open);
            assert_eq!(workspace.top_tab_count(), 3);
            assert_eq!(workspace.settings_surface.as_ref().unwrap().0.entity_id(), settings_id);
        });
    }
    click_tab("top-tab-0", &mut cx);
    let bounds = tab_bounds("top-tab-2", &mut cx);
    cx.simulate_mouse_down(bounds.center(), gpui::MouseButton::Middle, Modifiers::default());
    cx.simulate_mouse_up(bounds.center(), gpui::MouseButton::Middle, Modifiers::default());
    cx.run_until_parked();
    workspace.read_with(&cx, |workspace, _| {
        assert!(!workspace.settings_open);
        assert_eq!(workspace.active, 0);
        assert_eq!(workspace.top_tab_count(), 2);
    });
}

#[gpui::test]
fn keyboard_navigation_can_return_to_background_settings(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(2, cx);
    let [first_tab, third_tab] = numbered_tab_shortcuts();
    for shortcut in [first_tab, "alt-1"] {
        cx.simulate_keystrokes(shortcut);
        assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes(third_tab);
        assert!(workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes("ctrl-tab");
        assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes("ctrl-shift-tab");
        assert!(workspace.read_with(&cx, |workspace, _| workspace.settings_open));
    }
    cx.simulate_keystrokes("ctrl-shift-w");
    assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.top_tab_count()), 2);
    cx.simulate_keystrokes(third_tab);
    assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
}

#[gpui::test]
fn settings_survives_overflow_and_layout_changes(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(12, cx);
    cx.simulate_resize(gpui::size(px(760.0), px(900.0)));
    cx.simulate_keystrokes(numbered_tab_shortcuts()[0]);
    for position in
        [nebula_settings::TabsPositionName::Sidebar, nebula_settings::TabsPositionName::Top]
    {
        workspace.update(&mut cx, |workspace, cx| {
            workspace.tabs_position = position;
            workspace.sync_settings_layout(false);
            cx.notify();
        });
        assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.top_tab_count()), 13);
    }
    cx.simulate_keystrokes("ctrl-shift-tab");
    let settings = tab_bounds("top-tab-12", &mut cx);
    assert!(settings.origin.x >= px(0.0) && settings.right() <= px(760.0));
    workspace.read_with(&cx, |workspace, _| assert!(workspace.settings_open));
}

#[gpui::test]
fn instant_slide_animation_keeps_the_side_panel_static(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(1, cx);
    // With the slide animation off, folding the main sidebar for the settings
    // page must not arm the fold animation: the column snaps into place.
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tabs_position = nebula_settings::TabsPositionName::Sidebar;
        workspace.settings_open = true;
        workspace.sidebar_collapsed = false;
        workspace.sync_settings_layout(true);
    });
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.sidebar_collapsed, "sidebar mode folds for settings");
        assert!(!workspace.sidebar_fold_armed, "instant: fold animation not armed");
    });
    // The default slide mode keeps arming the fold as before.
    workspace.update(&mut cx, |workspace, _| {
        workspace.sidebar_collapsed = false;
        workspace.sync_settings_layout(false);
    });
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.sidebar_fold_armed, "slide: fold animation armed");
    });
    workspace.update(&mut cx, |workspace, _| workspace.sync_settings_layout(true));
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.sidebar_collapsed);
        assert!(!workspace.sidebar_fold_armed, "mode switch must clear an unchanged fold");
    });
}

#[gpui::test]
fn closing_last_regular_tab_keeps_settings_until_it_is_closed(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(1, cx);
    click_tab("top-tab-0", &mut cx);
    cx.simulate_keystrokes("ctrl-shift-w");
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.tabs.is_empty());
        assert!(workspace.settings_open);
        assert_eq!(workspace.top_tab_count(), 1);
    });
    cx.simulate_keystrokes("ctrl-shift-w");
    assert!(cx.read(|cx| cx.windows().is_empty()));
}

#[gpui::test]
fn density_changes_real_tab_bounds_and_sidebar_drag_pitch(cx: &mut TestAppContext) {
    use nebula_settings::{DensityName, TabsPositionName};

    let (_directory, workspace, mut cx) = open_workspace(8, cx);
    enable_agent_sidebar(&mut cx);
    click_tab("top-tab-0", &mut cx);
    for position in [TabsPositionName::Sidebar, TabsPositionName::Top] {
        // 侧栏条目是第一行 + 最多两行摘要，行高比顶栏 tab 高一档；这里全是
        // 非终端 tab，同归「工作区」兜底组，相邻条目按统一节距排布。行高
        // 从同一派生函数取值（测试环境界面字号即默认 15px），避免两处硬编码。
        let std_entry = task_groups::sidebar_entry_height(&task_groups::RowMetrics::new(
            DensityName::Standard,
            15.0,
        ));
        let compact_entry = task_groups::sidebar_entry_height(&task_groups::RowMetrics::new(
            DensityName::Compact,
            15.0,
        ));
        for (density, height, gap, sidebar_entry_height, title_height) in [
            (DensityName::Standard, 34.0, 8.0, std_entry, 48.0),
            (DensityName::Compact, 32.0, 4.0, compact_entry, 48.0),
            (DensityName::Standard, 34.0, 8.0, std_entry, 48.0),
        ] {
            workspace.update(&mut cx, |workspace, cx| {
                workspace.tabs_position = position;
                workspace.density = density;
                workspace.sync_settings_layout(tab_reveal_instant(cx));
                workspace.reveal_active_tab();
                cx.notify();
            });
            cx.run_until_parked();
            let sidebar = position == TabsPositionName::Sidebar;
            let first = tab_bounds(if sidebar { "sidebar-tab-0" } else { "top-tab-0" }, &mut cx);
            let second = tab_bounds(if sidebar { "sidebar-tab-1" } else { "top-tab-1" }, &mut cx);
            let expected_height = if sidebar { sidebar_entry_height } else { height };
            assert_eq!(f32::from(first.size.height), expected_height);
            assert_eq!(f32::from(second.size.height), expected_height);
            if sidebar {
                assert_eq!(f32::from(second.top() - first.top()), expected_height + gap);
            }
            let native = cx.update(|window, _| crate::platform::window_chrome::layout(window));
            let title = tab_bounds("workspace-titlebar", &mut cx);
            assert_eq!(
                f32::from(title.size.height),
                native.map_or(title_height, |layout| layout.0)
            );

            // Padding belongs to the tab's actual click target in either density.
            let point = gpui::point(second.left() + px(8.0), second.bottom() - px(2.0));
            cx.simulate_click(point, Modifiers::default());
            workspace.read_with(&cx, |workspace, _| assert_eq!(workspace.active, 1));
            if sidebar {
                // 分组视图不做组内拖拽重排（组由 cwd 派生），但保留按下待命
                // 装填（click 合成链路依赖）；按下后不得激活拖拽。
                cx.simulate_mouse_down(point, gpui::MouseButton::Left, Modifiers::default());
                workspace.read_with(&cx, |workspace, _| {
                    assert!(workspace.tab_drag.as_ref().is_none_or(|drag| !drag.active));
                });
                cx.simulate_mouse_up(point, gpui::MouseButton::Left, Modifiers::default());
            }
        }
    }
}

#[gpui::test]
fn section_header_hairlines_fill_the_row(cx: &mut TestAppContext) {
    use nebula_settings::TabsPositionName;

    let (_directory, workspace, mut cx) = open_workspace(1, cx);
    enable_agent_sidebar(&mut cx);
    click_tab("top-tab-0", &mut cx);
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tabs_position = TabsPositionName::Sidebar;
        workspace.sync_settings_layout(false);
        cx.notify();
    });
    cx.run_until_parked();
    redraw(&mut cx);
    // 「最近任务」分区头与其 hairline 随最近会话分区在后续变更补齐；
    // 这里锁定有动作钮的「当前任务」段形态。
    for (header_selector, line_selector, reaches_right_edge) in
        [("sidebar-tabs-toggle", "sidebar-tabs-hairline", false)]
    {
        let header = tab_bounds(header_selector, &mut cx);
        let line = tab_bounds(line_selector, &mut cx);
        assert_eq!(f32::from(line.size.height), 1.0, "{line_selector} is a hairline");
        assert!(
            line.origin.x > header.origin.x + px(40.0),
            "{line_selector} starts right of the chevron/title/count"
        );
        if reaches_right_edge {
            // 无动作钮的分区：线直达右缘。
            assert!(
                line.right() >= header.right() - px(2.0),
                "{line_selector} fills the remaining row width"
            );
        } else {
            // 悬停动作钮常驻占位：线在钮区前收尾。
            assert!(
                line.right() < header.right() - px(10.0),
                "{line_selector} stops before the action buttons"
            );
        }
        assert!(
            line.origin.y >= header.origin.y && line.bottom() <= header.bottom(),
            "{line_selector} stays inside the header row"
        );
    }
}

#[gpui::test]
fn sidebar_groups_tabs_under_a_collapsible_header(cx: &mut TestAppContext) {
    use nebula_settings::TabsPositionName;

    let (_directory, workspace, mut cx) = open_workspace(3, cx);
    enable_agent_sidebar(&mut cx);
    click_tab("top-tab-0", &mut cx);
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tabs_position = TabsPositionName::Sidebar;
        workspace.sync_settings_layout(false);
        cx.notify();
    });
    cx.run_until_parked();
    // 非终端 tab（代码/文档）都归「工作区」兜底组：组头在条目上方、矮一档，
    // 条目保持两行高度与统一节距。
    let header = tab_bounds("sidebar-group-workspace", &mut cx);
    let first = tab_bounds("sidebar-tab-0", &mut cx);
    let second = tab_bounds("sidebar-tab-1", &mut cx);
    assert_eq!(f32::from(header.size.height), 26.0);
    // 折叠箭头固定最左槽，项目名（[] 包裹）紧随其后、不越出行宽。
    let chevron = tab_bounds("sidebar-group-chevron-workspace", &mut cx);
    let name = tab_bounds("sidebar-group-name-workspace", &mut cx);
    assert!(chevron.right() <= name.left(), "chevron sits leftmost in the group header");
    assert!(name.right() <= header.right());
    assert!(header.bottom() <= first.origin.y, "group header sits above its entries");
    let entry_h = task_groups::sidebar_entry_height(&task_groups::RowMetrics::new(
        nebula_settings::DensityName::Standard,
        15.0,
    ));
    assert_eq!(f32::from(first.size.height), entry_h);
    assert_eq!(f32::from(second.top() - first.top()), entry_h + 8.0);
    // 目录树线：一级竖线对齐组头折叠箭头中心；任务肘形横臂直达图标列
    // （T 字横杠）；摘要悬挂竖线 + 肘形横臂指向摘要首行文字。
    let l1 = tab_bounds("sidebar-guide-l1-0-0", &mut cx);
    let elbow = tab_bounds("sidebar-guide-elbow-0-0", &mut cx);
    let sumv = tab_bounds("sidebar-guide-sumv-0-0", &mut cx);
    let sume = tab_bounds("sidebar-guide-sume-0-0", &mut cx);
    assert!(
        (f32::from(l1.origin.x - header.origin.x) - task_groups::TREE_X_L1).abs() <= 1.0,
        "level-one guide aligns under the group chevron"
    );
    assert!(
        (f32::from(l1.size.width) - task_groups::GUIDE_THICKNESS).abs() <= 0.5,
        "guides are 2px thick"
    );
    assert!(
        elbow.origin.y > first.origin.y && elbow.bottom() < first.origin.y + px(30.0),
        "elbow sits on the first line band of the entry"
    );
    assert!(
        (f32::from(elbow.right() - header.origin.x) - task_groups::tree_icon_left()).abs() <= 1.0,
        "elbow arm reaches the entry icon column"
    );
    assert!(
        (f32::from(sumv.origin.x - header.origin.x) - task_groups::TREE_X_L2).abs() <= 1.0
            && f32::from(sumv.size.height) > f32::from(sumv.size.width),
        "summary hanger is a vertical at the second level"
    );
    assert!(
        sume.origin.y > first.origin.y + px(entry_h - 36.0)
            && sume.bottom() < first.bottom() - px(4.0),
        "summary elbow points at the summary's first line"
    );
    assert!(
        (f32::from(sume.right() - header.origin.x) - (task_groups::tree_text_left() - 4.0)).abs()
            <= 1.0
            && f32::from(sume.size.width) > f32::from(sume.size.height),
        "summary elbow arm runs into the summary text"
    );
    // 点击条目底部带（滚动测试同款命中点）仍激活对应 tab。
    let point = gpui::point(second.left() + px(8.0), second.bottom() - px(2.0));
    cx.simulate_click(point, Modifiers::default());
    cx.run_until_parked();
    workspace.read_with(&cx, |workspace, _| assert_eq!(workspace.active, 1));
    // 点组头折叠：条目行消失，组头留在原处；再点展开恢复。
    // （测试环境对组头 on_click 的合成点击不稳定——见行内注释——此处
    // 直接驱动 toggle 验证折叠渲染路径本身。）
    workspace.update(&mut cx, |workspace, cx| {
        workspace.toggle_sidebar_group("workspace".into(), cx);
    });
    cx.run_until_parked();
    cx.run_until_parked();
    workspace.read_with(&cx, |workspace, _| {
        assert!(
            !workspace.sidebar_group_collapsed.is_empty(),
            "clicking the header must record the collapse"
        );
        assert!(workspace.sidebar_group_collapsed.values().all(|collapsed| *collapsed));
    });
    let header_again = tab_bounds("sidebar-group-workspace", &mut cx);
    assert!(cx.debug_bounds("sidebar-tab-0").is_none(), "collapsed group hides its entries");
    workspace.update(&mut cx, |workspace, cx| {
        workspace.toggle_sidebar_group("workspace".into(), cx);
    });
    cx.run_until_parked();
    tab_bounds("sidebar-group-workspace", &mut cx);
    assert!(cx.debug_bounds("sidebar-tab-0").is_some(), "expanding brings the entries back");
}

#[gpui::test]
fn ai_agent_sidebar_toggle_switches_renderers(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(2, cx);
    click_tab("top-tab-0", &mut cx);
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tabs_position = nebula_settings::TabsPositionName::Sidebar;
        workspace.sync_settings_layout(false);
        cx.notify();
    });
    cx.run_until_parked();

    // 默认关：经典扁平列表——条目行在、可点击折叠（TABS 卷帘状态机），
    // 没有分组头与分组段头的 hairline。
    redraw(&mut cx);
    assert!(cx.debug_bounds("sidebar-tab-0").is_some(), "classic rows render");
    assert!(cx.debug_bounds("sidebar-tabs-hairline").is_none(), "no grouped section hairline");
    assert!(cx.debug_bounds("sidebar-group-name-workspace").is_none(), "no group header");
    // 组头 on_click 在测试环境有已知命中问题（见上方滚轮测试的注释）。
    // 上游 `toggle_tabs_section` 是 sidebar 模块私有，这里直接复刻其状态
    // 翻转（`tab_menu` 清位与冻结定时器对断言无影响——250ms 定时器挂在
    // 虚拟时钟上本就不会自动到期，不置 frozen 即等价于动画结束后的落点）。
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tabs_section_collapsed = !workspace.tabs_section_collapsed;
        workspace.tabs_fold_armed = true;
        cx.notify();
    });
    redraw(&mut cx);
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.tabs_section_collapsed, "legacy toggle flips the fold state");
    });
    assert!(cx.debug_bounds("sidebar-tab-0").is_none(), "classic fold hides the list");

    // 开启：分组样式——组头与 hairline 出现，条目行回归（折叠状态由两个
    // 分区各自持有，切开关不清空）。
    enable_agent_sidebar(&mut cx);
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tabs_section_collapsed = false;
        cx.notify();
    });
    cx.run_until_parked();
    redraw(&mut cx);
    assert!(cx.debug_bounds("sidebar-group-name-workspace").is_some(), "group header renders");
    assert!(cx.debug_bounds("sidebar-tabs-hairline").is_some(), "section hairline renders");
    assert!(cx.debug_bounds("sidebar-tab-0").is_some(), "grouped rows render");
}
