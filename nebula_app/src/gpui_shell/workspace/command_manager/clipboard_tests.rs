//! Real dialog key routing with GPUI's in-memory TestPlatform clipboard only.

use super::*;
use gpui::{TestAppContext, VisualTestContext};

const NAME: &str = "编译 🚀 fixture";
const COMMAND: &str = "echo α\nprintf '🙂'\necho finish";
const SENTINEL: &str = "synthetic clipboard sentinel";

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn key(action: &str) -> &'static str {
    match (crate::platform::Platform::current() == crate::platform::Platform::MacOS, action) {
        (true, "all") => "cmd-a",
        (true, "copy") => "cmd-c",
        (true, "cut") => "cmd-x",
        (true, "undo") => "cmd-z",
        (false, "all") => "ctrl-a",
        (false, "copy") => "ctrl-c",
        (false, "cut") => "ctrl-x",
        (false, "undo") => "ctrl-z",
        _ => unreachable!(),
    }
}

fn notification_count(cx: &mut VisualTestContext) -> usize {
    cx.update(|window, cx| window.notifications(cx).len())
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    // VisualTestContext delegates to TestPlatform, never the native clipboard.
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn reset_notices(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        // Establish a deterministic duplicate-gate baseline without changing
        // production toast policy or sleeping on a real-time cooldown.
        crate::gpui_shell::toast::toast(
            window,
            cx,
            crate::gpui_shell::toast::ToastKind::Info,
            "synthetic clipboard feedback fixture",
        );
        let root = window.root::<Root>().flatten().unwrap();
        root.update(cx, |root, cx| root.clear_notifications(window, cx));
    });
    draw(cx);
    assert_eq!(notification_count(cx), 0);
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
    let mut cx = window.clone();
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    draw(&mut cx);
    (workspace.unwrap(), cx)
}

fn check_focused_input(text: &str, cx: &mut VisualTestContext) {
    // The pinned InputState groups edits for one real-time second. Add-dialog
    // fixture typing must finish its undo group before exercising Cut, or Undo
    // correctly reverts both the fixture insertion and the cut together. Its
    // clock is std::time::Instant, so advancing TestExecutor time is insufficient.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    reset_notices(cx);
    cx.write_to_clipboard(ClipboardItem::new_string(SENTINEL.into()));
    cx.simulate_keystrokes(key("copy"));
    cx.simulate_keystrokes(key("cut"));
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(SENTINEL));
    assert_eq!(notification_count(cx), 0, "no selection stays silent");

    cx.simulate_keystrokes(key("all"));
    cx.simulate_keystrokes(key("copy"));
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(text));
    assert_eq!(notification_count(cx), 1, "successful copy renders feedback");

    cx.simulate_keystrokes(key("cut"));
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(text));
    assert_eq!(notification_count(cx), 2, "successful cut renders separate feedback");

    cx.write_to_clipboard(ClipboardItem::new_string(SENTINEL.into()));
    cx.simulate_keystrokes(key("all"));
    cx.simulate_keystrokes(key("copy"));
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(SENTINEL), "cut actually removed the input text");
    assert_eq!(notification_count(cx), 2, "empty input does not report success");

    cx.simulate_keystrokes(key("undo"));
    cx.simulate_keystrokes(key("all"));
    cx.simulate_keystrokes(key("copy"));
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(text), "cut retains the component's undo history");
}

fn check_dialog(edit: bool, cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved_commands.json");
    let mut saved = crate::saved_commands::SavedCommands::load_from(&path).unwrap();
    let id = edit.then(|| saved.insert(NAME, COMMAND, false).unwrap().id);
    let before = std::fs::read(&path).ok();
    let (workspace, mut cx) = open_manager(saved, cx);
    cx.update(|window, cx| {
        workspace.update(cx, |this, cx| this.open_saved_command_editor(id, window, cx));
    });
    draw(&mut cx);
    assert!(cx.debug_bounds("saved-command-save").is_some(), "real add/edit dialog is open");
    if !edit {
        cx.simulate_input(NAME);
    }
    check_focused_input(NAME, &mut cx);
    reset_notices(&mut cx);
    // The previous Select All remains selected. Collapse to its beginning,
    // then select two CJK graphemes with real keyboard selection events.
    cx.simulate_keystrokes("left shift-right shift-right");
    cx.simulate_keystrokes(key("copy"));
    draw(&mut cx);
    assert_eq!(clipboard(&mut cx).as_deref(), Some("编译"));
    assert_eq!(notification_count(&mut cx), 1);
    cx.simulate_keystrokes(key("cut"));
    draw(&mut cx);
    assert_eq!(clipboard(&mut cx).as_deref(), Some("编译"));
    assert_eq!(notification_count(&mut cx), 2);
    cx.simulate_keystrokes(key("undo"));
    cx.simulate_keystrokes("tab");
    if !edit {
        cx.simulate_input(COMMAND);
    }
    check_focused_input(COMMAND, &mut cx);
    assert_eq!(std::fs::read(&path).ok(), before, "clipboard actions do not persist edits");
    assert!(workspace.read_with(&cx, |this, _| this.tabs.is_empty()), "no terminal was started");
}

#[gpui::test]
fn add_command_dialog_copy_and_cut_report_actual_selection(cx: &mut TestAppContext) {
    check_dialog(false, cx);
}

#[gpui::test]
fn edit_command_dialog_copy_and_cut_report_actual_selection(cx: &mut TestAppContext) {
    check_dialog(true, cx);
}

#[gpui::test]
fn command_search_keeps_its_existing_clipboard_behavior(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let saved = crate::saved_commands::SavedCommands::load_from(
        &directory.path().join("saved_commands.json"),
    )
    .unwrap();
    let (_, mut cx) = open_manager(saved, cx);
    reset_notices(&mut cx);
    cx.simulate_input("search fixture");
    cx.simulate_keystrokes(key("all"));
    cx.simulate_keystrokes(key("copy"));
    draw(&mut cx);
    assert_eq!(clipboard(&mut cx).as_deref(), Some("search fixture"));
    assert_eq!(notification_count(&mut cx), 0, "feedback is scoped to dialog fields");
}
