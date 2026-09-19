use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn open_manager(
    saved: crate::saved_commands::SavedCommands,
    cx: &mut TestAppContext,
) -> (Entity<NebulaWorkspace>, VisualTestContext) {
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        windowing::initialize(cx, hub.clone());
    });
    let mut workspace = None;
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
        view.update(cx, |this, cx| {
            this.saved_commands = saved;
            this.toggle_command_manager(window, cx);
        });
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    let workspace = workspace.unwrap();
    let mut cx = window.clone();
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    draw(&mut cx);
    (workspace, cx)
}

#[gpui::test]
fn grouping_drag_and_context_menu_keep_commands_and_keyboard_order(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved_commands.json");
    let mut saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    let command = saved.insert("Build", "cargo build", false).unwrap();
    saved.create_group("Work").unwrap();
    let group = saved.groups()[0].id.clone();
    let (workspace, mut cx) = open_manager(saved, cx);
    let row = cx.debug_bounds("saved-command-row-0").unwrap();
    let target =
        cx.debug_bounds(Box::leak(format!("command-group-{group}").into_boxed_str())).unwrap();
    let start = gpui::point(row.origin.x + px(125.0), row.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        start + gpui::point(px(12.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(&mut cx);
    cx.simulate_mouse_move(target.center(), Some(MouseButton::Left), Modifiers::default());
    draw(&mut cx);
    cx.simulate_mouse_up(target.center(), MouseButton::Left, Modifiers::default());
    draw(&mut cx);
    let reloaded = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    assert_eq!(reloaded.group_for(&command.id), Some(group.as_str()));
    workspace.read_with(&cx, |this, cx| {
        assert!(this.command_manager_open && this.tabs.is_empty());
        assert_eq!(this.filtered_saved_commands(cx)[0].id, command.id);
        assert_eq!(this.command_scroll_index(&this.filtered_saved_commands(cx), cx), 2);
    });
    let row = cx.debug_bounds("saved-command-row-0").unwrap();
    cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
    draw(&mut cx);
    workspace.read_with(&cx, |this, _| assert!(this.command_group_menu.is_some()));
    // First menu action is Remove from group. Selecting it must not execute the command.
    cx.simulate_keystrokes("down enter");
    draw(&mut cx);
    assert_eq!(
        crate::saved_commands::SavedCommands::load_from(&path).unwrap().group_for(&command.id),
        None
    );
    workspace.read_with(&cx, |this, cx| {
        assert!(this.command_manager_open && this.tabs.is_empty());
        assert_eq!(this.command_scroll_index(&this.filtered_saved_commands(cx), cx), 1);
    });
    cx.simulate_keystrokes("down");
    workspace.read_with(&cx, |this, _| assert_eq!(this.command_manager_selected, 1));
}

#[gpui::test]
fn builtin_delete_can_be_cancelled_and_stays_deleted_after_reopening(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved_commands.json");
    let saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    let (workspace, mut cx) = open_manager(saved, cx);
    let deleted_id =
        workspace.read_with(&cx, |this, cx| this.filtered_saved_commands(cx)[0].id.clone());
    assert!(deleted_id.starts_with("builtin:"));
    for confirm in [false, true] {
        let button = cx.debug_bounds("saved-command-delete-0").expect("builtin delete button");
        cx.simulate_click(button.center(), Modifiers::default());
        draw(&mut cx);
        let action =
            if confirm { "saved-command-delete-confirm" } else { "saved-command-delete-cancel" };
        let button = cx.debug_bounds(action).expect("delete dialog action");
        cx.simulate_click(button.center(), Modifiers::default());
        draw(&mut cx);
        workspace.read_with(&cx, |this, cx| {
            assert!(this.command_manager_open && this.tabs.is_empty());
            assert_eq!(
                this.filtered_saved_commands(cx).iter().any(|row| row.id == deleted_id),
                !confirm
            );
        });
    }
    workspace.update_in(&mut cx, |this, window, cx| {
        this.toggle_command_manager(window, cx);
        this.toggle_command_manager(window, cx);
    });
    draw(&mut cx);
    workspace.read_with(&cx, |this, cx| {
        assert!(this.filtered_saved_commands(cx).iter().all(|row| row.id != deleted_id));
    });
    let saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    assert!(
        saved
            .builtin_commands(
                crate::i18n::UiLanguage::EnUs,
                crate::saved_commands::builtins::CommandPlatform::Windows
            )
            .iter()
            .all(|row| row.id != deleted_id)
    );
}
