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
                ["test-host".into()],
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
