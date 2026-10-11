use super::{file_tree::display_path, *};
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, size};
use std::path::PathBuf;

mod wire;

const DESTINATION: &str = "fixture@host\ninvalid";

#[test]
fn isolated_dual_file_tree_controls() {
    for test in [
        "paired_drawers_use_explicit_panes_and_real_resize_controls",
        "source_drawers_keep_distinct_paths_search_selection_and_scroll",
        "wire::drag_uses_the_paired_controller_and_rejects_stale_confirmation",
    ] {
        let directory = tempfile::tempdir().unwrap();
        files_fixture::prepare(directory.path(), "en-US").unwrap();
        let output = files_fixture::child_command(directory.path())
            .arg("--exact")
            .arg(format!("gpui_shell::workspace::dual_files::tests::{test}"))
            .args(["--ignored", "--nocapture"])
            .env("PEBREL_DUAL_FILES_QA_DIR", directory.path())
            .output()
            .unwrap();
        let evidence = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success() && evidence.contains("1 passed;"), "{test}: {evidence}");
    }
}

fn draw(window: &mut VisualTestContext) {
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn click(window: &mut VisualTestContext, selector: impl Into<String>) {
    let selector = Box::leak(selector.into().into_boxed_str());
    let bounds = window.debug_bounds(selector).unwrap_or_else(|| panic!("missing {selector}"));
    window.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    window.simulate_click(bounds.center(), Modifiers::default());
    draw(window);
}

fn append_pane(
    workspace: &mut NebulaWorkspace,
    directory: &Path,
    remote: bool,
    ready: bool,
    window: &mut Window,
    cx: &mut Context<NebulaWorkspace>,
) -> u64 {
    files_fixture::append_pane(
        workspace,
        directory,
        remote.then_some(DESTINATION),
        ready,
        ready.then_some("/synthetic/retained"),
        window,
        cx,
    )
}

#[gpui::test]
#[ignore = "launched by isolated_dual_file_tree_controls with synthetic preferences"]
fn paired_drawers_use_explicit_panes_and_real_resize_controls(cx: &mut TestAppContext) {
    let directory =
        PathBuf::from(std::env::var_os("PEBREL_DUAL_FILES_QA_DIR").expect("isolated fixture"));
    files_fixture::assert_isolated(&directory);
    cx.update(|cx| {
        crate::gpui_shell::init(cx, None);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
    });
    let mut workspace = None;
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
            workspace.tabs_position = nebula_settings::TabsPositionName::Top;
            workspace.sidebar_collapsed = true;
            append_pane(&mut workspace, &directory, false, false, window, cx);
            append_pane(&mut workspace, &directory, false, false, window, cx);
            workspace
        });
        workspace = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let workspace = workspace.unwrap();
    window.update(|window, _| window.activate_window());
    window.simulate_resize(size(px(1400.0), px(800.0)));
    draw(window);
    assert!(!workspace.read_with(window, |workspace, _| workspace.dual_files.enabled));
    assert!(window.debug_bounds("local-files-slot").is_none());
    click(window, "dual-file-trees");
    assert!(window.debug_bounds("dual-files-no-pair").is_some());
    assert!(
        workspace.read_with(window, |workspace, cx| workspace.remote_browser_owner(cx).is_none())
    );
    let (local, other_remote, remote) = window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let WorkspaceTab::Terminal { panes, .. } = &workspace.tabs[0] else { panic!() };
            let local = panes[1].id;
            let other = append_pane(workspace, &directory, true, true, window, cx);
            let remote = append_pane(workspace, &directory, true, true, window, cx);
            append_pane(workspace, &directory, true, false, window, cx);
            assert_eq!(
                workspace.available_file_pairs(cx).len(),
                4,
                "pending SSH is not a candidate"
            );
            (local, other, remote)
        })
    });
    draw(window);
    assert!(workspace.read_with(window, |workspace, _| workspace.dual_files.pair.is_none()));
    click(window, format!("file-pair-{local}-{remote}"));
    let old_target = workspace.read_with(window, |workspace, cx| {
        assert_eq!(workspace.dual_files.pair.as_ref().unwrap().local, local);
        assert_eq!(workspace.remote_browser_owner(cx), Some((remote, DESTINATION.into())));
        workspace.current_remote_transfer_target(cx).unwrap()
    });
    assert_eq!(old_target.path, "/synthetic/retained");
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.focus_pane(0, other_remote, window, cx);
            workspace.focus_pane(0, local, window, cx);
        })
    });
    draw(window);
    workspace.read_with(window, |workspace, cx| {
        assert_eq!(workspace.current_remote_transfer_target(cx), Some(old_target.clone()));
        assert!(workspace.remote_transfer_target_matches(&old_target, cx));
    });
    let before = window.debug_bounds("local-files-slot").unwrap().size.width;
    let start = window.debug_bounds("local-files-resize").unwrap().center();
    let end = point(start.x + px(50.0), start.y);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(window);
    window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    draw(window);
    window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(window);
    assert!(window.debug_bounds("local-files-slot").unwrap().size.width > before);
    let preferred = workspace.read_with(window, |workspace, _| workspace.dual_files.width);
    window.simulate_resize(size(px(440.0), px(800.0)));
    draw(window);
    let left = window.debug_bounds("local-files-slot").unwrap().size.width;
    let right = window.debug_bounds("workspace-details-slot").unwrap().size.width;
    assert!(left + right <= px(264.1));
    window.simulate_resize(size(px(1400.0), px(800.0)));
    draw(window);
    assert_eq!(workspace.read_with(window, |workspace, _| workspace.dual_files.width), preferred);
    let start = window.debug_bounds("local-files-resize").unwrap().center();
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(window);
    window.simulate_mouse_move(
        point(start.x + px(30.0), start.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    window.simulate_keystrokes("escape");
    draw(window);
    assert_eq!(workspace.read_with(window, |workspace, _| workspace.dual_files.width), preferred);
    assert!(workspace.read_with(window, |workspace, _| workspace.dual_files.resize.is_none()));
    let start = window.debug_bounds("local-files-resize").unwrap().center();
    let end = point(start.x + px(30.0), start.y);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(window);
    window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    draw(window);
    assert!(workspace.read_with(window, |workspace, _| workspace.dual_files.width > preferred));
    // Exercise the platform activation observer, not the cancellation method.
    window.deactivate_window();
    draw(window);
    assert!(!window.update(|window, _| window.is_window_active()));
    assert!(workspace.read_with(window, |workspace, _| workspace.dual_files.resize.is_none()));
    assert_eq!(workspace.read_with(window, |workspace, _| workspace.dual_files.width), preferred);
    window.update(|window, _| window.activate_window());
    draw(window);
    assert!(window.update(|window, _| window.is_window_active()));
    window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(window);
    assert_eq!(workspace.read_with(window, |workspace, _| workspace.dual_files.width), preferred);
    click(window, "close-local-files");
    assert!(window.debug_bounds("local-files-slot").is_none());
    click(window, "dual-file-trees");
    click(window, format!("file-pair-{local}-{remote}"));
    // Closing/replacing a connection preserves the host string, never pane authority.
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_pane(0, remote, window, cx))
    });
    draw(window);
    assert!(window.debug_bounds("dual-files-unavailable").is_some());
    let replacement = window.update(|window, cx| {
        workspace
            .update(cx, |workspace, cx| append_pane(workspace, &directory, true, true, window, cx))
    });
    draw(window);
    workspace.read_with(window, |workspace, cx| {
        assert!(workspace.current_remote_transfer_target(cx).is_none());
        assert!(!workspace.remote_transfer_target_matches(&old_target, cx));
    });
    click(window, "choose-file-pair");
    click(window, format!("file-pair-{local}-{replacement}"));
    workspace.read_with(window, |workspace, cx| {
        let new_target = workspace.current_remote_transfer_target(cx).unwrap();
        assert_eq!(new_target.destination, old_target.destination);
        assert_eq!(new_target.pane, replacement);
        assert_ne!(new_target.navigation_generation, old_target.navigation_generation);
        assert!(!workspace.remote_transfer_target_matches(&old_target, cx));
    });
}

type DrawerState = (Option<PathBuf>, String, Option<PathBuf>, Vec<PathBuf>, usize, bool);

fn drawer_state(
    workspace: &Entity<NebulaWorkspace>,
    source: u64,
    window: &VisualTestContext,
) -> DrawerState {
    workspace.read_with(window, |workspace, cx| {
        workspace.dual_files.drawers.get(&source).unwrap().read(cx).files_test_state(cx)
    })
}

fn drawer_focus(
    workspace: &Entity<NebulaWorkspace>,
    source: u64,
    window: &mut VisualTestContext,
) -> (bool, bool) {
    window.update(|window, cx| {
        workspace
            .read(cx)
            .dual_files
            .drawers
            .get(&source)
            .unwrap()
            .read(cx)
            .files_test_focus(window, cx)
    })
}

fn wait_drawer(
    workspace: &Entity<NebulaWorkspace>,
    source: u64,
    window: &mut VisualTestContext,
    ready: impl Fn(&DrawerState) -> bool,
) -> DrawerState {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        window.executor().advance_clock(Duration::from_millis(100));
        draw(window);
        let state = drawer_state(workspace, source, window);
        if ready(&state) {
            return state;
        }
        assert!(std::time::Instant::now() < deadline, "drawer did not settle: {state:?}");
        // SidePanel's filesystem workers use real threads; advancing the GPUI
        // timer alone does not guarantee that their snapshot/search has arrived.
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn replace_input(window: &mut VisualTestContext, text: &str) {
    window.simulate_keystrokes(
        if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
            "cmd-a"
        } else {
            "ctrl-a"
        },
    );
    window.simulate_input(text);
    draw(window);
}

fn scroll_source(window: &mut VisualTestContext, delta: f32) {
    let position = window.debug_bounds("local-files-list").unwrap().center();
    window.simulate_mouse_move(position, None, Modifiers::default());
    window.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(delta))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    draw(window);
}

#[gpui::test]
#[ignore = "launched by isolated_dual_file_tree_controls with synthetic preferences"]
fn source_drawers_keep_distinct_paths_search_selection_and_scroll(cx: &mut TestAppContext) {
    let directory =
        PathBuf::from(std::env::var_os("PEBREL_DUAL_FILES_QA_DIR").expect("isolated fixture"));
    files_fixture::assert_isolated(&directory);
    let roots = [directory.join("owner-a"), directory.join("owner-b")];
    let browsed = [roots[0].join("中文 folder a"), roots[1].join("中文 folder b")];
    for (index, prefix) in ["alpha", "beta"].into_iter().enumerate() {
        std::fs::create_dir_all(&browsed[index]).unwrap();
        std::fs::write(roots[index].join(format!("{prefix}-root-only.txt")), b"root").unwrap();
        for item in 0..100 {
            std::fs::write(browsed[index].join(format!("{prefix}-{item:03}.txt")), b"fixture")
                .unwrap();
        }
    }
    let process_cwd = std::env::current_dir().unwrap();
    cx.update(|cx| {
        crate::gpui_shell::init(cx, None);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
    });
    let mut fixture = None;
    let mut workspace = None;
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
            workspace.tabs_position = nebula_settings::TabsPositionName::Top;
            workspace.sidebar_collapsed = true;
            let a = append_pane(&mut workspace, &roots[0], false, false, window, cx);
            let b = append_pane(&mut workspace, &roots[1], false, false, window, cx);
            let remote = append_pane(&mut workspace, &directory, true, true, window, cx);
            fixture = Some((a, b, remote));
            workspace
        });
        workspace = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let (a, b, remote) = fixture.unwrap();
    let workspace = workspace.unwrap();
    window.update(|window, _| window.activate_window());
    window.simulate_resize(size(px(1400.0), px(800.0)));
    draw(window);
    click(window, "dual-file-trees");
    let mut saved = Vec::new();
    for (index, source, query, delta) in [(0, a, "alpha", -330.0), (1, b, "beta", -620.0)] {
        if index > 0 {
            click(window, "choose-file-pair");
        }
        click(window, format!("file-pair-{source}-{remote}"));
        let initial = wait_drawer(&workspace, source, window, |state| {
            state.0.as_deref() == Some(roots[index].as_path()) && !state.5 && !state.3.is_empty()
        });
        assert!(initial.1.is_empty() && initial.2.is_none() && initial.4 == 0, "{initial:?}");
        assert!(initial.3.contains(&roots[index].join(format!("{query}-root-only.txt"))));
        assert!(!initial.3.iter().any(|path| path.starts_with(&roots[1 - index])));
        click(window, "local-files-path");
        assert_eq!(drawer_focus(&workspace, source, window), (true, false));
        replace_input(window, browsed[index].file_name().unwrap().to_str().unwrap());
        window.simulate_keystrokes("enter");
        let canonical = PathBuf::from(display_path(&browsed[index].canonicalize().unwrap()));
        wait_drawer(&workspace, source, window, |state| {
            state.0.as_ref() == Some(&canonical) && !state.5 && state.3.len() >= 100
        });
        assert!(window.debug_bounds("local-files-path").is_some(), "Enter commits the editor");
        assert_eq!(drawer_focus(&workspace, source, window), (false, true));
        click(window, "local-files-path");
        replace_input(window, "unsubmitted-draft");
        window.simulate_keystrokes("escape");
        draw(window);
        assert!(window.debug_bounds("local-files-path").is_some(), "Escape removes the editor");
        assert_eq!(drawer_focus(&workspace, source, window), (false, true));
        assert_eq!(drawer_state(&workspace, source, window).0.as_ref(), Some(&canonical));
        click(window, "local-files-search");
        replace_input(window, query);
        let found = wait_drawer(&workspace, source, window, |state| {
            state.1 == query && !state.5 && state.3.len() == 100
        });
        assert!(found.3.iter().all(|path| path.parent() == Some(canonical.as_path())
            && path.file_name().unwrap().to_str().unwrap().starts_with(query)));
        scroll_source(window, delta);
        let scrolled = drawer_state(&workspace, source, window);
        assert!(scrolled.4 > 0, "real wheel must change list position: {scrolled:?}");
        let selected = scrolled.4 + 1;
        click(window, format!("local-file-row-{selected}"));
        let state = drawer_state(&workspace, source, window);
        assert_eq!(state.2.as_ref(), Some(&state.3[selected]));
        saved.push(state);
    }
    assert_ne!(saved[0].4, saved[1].4, "fixtures exercise different scroll positions");
    // Return through the actual pairing menu twice; each owner retains its own
    // manually browsed path, input, selected file and virtual-list position.
    for (index, source) in [(0, a), (1, b)] {
        click(window, "choose-file-pair");
        click(window, format!("file-pair-{source}-{remote}"));
        let restored = wait_drawer(&workspace, source, window, |state| !state.5);
        assert_eq!(restored, saved[index]);
        assert!(
            window
                .debug_bounds(Box::leak(
                    format!("local-file-row-{}", restored.4 + 1).into_boxed_str()
                ))
                .is_some()
        );
        workspace.read_with(window, |workspace, cx| {
            assert_eq!(workspace.remote_browser_owner(cx), Some((remote, DESTINATION.into())));
        });
    }
    click(window, "local-files-search");
    assert_eq!(drawer_focus(&workspace, b, window), (false, true));
    click(window, "close-local-files");
    assert_eq!(drawer_focus(&workspace, b, window), (false, false));
    window.simulate_input("hidden-search-input");
    draw(window);
    assert_eq!(drawer_state(&workspace, b, window).1, saved[1].1);
    click(window, "dual-file-trees");
    click(window, format!("file-pair-{b}-{remote}"));
    assert_eq!(wait_drawer(&workspace, b, window, |state| !state.5), saved[1]);
    click(window, "local-files-path");
    assert_eq!(drawer_focus(&workspace, b, window), (true, false));
    click(window, "close-local-files");
    assert_eq!(drawer_focus(&workspace, b, window), (false, false));
    window.simulate_input("hidden-path-input");
    draw(window);
    click(window, "dual-file-trees");
    click(window, format!("file-pair-{b}-{remote}"));
    // The untouched draft is still a valid directory. A hidden input receiving
    // the previous keys would instead leave the editor open with a path error.
    click(window, "local-files-path-go");
    assert!(window.debug_bounds("local-files-path").is_some());
    assert_eq!(drawer_focus(&workspace, b, window), (false, true));
    assert_eq!(drawer_state(&workspace, b, window).0, saved[1].0);
    let pinned = workspace
        .read_with(window, |workspace, cx| workspace.current_remote_transfer_target(cx).unwrap());
    click(window, "side-panel-git");
    for (index, source) in [(1, b), (0, a)] {
        click(window, format!("pane-header-grip-{source}"));
        workspace.read_with(window, |workspace, cx| {
            assert_eq!(workspace.side_panel.view, crate::display::side_panel::PanelView::Git);
            assert_eq!(workspace.side_panel.root(), Some(roots[index].as_path()));
            let WorkspaceTab::Terminal { focused, .. } = &workspace.tabs[0] else { panic!() };
            assert_eq!(*focused, source, "real pane header changes focus");
            assert_eq!(workspace.current_remote_transfer_target(cx), Some(pinned.clone()));
        });
    }
    assert_eq!(std::env::current_dir().unwrap(), process_cwd);
}
