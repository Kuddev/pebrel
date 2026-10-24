use super::group_tests::{draw, open_manager};
use super::*;
use crate::saved_commands::{BUILTIN_GROUP_ID, SavedCommands};
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn bounds(cx: &mut VisualTestContext, selector: &str) -> gpui::Bounds<Pixels> {
    cx.update(|window, _| window.debug_bounds_for_selector(selector)).unwrap()
}

fn click(cx: &mut VisualTestContext, selector: &str) {
    let bounds = bounds(cx, selector);
    // Hit the padding, not just the 16px icon.
    cx.simulate_click(bounds.origin + gpui::point(px(3.0), px(3.0)), Modifiers::default());
    draw(cx);
}

fn command_ids(workspace: &Entity<NebulaWorkspace>, cx: &VisualTestContext) -> Vec<String> {
    workspace.read_with(cx, |this, cx| {
        this.filtered_saved_commands(cx).into_iter().map(|command| command.id).collect()
    })
}

#[gpui::test]
fn folder_order_buttons_keep_the_fixed_folder_and_do_not_enter_or_delete(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let mut saved = SavedCommands::load_from(&path).unwrap();
    saved.create_group("Work").unwrap();
    saved.create_group("Servers").unwrap();
    let work = saved.groups()[0].id.clone();
    let servers = saved.groups()[1].id.clone();
    let (workspace, mut cx) = open_manager(saved, cx);
    let up = format!("group-move-up-{work}");
    let down = format!("group-move-down-{work}");
    assert_eq!(bounds(&mut cx, &down).size, gpui::size(px(32.0), px(32.0)));
    let before = std::fs::read(&path).unwrap();
    click(&mut cx, &up);
    assert_eq!(std::fs::read(&path).unwrap(), before, "first folder cannot move up");
    click(&mut cx, &down);
    assert_eq!(SavedCommands::load_from(&path).unwrap().groups()[0].id, servers);
    workspace.read_with(&cx, |this, cx| {
        assert!(this.command_manager_open && this.command_manager_group.is_none());
        assert_eq!(this.command_manager_selection.cursor, 1);
        assert!(matches!(this.command_manager_rows(cx).last(),
            Some(ManagerRow::Folder { id, .. }) if id == BUILTIN_GROUP_ID));
    });
    assert!(
        cx.update(|window, _| window.debug_bounds_for_selector("group-move-up-builtin")).is_none()
    );
    click(&mut cx, &down);
    assert_eq!(SavedCommands::load_from(&path).unwrap().groups()[0].id, servers);
    click(&mut cx, &up);
    assert_eq!(SavedCommands::load_from(&path).unwrap().groups()[0].id, work);
}

#[gpui::test]
fn command_order_clicks_preserve_folder_membership_selection_and_restart_order(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let mut saved = SavedCommands::load_from(&path).unwrap();
    saved.create_group("Work").unwrap();
    let group = saved.groups()[0].id.clone();
    let a = saved
        .insert_in_group("A long command name with 中文 and emoji 🚀", "echo a", true, Some(&group))
        .unwrap();
    let other = saved.insert("Other folder", "echo other", false).unwrap();
    let b = saved.insert_in_group("Build", "cargo build", true, Some(&group)).unwrap();
    let (workspace, mut cx) = open_manager(saved, cx);
    workspace.update_in(&mut cx, |this, window, cx| {
        this.enter_command_group(Some(group.clone()), window, cx)
    });
    cx.simulate_resize(gpui::size(px(360.0), px(600.0)));
    draw(&mut cx);
    let row = bounds(&mut cx, "saved-command-row-0");
    cx.simulate_mouse_move(row.center(), None, Modifiers::default());
    draw(&mut cx);
    let down = format!("command-move-down-{}", a.id);
    let control = bounds(&mut cx, &down);
    assert_eq!(control.size, gpui::size(px(32.0), px(32.0)));
    assert!(row.contains(&control.origin) && row.contains(&control.bottom_right()));
    click(&mut cx, &down);
    assert_eq!(command_ids(&workspace, &cx), [b.id.clone(), a.id.clone()]);
    workspace.read_with(&cx, |this, _| {
        assert!(this.command_manager_open && this.tabs.is_empty());
        assert_eq!(this.command_manager_group.as_deref(), Some(group.as_str()));
        assert_eq!(this.command_manager_selection.cursor, 1);
    });
    let loaded = SavedCommands::load_from(&path).unwrap();
    assert_eq!(loaded.commands(), &[b, other, a]);
    assert_eq!(
        cx.update(|window, cx| window.notifications(cx).len()),
        0,
        "no command was dispatched"
    );
}

#[gpui::test]
fn search_keeps_relevance_order_and_hides_reorder_controls(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let mut saved = SavedCommands::load_from(&path).unwrap();
    let exact = saved.insert("zxreorder", "echo exact", false).unwrap();
    let partial = saved.insert("prefix zxreorder suffix", "echo partial", false).unwrap();
    saved.create_group("zxreorder folder").unwrap();
    let group = saved.groups()[0].id.clone();
    let (workspace, mut cx) = open_manager(saved, cx);
    let down = format!("command-move-down-{}", exact.id);
    let row = bounds(&mut cx, "saved-command-row-0");
    cx.simulate_mouse_move(row.center(), None, Modifiers::default());
    draw(&mut cx);
    click(&mut cx, &down);
    assert_eq!(command_ids(&workspace, &cx), [partial.id.clone(), exact.id.clone()]);
    workspace.update_in(&mut cx, |this, window, cx| {
        this.command_manager_input.update(cx, |input, cx| input.set_value("zxreorder", window, cx));
    });
    draw(&mut cx);
    assert_eq!(command_ids(&workspace, &cx), [exact.id.clone(), partial.id.clone()]);
    assert!(cx.update(|window, _| window.debug_bounds_for_selector(&down)).is_none());
    assert!(
        cx.update(|window, _| window.debug_bounds_for_selector(&format!("group-move-up-{group}")))
            .is_none()
    );
    workspace.update_in(&mut cx, |this, window, cx| {
        this.command_manager_input.update(cx, |input, cx| input.set_value("  ", window, cx));
    });
    draw(&mut cx);
    assert_eq!(command_ids(&workspace, &cx), [partial.id, exact.id]);
    assert!(cx.update(|window, _| window.debug_bounds_for_selector(&down)).is_some());
    click(&mut cx, "command-select-mode");
    assert!(cx.update(|window, _| window.debug_bounds_for_selector(&down)).is_none());
}

#[gpui::test]
fn locked_order_change_reports_failure_and_can_be_retried(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let mut saved = SavedCommands::load_from(&path).unwrap();
    saved.create_group("Work").unwrap();
    saved.create_group("Servers").unwrap();
    let first = saved.groups()[0].id.clone();
    let (workspace, mut cx) = open_manager(saved, cx);
    let down = format!("group-move-down-{first}");
    let before = std::fs::read(&path).unwrap();
    let lock = crate::atomic_file::try_lock(&path).unwrap().unwrap();
    click(&mut cx, &down);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        workspace.read_with(&cx, |this, _| this.saved_commands.groups()[0].id.clone()),
        first
    );
    assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);
    drop(lock);
    click(&mut cx, &down);
    assert_eq!(SavedCommands::load_from(&path).unwrap().groups()[1].id, first);
}

#[gpui::test]
fn command_order_keyboard_focus_follows_the_moved_command(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let mut saved = SavedCommands::load_from(&path).unwrap();
    let first = saved.insert("First", "echo first", true).unwrap();
    let second = saved.insert("Second", "echo second", true).unwrap();
    let third = saved.insert("Third", "echo third", true).unwrap();
    let (workspace, mut cx) = open_manager(saved, cx);
    let label = bounds(&mut cx, "saved-command-label-0");
    // Search -> Select -> Manage -> Run -> enabled Move down; Move up is disabled.
    cx.simulate_keystrokes("tab tab tab tab");
    draw(&mut cx);
    assert_eq!(bounds(&mut cx, "saved-command-label-0"), label, "focus does not shift text");
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    assert_eq!(
        command_ids(&workspace, &cx),
        [second.id.clone(), first.id.clone(), third.id.clone()]
    );
    cx.simulate_keystrokes("space");
    draw(&mut cx);
    assert_eq!(
        command_ids(&workspace, &cx),
        [second.id.clone(), third.id.clone(), first.id.clone()]
    );
    cx.simulate_keystrokes("enter space enter");
    draw(&mut cx);
    assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 0);
    workspace.read_with(&cx, |this, _| {
        assert!(this.command_manager_open && this.tabs.is_empty());
        assert_eq!(this.command_manager_selection.cursor, 2);
    });
    cx.simulate_keystrokes("tab enter");
    draw(&mut cx);
    assert_eq!(workspace.read_with(&cx, |this, _| this.command_manager_selection.cursor), 1);
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    assert_eq!(command_ids(&workspace, &cx), [first.id, second.id, third.id]);
    cx.simulate_keystrokes("enter space");
    draw(&mut cx);
    assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 0);
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(!workspace.read_with(&cx, |this, _| this.command_manager_open));
}

#[gpui::test]
fn order_controls_accept_tab_enter_and_space_without_opening_folders(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let mut saved = SavedCommands::load_from(&path).unwrap();
    for name in ["Work", "Servers", "Containers"] {
        saved.create_group(name).unwrap();
    }
    let first = saved.groups()[0].id.clone();
    let (workspace, mut cx) = open_manager(saved, cx);
    // Search -> Select -> Manage -> first folder's enabled Move down.
    cx.simulate_keystrokes("tab tab tab");
    draw(&mut cx);
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    assert_eq!(SavedCommands::load_from(&path).unwrap().groups()[1].id, first);
    cx.simulate_keystrokes("space");
    draw(&mut cx);
    assert_eq!(SavedCommands::load_from(&path).unwrap().groups()[2].id, first);
    cx.simulate_keystrokes("enter space enter");
    draw(&mut cx);
    assert!(
        cx.update(|window, _| window.debug_bounds_for_selector(&format!("group-move-up-{first}")))
            .is_some()
    );
    workspace.read_with(&cx, |this, _| {
        assert!(this.command_manager_open && this.command_manager_group.is_none());
        assert_eq!(this.command_manager_selection.cursor, 2);
    });
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(!workspace.read_with(&cx, |this, _| this.command_manager_open));
}
