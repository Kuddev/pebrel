//! True GPUI file gestures through the production preflight/dialog/controller
//! and real owned loopback SSH/SFTP channels. No personal SSH resolution.
use super::*;
use crate::gpui_shell::workspace::remote_files::network_fixture::NetworkPump;
use crate::ssh_session::sftp_test_support::Fixture;

fn settle(window: &mut VisualTestContext, ready: impl Fn(&mut VisualTestContext) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        window.executor().advance_clock(Duration::from_millis(100));
        NetworkPump::tick();
        draw(window);
        if ready(window) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "owned SFTP interaction timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn drag_file(window: &mut VisualTestContext) {
    let start = window.debug_bounds("local-file-row-1").expect("source file row").center();
    let target = window.debug_bounds("remote-files-browser").expect("remote drop surface").center();
    window.simulate_mouse_move(start, None, Modifiers::default());
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(window);
    window.simulate_mouse_move(
        point(start.x + px(20.0), start.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(window);
    window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
    draw(window);
    window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
    draw(window);
}

#[gpui::test]
#[ignore = "launched by isolated_dual_file_tree_controls with owned preferences and endpoints"]
fn drag_uses_the_paired_controller_and_rejects_stale_confirmation(cx: &mut TestAppContext) {
    let root = PathBuf::from(std::env::var_os("PEBREL_DUAL_FILES_QA_DIR").unwrap());
    files_fixture::assert_isolated(&root);
    let _network_pump = NetworkPump::new();
    let a = root.join("local-a");
    let b = root.join("local-b");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    let bytes = "中文 source\0with spaces\n".as_bytes();
    let source = b.join("中文 space.txt");
    std::fs::write(&source, bytes).unwrap();
    let runtime = crate::ssh_session::runtime().unwrap();
    let selected = runtime.block_on(Fixture::new());
    let other = runtime.block_on(Fixture::new());
    cx.update(|cx| {
        crate::gpui_shell::init(cx, None);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
    });
    let mut fixture = None;
    let mut workspace_slot = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
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
            files_fixture::append_pane(&mut workspace, &a, None, false, None, window, cx);
            let local =
                files_fixture::append_pane(&mut workspace, &b, None, false, None, window, cx);
            let remote = files_fixture::append_pane(
                &mut workspace,
                &a,
                Some(&selected.destination),
                true,
                None,
                window,
                cx,
            );
            let another = files_fixture::append_pane(
                &mut workspace,
                &a,
                Some(&other.destination),
                true,
                None,
                window,
                cx,
            );
            fixture = Some((local, remote, another));
            workspace
        });
        workspace_slot = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let (local, remote, another) = fixture.unwrap();
    let workspace = workspace_slot.unwrap();
    window.update(|window, _| window.activate_window());
    window.simulate_resize(size(px(1400.0), px(800.0)));
    draw(window);
    click(window, "dual-file-trees");
    click(window, format!("file-pair-{local}-{remote}"));
    settle(window, |window| {
        window.debug_bounds("local-file-row-1").is_some()
            && workspace.read_with(window, |workspace, cx| {
                workspace.current_remote_transfer_target(cx).is_some()
            })
    });
    // The SFTP-only peer has no interactive shell cwd; browse before capturing.
    window.update(|_, cx| {
        workspace.update(cx, |workspace, cx| workspace.navigate_remote("/target".into(), cx))
    });
    settle(window, |window| {
        workspace.read_with(window, |workspace, cx| {
            workspace
                .current_remote_transfer_target(cx)
                .is_some_and(|target| target.pane == remote && target.path == "/target")
        })
    });
    let captured = workspace
        .read_with(window, |workspace, cx| workspace.current_remote_transfer_target(cx).unwrap());
    assert_eq!((captured.pane, captured.path.as_str()), (remote, "/target"));
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.focus_pane(0, another, window, cx);
            workspace.focus_pane(0, local, window, cx);
        })
    });
    draw(window);
    assert_eq!(
        workspace.read_with(window, |workspace, cx| workspace
            .current_remote_transfer_target(cx)
            .unwrap()),
        captured
    );
    drag_file(window);
    let path = "/target/中文 space.txt";
    settle(window, |_| {
        selected.store.lock().unwrap().files.get(path).is_some_and(|file| file.data == bytes)
    });
    assert!(!other.store.lock().unwrap().files.contains_key(path));
    let updated = b"updated local bytes\0for overwrite";
    std::fs::write(&source, updated).unwrap();
    for button in [
        "sftp-conflict-cancel",
        "sftp-conflict-skip",
        "sftp-conflict-keep-both",
        "sftp-conflict-overwrite",
    ] {
        // Wait for controller completion and retained browser entries before the next gesture.
        settle(window, |window| {
            workspace.read_with(window, |workspace, cx| {
                workspace.current_remote_transfer_target(cx).is_some()
                    && !workspace.remote_browser.files_test_transfer_working()
            }) && window.debug_bounds("sftp-conflict-cancel").is_none()
        });
        drag_file(window);
        settle(window, |window| window.debug_bounds(button).is_some());
        click(window, button);
        settle(window, |window| window.debug_bounds(button).is_none());
        settle(window, |window| {
            !workspace.read_with(window, |workspace, _| {
                workspace.remote_browser.files_test_transfer_working()
            })
        });
        assert_eq!(
            selected.store.lock().unwrap().files[path].data,
            if button == "sftp-conflict-overwrite" { updated.as_slice() } else { bytes }
        );
        assert!(!other.store.lock().unwrap().files.contains_key(path));
    }
    settle(window, |_| {
        selected
            .store
            .lock()
            .unwrap()
            .files
            .iter()
            .any(|(file, node)| file != path && node.data == updated)
    });
    assert_eq!(selected.store.lock().unwrap().files[path].data, updated);
    drag_file(window);
    settle(window, |window| window.debug_bounds("sftp-conflict-overwrite").is_some());
    std::fs::write(&source, b"stale source must not publish").unwrap();
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_pane(0, remote, window, cx))
    });
    draw(window);
    click(window, "sftp-conflict-overwrite");
    assert!(!workspace.read_with(window, |workspace, cx| {
        workspace.remote_transfer_target_matches(&captured, cx)
    }));
    assert!(!other.store.lock().unwrap().files.contains_key(path));
    let selected_files = selected.store.lock().unwrap().files.len();
    for _ in 0..20 {
        window.executor().advance_clock(Duration::from_millis(100));
        NetworkPump::tick();
        draw(window);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(selected.store.lock().unwrap().files.len(), selected_files);
    assert_eq!(selected.store.lock().unwrap().files[path].data, updated);
}
