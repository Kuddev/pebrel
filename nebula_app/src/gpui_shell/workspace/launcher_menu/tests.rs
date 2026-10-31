use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn shell(id: &str) -> crate::shell_detect::DetectedShell {
    crate::shell_detect::DetectedShell {
        id: id.into(),
        name: id.into(),
        program: format!("{id}.exe"),
        args: Vec::new(),
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<NebulaWorkspace>, VisualTestContext) {
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        windowing::initialize(cx, hub.clone());
    });
    let mut output = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
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
        view.update(cx, |workspace, cx| {
            workspace.palette_override = Some(shell_palette_rows(
                vec![shell("cmd"), shell("pwsh")],
                vec![],
                [("test-host".into(), String::new())],
                "cmd",
                workspace_ui_language(),
                1.0,
            ));
            workspace.command_palette_open = true;
            workspace.shell_picker_open = true;
            workspace.command_palette_input.read(cx).focus_handle(cx).focus(window, cx);
        });
        output = Some(view.clone());
        Root::new(view, window, cx)
    });
    let mut window = window.clone();
    window.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    draw(&mut window);
    (output.unwrap(), window)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

/// 启动器 SSH 搜索的当前契约（#525 的 tags/notes 管道已随 474c98d7 的
/// 重构移除，本测试只锚定现存行为）：主机地址、起名（label）与通用关键词
/// 都参与匹配；多关键词 AND；查询对 ASCII 大小写不敏感；未命中不残留行。
#[gpui::test]
fn ssh_launcher_search_matches_host_label_and_multiple_keywords(cx: &mut TestAppContext) {
    let (workspace, mut cx) = open(cx);
    // named-host 带起名：label 进搜索串（test-host 保持空名 = 纯地址形态）。
    workspace.update(&mut cx, |workspace, cx| {
        workspace.palette_override = Some(shell_palette_rows(
            vec![shell("cmd"), shell("pwsh")],
            vec![],
            [("test-host".into(), String::new()), ("named-host".into(), "alice 生产".to_owned())],
            "cmd",
            workspace_ui_language(),
            1.0,
        ));
        cx.notify();
    });
    for (query, expected) in [
        ("test-host", vec!["test-host"]),
        ("ALICE", vec!["named-host"]),
        ("生产", vec!["named-host"]),
        ("named-host alice 生产", vec!["named-host"]),
        ("remote", vec!["named-host", "test-host"]),
        ("missing-tag", vec![]),
    ] {
        // 查询替换只依赖输入框的全选动作，不重复判断平台快捷键。
        cx.update(|window, cx| {
            window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
        });
        cx.simulate_input(query);
        draw(&mut cx);
        workspace.read_with(&cx, |workspace, cx| {
            let rows = workspace.filtered_palette_rows(cx);
            let mut hosts: Vec<&str> = rows
                .iter()
                .filter_map(|row| match &row.action {
                    WorkspacePaletteAction::LaunchSshHost(host) => Some(host.as_str()),
                    _ => None,
                })
                .collect();
            hosts.sort_unstable();
            assert_eq!(hosts, expected, "query: {query}");
            assert_eq!(rows.len(), expected.len(), "query: {query}");
            assert!(workspace.tabs.is_empty(), "search must not connect a host");
        });
        assert_eq!(cx.debug_bounds("command-palette-row-0").is_some(), !expected.is_empty());
        assert_eq!(cx.debug_bounds("command-palette-row-1").is_some(), expected.len() > 1);
    }
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    workspace.read_with(&cx, |workspace, _| {
        assert!(!workspace.command_palette_open);
        assert!(workspace.tabs.is_empty());
    });
}

#[gpui::test]
fn launcher_filters_keep_compact_search_spacing_and_follow_real_slots(cx: &mut TestAppContext) {
    let (workspace, mut cx) = open(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    draw(&mut cx);
    let search = cx.debug_bounds("workspace-palette-search").unwrap();
    assert_eq!(search.size.height, px(48.0));
    let all = cx.debug_bounds("workspace-palette-filter-Launcher(All)").unwrap();
    assert!(all.top() - search.bottom() >= px(10.0));
    for (filter, selector) in [
        (
            crate::display::command_palette::LauncherFilter::Ssh,
            "workspace-palette-filter-Launcher(Ssh)",
        ),
        (
            crate::display::command_palette::LauncherFilter::Shell,
            "workspace-palette-filter-Launcher(Shell)",
        ),
        (
            crate::display::command_palette::LauncherFilter::All,
            "workspace-palette-filter-Launcher(All)",
        ),
    ] {
        let slot = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(slot.center(), Modifiers::default());
        draw(&mut cx);
        assert_eq!(
            cx.debug_bounds("launcher-filters-indicator").unwrap(),
            cx.debug_bounds(selector).unwrap()
        );
        workspace.read_with(&cx, |workspace, cx| {
            assert_eq!(workspace.launcher_filter, filter);
            assert_eq!(workspace.command_palette_selected, 0);
            assert!(!workspace.filtered_palette_rows(cx).is_empty());
            assert!(workspace.tabs.is_empty(), "filtering must not launch a shell or connect SSH");
        });
    }
}

fn right_click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("launcher row");
    cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(bounds.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
    assert!(cx.debug_bounds("launcher-context-menu").is_some());
}

#[test]
fn shell_and_ssh_commands_do_not_cross_categories() {
    let ssh = LauncherTarget::Ssh("host".into());
    assert_eq!(
        commands(&ssh),
        [LauncherCommand::Connect, LauncherCommand::Edit, LauncherCommand::Delete]
    );
    assert!(ssh.default_id().is_none() && ssh.local_launch().is_none());
    let local = LauncherTarget::Shell(shell("cmd"));
    assert!(commands(&local).contains(&LauncherCommand::SetDefault));
    assert!(!commands(&local).contains(&LauncherCommand::Delete));
    assert_eq!(
        commands(&local).contains(&LauncherCommand::OpenAdmin),
        crate::platform::elevation::SUPPORTED
    );
}

#[gpui::test]
fn right_click_opens_one_menu_without_launch_and_escape_restores_search(cx: &mut TestAppContext) {
    let (workspace, mut cx) = open(cx);
    for selector in ["command-palette-row-0", "command-palette-row-1", "command-palette-row-2"] {
        right_click(selector, &mut cx);
        workspace.read_with(&cx, |workspace, _| {
            assert!(workspace.tabs.is_empty(), "right click must not create a session");
            assert!(workspace.command_palette_open && workspace.launcher_menu.is_some());
        });
        cx.simulate_keystrokes("escape");
        draw(&mut cx);
        cx.update(|window, cx| {
            let workspace = workspace.read(cx);
            assert!(workspace.launcher_menu.is_none());
            assert!(workspace.command_palette_open, "Escape should only dismiss the context menu");
            assert!(workspace.command_palette_input.read(cx).focus_handle(cx).is_focused(window));
        });
    }
}

#[gpui::test]
fn ssh_menu_keeps_its_target_when_launcher_rows_change(cx: &mut TestAppContext) {
    let (workspace, mut cx) = open(cx);
    right_click("command-palette-row-2", &mut cx);
    workspace.update(&mut cx, |workspace, cx| {
        workspace.palette_override.as_mut().unwrap().reverse();
        cx.notify();
    });
    // No item is initially selected; the second Down selects Edit.
    cx.simulate_keystrokes("down down enter");
    draw(&mut cx);
    workspace.read_with(&cx, |workspace, cx| {
        assert!(workspace.tabs.is_empty());
        assert!(workspace.settings_open);
        let pane = workspace.settings_surface.as_ref().unwrap().0.read(cx);
        assert_eq!(
            pane.ssh_editor.as_ref().unwrap().original_destination.as_deref(),
            Some("test-host")
        );
    });
}

#[gpui::test]
fn ssh_delete_only_arms_the_existing_confirmation(cx: &mut TestAppContext) {
    let (workspace, mut cx) = open(cx);
    right_click("command-palette-row-2", &mut cx);
    cx.simulate_keystrokes("down down down enter");
    draw(&mut cx);
    workspace.read_with(&cx, |workspace, cx| {
        assert!(workspace.settings_open);
        let pane = workspace.settings_surface.as_ref().unwrap().0.read(cx);
        assert_eq!(pane.ssh_delete_confirm.as_deref(), Some("test-host"));
        assert!(pane.ssh_delete_undo.is_none(), "no deletion before the user confirms");
    });
}
