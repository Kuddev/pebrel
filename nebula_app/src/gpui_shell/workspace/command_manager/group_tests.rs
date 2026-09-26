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
        assert!(
            this.filtered_saved_commands(cx).is_empty(),
            "the moved command is inside its folder"
        );
    });
    let folder =
        cx.debug_bounds(Box::leak(format!("command-group-{group}").into_boxed_str())).unwrap();
    cx.simulate_click(folder.origin + gpui::point(px(6.0), px(6.0)), Modifiers::default());
    draw(&mut cx);
    workspace
        .read_with(&cx, |this, cx| assert_eq!(this.filtered_saved_commands(cx)[0].id, command.id));
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
        assert!(this.filtered_saved_commands(cx).is_empty());
    });
    let back = cx.debug_bounds("saved-command-group-back").unwrap();
    cx.simulate_click(back.center(), Modifiers::default());
    draw(&mut cx);
    workspace
        .read_with(&cx, |this, cx| assert_eq!(this.filtered_saved_commands(cx)[0].id, command.id));
    cx.simulate_keystrokes("down");
    workspace.read_with(&cx, |this, _| assert_eq!(this.command_manager_selected, 1));
}

#[gpui::test]
fn builtin_delete_can_be_cancelled_and_stays_deleted_after_reopening(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved_commands.json");
    let saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    let (workspace, mut cx) = open_manager(saved, cx);
    let folder = cx.debug_bounds("command-group-builtin").unwrap();
    cx.simulate_click(folder.center(), Modifiers::default());
    draw(&mut cx);
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
    let folder = cx.debug_bounds("command-group-builtin").unwrap();
    cx.simulate_click(folder.center(), Modifiers::default());
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

#[gpui::test]
fn root_has_commands_and_enterable_folders_with_aligned_creation_actions(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved_commands.json");
    let mut saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    let command = saved.insert("npm", "npm run dev", true).unwrap();
    let (workspace, mut cx) = open_manager(saved, cx);
    assert!(cx.debug_bounds("command-group-ungrouped").is_none());
    let root_row = cx.debug_bounds("saved-command-row-0").unwrap();
    let folder = cx.debug_bounds("command-group-builtin").unwrap();
    assert!(folder.origin.y >= root_row.bottom());
    assert_eq!(folder.size.height, root_row.size.height);
    let run_icon = cx.debug_bounds("saved-command-run-0").unwrap();
    let folder_icon = cx.debug_bounds("command-group-icon-1").unwrap();
    assert_eq!(folder_icon.size, run_icon.size);
    assert_eq!(folder_icon.origin.x, run_icon.origin.x);
    assert_eq!(
        cx.debug_bounds("command-group-label-1").unwrap().origin.x,
        cx.debug_bounds("saved-command-label-0").unwrap().origin.x
    );
    let command_add = cx.debug_bounds("saved-command-add").unwrap();
    let group_add = cx.debug_bounds("saved-command-add-group").unwrap();
    assert!(
        folder.bottom() <= command_add.origin.y,
        "a fitting folder row must not be clipped by the footer"
    );
    assert_eq!(command_add.origin.x, group_add.origin.x);
    assert_eq!(command_add.size.height, group_add.size.height);
    assert_eq!(
        cx.debug_bounds("saved-command-add-icon").unwrap().origin.x,
        cx.debug_bounds("saved-command-add-group-icon").unwrap().origin.x
    );
    // The only next item at the root is the folder; Enter navigates, never runs.
    cx.simulate_keystrokes("down enter");
    draw(&mut cx);
    workspace.read_with(&cx, |this, cx| {
        assert!(this.command_manager_open && this.tabs.is_empty());
        assert_eq!(
            this.command_manager_group.as_deref(),
            Some(crate::saved_commands::BUILTIN_GROUP_ID)
        );
        assert!(this.filtered_saved_commands(cx).iter().all(|entry| entry.id != command.id));
    });
    assert!(cx.debug_bounds("saved-command-group-back").is_some());
    assert!(cx.debug_bounds("saved-command-add-group").is_none());
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    workspace.read_with(&cx, |this, cx| {
        assert!(this.command_manager_open && this.command_manager_group.is_none());
        assert_eq!(this.filtered_saved_commands(cx)[0].id, command.id);
    });
}

#[gpui::test]
fn command_results_render_only_visible_rows_and_scroll_to_keyboard_selection(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved_commands.json");
    let mut saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    for index in 0..40 {
        saved.insert(&format!("Command {index}"), "echo visible", false).unwrap();
    }
    let (workspace, mut cx) = open_manager(saved, cx);
    assert!(cx.debug_bounds("saved-command-row-0").is_some());
    assert!(cx.debug_bounds("saved-command-row-39").is_none());
    for _ in 0..39 {
        cx.simulate_keystrokes("down");
    }
    draw(&mut cx);
    assert!(cx.debug_bounds("saved-command-row-39").is_some());
    assert!(cx.debug_bounds("saved-command-row-0").is_none());
    workspace.read_with(&cx, |this, _| assert_eq!(this.command_manager_selected, 39));
}

#[gpui::test]
fn root_search_finds_nested_commands_while_folder_search_stays_in_that_folder(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved_commands.json");
    let mut saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    saved.create_group("Work").unwrap();
    let group = saved.groups()[0].id.clone();
    let nested =
        saved.insert_in_group("project-only-token", "echo nested", false, Some(&group)).unwrap();
    let (workspace, mut cx) = open_manager(saved, cx);
    workspace.update_in(&mut cx, |this, window, cx| {
        this.command_manager_input
            .update(cx, |input, cx| input.set_value("project-only-token", window, cx));
    });
    draw(&mut cx);
    workspace.read_with(&cx, |this, cx| {
        assert_eq!(this.filtered_saved_commands(cx), vec![nested.clone()])
    });
    workspace.update_in(&mut cx, |this, window, cx| {
        this.enter_command_group(Some(crate::saved_commands::BUILTIN_GROUP_ID.into()), window, cx);
        this.command_manager_input
            .update(cx, |input, cx| input.set_value("project-only-token", window, cx));
    });
    draw(&mut cx);
    workspace.read_with(&cx, |this, cx| assert!(this.command_manager_rows(cx).is_empty()));
}
