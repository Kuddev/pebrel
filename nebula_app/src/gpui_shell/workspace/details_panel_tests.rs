use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point};

fn open(
    path: std::path::PathBuf,
    cx: &mut TestAppContext,
) -> (Entity<NebulaWorkspace>, VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        cx.set_reduce_motion(true);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
    });
    let mut workspace = None;
    let (_, mut window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                crate::runtime_api::RuntimeHub::new(),
                windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular,
                cx,
            )
        });
        view.update(cx, |view, cx| {
            view.open_document_path(path, window, cx);
            view.side_panel.open = true;
            cx.notify();
        });
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
    (workspace.unwrap(), window.clone())
}

#[test]
fn width_limits_keep_room_for_the_document_without_losing_preference() {
    assert_eq!(panel_width(320.0, 1400.0), 320.0);
    assert_eq!(panel_width(1000.0, 1400.0), MAX_WIDTH);
    assert_eq!(panel_width(-1.0, 1400.0), MIN_WIDTH);
    assert_eq!(panel_width(500.0, 400.0), 184.0);
    assert_eq!(panel_width(500.0, 1400.0), 500.0);
}

#[gpui::test]
fn file_path_edit_navigates_and_keeps_the_last_directory_on_error_or_escape(
    cx: &mut TestAppContext,
) {
    let select_all = match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-a",
        _ => "ctrl-a",
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sidebar.md");
    let target = directory.path().join("中文 folder");
    std::fs::write(&path, "# Heading").unwrap();
    std::fs::create_dir(&target).unwrap();
    let process_cwd = std::env::current_dir().unwrap();
    let (workspace, mut cx) = open(path, cx);
    workspace.update(&mut cx, |view, cx| {
        view.details_panel.section = None;
        view.side_panel.view = PanelView::Files;
        view.side_panel.set_custom_root(directory.path().to_path_buf());
        cx.notify();
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let path_button = cx.debug_bounds("file-tree-path").unwrap();
    cx.simulate_click(path_button.center(), Modifiers::default());
    cx.simulate_keystrokes(select_all);
    cx.simulate_input("中文 folder");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    workspace.read_with(&cx, |view, _| {
        assert!(view.file_tree_path.is_none());
        assert_eq!(
            view.side_panel.root().unwrap().canonicalize().unwrap(),
            target.canonicalize().unwrap()
        );
    });
    assert_eq!(std::env::current_dir().unwrap(), process_cwd);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let empty = cx.debug_bounds("file-tree-empty").unwrap();
    let message = cx.debug_bounds("file-tree-empty-message").unwrap();
    assert!((f32::from(empty.center().x - message.center().x)).abs() <= 1.0);
    assert!((f32::from(empty.center().y - message.center().y)).abs() <= 1.0);
    let path_button = cx.debug_bounds("file-tree-path").unwrap();
    cx.simulate_click(path_button.center(), Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("file-tree-path-editor").is_some());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    workspace.read_with(&cx, |view, _| {
        assert!(
            view.file_tree_path.is_none(),
            "submitting the current directory must succeed: {:?}",
            view.file_tree_path_error()
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let path_button = cx.debug_bounds("file-tree-path").unwrap();
    cx.simulate_click(path_button.center(), Modifiers::default());
    cx.simulate_keystrokes(select_all);
    cx.simulate_input("missing-directory");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    workspace.read_with(&cx, |view, _| {
        assert!(view.file_tree_path_error().is_some());
        assert_eq!(
            view.side_panel.root().unwrap().canonicalize().unwrap(),
            target.canonicalize().unwrap()
        );
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    workspace.read_with(&cx, |view, _| assert!(view.file_tree_path.is_none()));
}

#[gpui::test]
fn shared_sidebar_drag_changes_actual_geometry_and_escape_cancels(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sidebar.md");
    std::fs::write(&path, "# Heading\n\nBody").unwrap();
    let (workspace, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let before = cx.debug_bounds("workspace-details-slot").unwrap();
    let handle = cx.debug_bounds("workspace-details-resize").unwrap();
    assert!(handle.size.width >= px(8.0));
    assert_eq!(handle.top(), px(0.0), "resize must include the titlebar");
    let start = handle.center();
    let target = point(start.x - px(40.0), start.y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let after = cx.debug_bounds("workspace-details-slot").unwrap();
    assert!(after.size.width > before.size.width, "{before:?} -> {after:?}");
    cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert!(workspace.read_with(&cx, |view, _| view.details_panel.resize.is_none()));
    cx.update(|window, cx| {
        assert!(window.selected_text(cx).is_empty());
        let _ = window.draw(cx);
    });
    let start = cx.debug_bounds("workspace-details-resize").unwrap().center();
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workspace.read_with(&cx, |view, _| view.details_panel.resize.is_none()));
}

#[gpui::test]
fn sidebar_tabs_keep_square_hit_targets_and_files_precedes_git(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("controls.md");
    std::fs::write(&path, "# Heading").unwrap();
    let (workspace, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    for id in ["details-info", "side-panel-files", "side-panel-git", "workspace-details-close"] {
        let bounds = cx.debug_bounds(id).unwrap();
        assert_eq!(bounds.size.width, px(32.0), "{id}: {bounds:?}");
        assert_eq!(bounds.size.height, px(32.0), "{id}: {bounds:?}");
    }
    let files = cx.debug_bounds("side-panel-files").unwrap();
    assert!(files.right() <= cx.debug_bounds("side-panel-git").unwrap().left());
    let corner = point(files.left() + px(3.0), files.top() + px(3.0));
    cx.simulate_mouse_down(corner, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(corner, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(workspace.read_with(&cx, |view, cx| view.active_document_section(cx).is_none()));
    assert_eq!(cx.debug_bounds("details-outline").unwrap().size.width, px(32.0));
    assert!(cx.debug_bounds("side-panel-files").unwrap().size.width > px(32.0));
    let selected = cx.debug_bounds("side-panel-files").unwrap().center();
    cx.simulate_mouse_down(selected, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(selected, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert!(!workspace.read_with(&cx, |view, _| view.side_panel.open));
}

#[gpui::test]
fn right_sidebar_stops_at_minimum_then_closes_on_release_and_reopens_at_previous_width(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("detent.md");
    std::fs::write(&path, "# Heading").unwrap();
    let (workspace, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let original = workspace.read_with(&cx, |view, _| view.details_panel.width);
    let start = cx.debug_bounds("workspace-details-resize").unwrap().center();
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let minimum = point(start.x + px(original - MIN_WIDTH + 18.0), start.y);
    cx.simulate_mouse_move(minimum, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(workspace.read_with(&cx, |view, _| view.details_panel.width), MIN_WIDTH);
    assert!(workspace.read_with(&cx, |view, _| view.side_panel.open));
    let close = point(start.x + px(original - MIN_WIDTH + 100.0), start.y);
    cx.simulate_mouse_move(close, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    assert!(workspace.read_with(&cx, |view, _| view.side_panel.open));
    cx.simulate_mouse_up(close, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert!(!workspace.read_with(&cx, |view, _| view.side_panel.open));
    workspace.update(&mut cx, |view, cx| view.toggle_document_details(cx));
    assert_eq!(workspace.read_with(&cx, |view, _| view.details_panel.width), original);
}

#[gpui::test]
fn left_sidebar_close_and_escape_share_the_right_sidebar_detent(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("left.md");
    std::fs::write(&path, "# Heading").unwrap();
    let (workspace, mut cx) = open(path, cx);
    let original = workspace.read_with(&cx, |view, _| view.sidebar_width);
    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.sidebar_collapsed = false;
            view.sidebar_resizing =
                Some(super::super::sidebar_resize::ResizeDrag::new(original, original));
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    let close = point(px(nebula_settings::MIN_SIDEBAR_WIDTH - 100.0), px(200.0));
    cx.simulate_mouse_move(close, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(workspace.read_with(&cx, |view, _| view.sidebar_width), original);
    assert!(!workspace.read_with(&cx, |view, _| view.sidebar_collapsed));
    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.sidebar_resizing =
                Some(super::super::sidebar_resize::ResizeDrag::new(original, original));
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    cx.simulate_mouse_move(close, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_up(close, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert!(workspace.read_with(&cx, |view, _| view.sidebar_collapsed));
    assert_eq!(workspace.read_with(&cx, |view, _| view.sidebar_width), original);
}

#[gpui::test]
fn document_outline_and_files_share_one_slot_and_one_width(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tabs.md");
    std::fs::write(&path, "# Heading\n\nBody").unwrap();
    let (workspace, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.details_panel.width = 380.0;
            view.select_document_section(DocumentSection::Outline, cx);
        });
        let _ = window.draw(cx);
    });
    let outline = cx.debug_bounds("markdown-outline-list").unwrap();
    let slot = cx.debug_bounds("workspace-details-slot").unwrap();
    assert!(outline.left() >= slot.left());
    let width = slot.size.width;
    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| view.toggle_file_tree(cx));
        let _ = window.draw(cx);
    });
    assert!(workspace.read_with(&cx, |view, cx| view.active_document_section(cx).is_none()));
    assert!(cx.debug_bounds("markdown-outline-list").is_none());
    assert_eq!(cx.debug_bounds("workspace-details-slot").unwrap().size.width, width);
}
