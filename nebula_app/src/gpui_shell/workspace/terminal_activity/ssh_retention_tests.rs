use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use gpui::{TestAppContext, VisualTestContext};
use nebula_terminal::event::Event as TermEvent;

#[test]
fn isolated_ended_ssh_workspace_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(".ssh")).unwrap();
    std::fs::write(directory.path().join(".ssh/config"), "").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "gpui_shell::workspace::terminal_activity::ssh_retention_tests::ended_ssh_keeps_last_tab_and_reconnect_replaces_only_its_pane",
            "--ignored",
            "--nocapture",
        ])
        .env_remove("PEBREL_CONFIG_FILE")
        .env_remove("NEBULA_CONFIG_FILE")
        .env_remove("PEBREL_GPUI_CONFIG")
        .env_remove("NEBULA_GPUI_CONFIG")
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("NEBULA_GPUI_OPEN_DOC")
        .env("PEBREL_TEST_SSH_HOME", directory.path())
        .env("PEBREL_CONFIG_DIR", directory.path())
        .env("PEBREL_SSH_LIFECYCLE_QA_DIR", directory.path())
        .output()
        .unwrap();
    let evidence = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::gpui_shell::try_write_stderr(format_args!("{evidence}"));
    assert!(output.status.success() && evidence.contains("1 passed;"), "{evidence}");
}

fn draw(window: &mut VisualTestContext) -> (usize, usize) {
    window.run_until_parked();
    window.update(|window, cx| {
        let delivered = crate::gpui_shell::terminal::SshSessionPump::drain(cx);
        window.refresh();
        window.draw(cx).clear(cx);
        delivered
    })
}

#[gpui::test]
#[ignore = "launched by isolated_ended_ssh_workspace_lifecycle with synthetic preferences"]
fn ended_ssh_keeps_last_tab_and_reconnect_replaces_only_its_pane(cx: &mut TestAppContext) {
    let directory = std::path::PathBuf::from(
        std::env::var_os("PEBREL_SSH_LIFECYCLE_QA_DIR").expect("isolated fixture directory"),
    );
    assert_eq!(Some(directory.as_os_str()), std::env::var_os("PEBREL_CONFIG_DIR").as_deref());
    assert_eq!(crate::ssh::ssh_config_path(), Some(directory.join(".ssh/config")));
    assert_eq!(crate::ssh::read_ssh_config().unwrap(), "");
    let path = directory.join("pebrel_settings.txt");
    assert!(!path.exists());
    std::fs::write(&path, "language=en-US\nai_hooks=0\nauto_check_updates=0\nkeep_ssh_session=1\n")
        .unwrap();
    let settings = nebula_settings::RawSettings::load();
    assert_eq!(settings.bool_on("ai_hooks"), Some(false));
    assert_eq!(settings.bool_on("auto_check_updates"), Some(false));
    cx.update(|cx| {
        cx.set_global(crate::gpui_shell::terminal::SshSessionPump::default());
        gpui_component::init(cx);
        crate::gpui_shell::init(cx, None);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
    });
    let mut workspace = None;
    let mut old_view = None;
    let mut input = None;
    // An invalid synthetic destination stops a requested retry before OpenSSH
    // config/key discovery or any network authentication can run.
    let destination = "fixture@host\ninvalid";
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
            let mut pane = workspace.new_pane(
                (80, 24),
                TerminalLaunch::Local {
                    cwd: None,
                    shell: Some(nebula_terminal::tty::Shell::new(
                        "pebrel-test-missing-shell-executable".into(),
                        vec![],
                    )),
                    shell_name: None,
                },
                None,
                window,
                cx,
            );
            pane.custom_name = Some("retained-output".into());
            pane.name_history = vec![Some("old-name".into())];
            input = Some(pane.view.update(cx, |view, cx| {
                let input = view.install_completion_test_session();
                view.ssh_destination = Some(destination.into());
                view.session_launch =
                    crate::session::LaunchSession::Ssh { host: destination.into() };
                view.cwd = "/synthetic/remote directory".into();
                view.lifecycle_test_ssh_ready(cx);
                view.lifecycle_test_event(TermEvent::Wakeup, cx);
                input
            }));
            old_view = Some(pane.view.clone());
            let focused = pane.id;
            workspace.tabs.push(WorkspaceTab::Terminal {
                panes: vec![pane],
                tree: SplitTree::Leaf(focused),
                focused,
                zoomed: false,
                broadcast: false,
            });
            workspace.tab_meta.push(TabMeta::default());
            workspace
        });
        workspace = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let workspace = workspace.unwrap();
    let old_view = old_view.unwrap();
    let input = input.unwrap();
    window.simulate_resize(size(px(1000.0), px(750.0)));
    draw(window);
    // The opt-in is sampled at exit, and never changes local shell close behavior.
    for remote in [false, true] {
        window.update(|window, cx| {
            cx.global_mut::<crate::gpui_shell::config::Settings>().keep_ssh_session = !remote;
            workspace.update(cx, |workspace, cx| {
                let pane = workspace.new_pane(
                    (80, 24),
                    TerminalLaunch::Local {
                        cwd: None,
                        shell: Some(nebula_terminal::tty::Shell::new(
                            "pebrel-test-missing-shell-executable".into(),
                            vec![],
                        )),
                        shell_name: None,
                    },
                    None,
                    window,
                    cx,
                );
                let view = pane.view.clone();
                view.update(cx, |view, cx| {
                    let _ = view.install_completion_test_session();
                    if remote {
                        view.ssh_destination = Some(destination.into());
                        view.lifecycle_test_ssh_ready(cx);
                    }
                });
                let focused = pane.id;
                workspace.tabs.push(WorkspaceTab::Terminal {
                    panes: vec![pane],
                    tree: SplitTree::Leaf(focused),
                    focused,
                    zoomed: false,
                    broadcast: false,
                });
                workspace.tab_meta.push(TabMeta::default());
                view.update(cx, |view, cx| view.lifecycle_test_event(TermEvent::Exit, cx));
            });
        });
        window.run_until_parked();
        assert_eq!(
            workspace.read_with(window, |workspace, _| workspace.tabs.len()),
            1,
            "local exits close even when enabled; SSH exits close by default"
        );
    }
    window.update(|_, cx| {
        cx.global_mut::<crate::gpui_shell::config::Settings>().keep_ssh_session = true;
    });
    while input.try_recv().is_ok() {}
    old_view.update(window, |view, cx| view.lifecycle_test_event(TermEvent::Exit, cx));
    draw(window);
    workspace.read_with(window, |workspace, cx| {
        assert_eq!(workspace.tabs.len(), 1, "the final SSH pane remains visible");
        let Some(WorkspaceTab::Terminal { panes, tree, focused, .. }) = workspace.tabs.first()
        else {
            panic!()
        };
        assert_eq!(panes.len(), 1);
        assert!(tree.is_leaf());
        assert_eq!(*focused, old_view.read(cx).pane_id);
        let read = old_view.read(cx).runtime_read(1, 24, None).unwrap();
        assert!(read.exited);
        assert!(read.text.contains("retained remote output"));
        let snapshot = workspace.snapshot_session(cx);
        assert_eq!(snapshot.tabs.len(), 1);
        assert_eq!(snapshot.tabs[0].cwd, "/synthetic/remote directory");
        assert!(matches!(snapshot.tabs[0].launch.as_ref(),
            Some(crate::session::LaunchSession::Ssh { host }) if host == destination));
        assert!(matches!(snapshot.tabs[0].layout.as_ref(),
            Some(crate::session::LayoutSession::Pane { cwd, launch: Some(crate::session::LaunchSession::Ssh { host }), custom_name: Some(name), .. })
            if cwd == "/synthetic/remote directory" && host == destination && name == "retained-output"));
    });
    assert!(input.try_recv().is_err(), "exit cannot retry or submit a command");
    let bounds = window.debug_bounds("nebula-btn-ssh-session-reconnect").unwrap();
    assert!(bounds.size.height >= px(32.0));
    window.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
    window.simulate_click(bounds.center(), gpui::Modifiers::default());
    window.run_until_parked();
    workspace.read_with(window, |workspace, cx| {
        assert_eq!(workspace.tabs.len(), 1);
        let Some(WorkspaceTab::Terminal { panes, tree, focused, .. }) = workspace.tabs.first()
        else {
            panic!()
        };
        assert_eq!(panes.len(), 1);
        assert!(tree.is_leaf());
        assert_eq!(*focused, panes[0].id);
        assert_ne!(panes[0].view.entity_id(), old_view.entity_id());
        assert_eq!(panes[0].custom_name.as_deref(), Some("retained-output"));
        assert_eq!(panes[0].name_history, vec![Some("old-name".into())]);
        assert_eq!(panes[0].view.read(cx).ssh_destination.as_deref(), Some(destination));
    });
    let replacement = workspace
        .read_with(window, |workspace, _| workspace.tabs[0].focused_view().unwrap().clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let (mut delivered_events, mut delivered_stages) = (0, 0);
    loop {
        let (events, stages) = draw(window);
        delivered_events += events;
        delivered_stages += stages;
        let failed = replacement.read_with(window, |view, _| {
            view.lifecycle_test_ssh_failed()
                && view.lifecycle_test_grid_contains("SSH connection stopped:")
        });
        if failed && delivered_events > 0 && delivered_stages > 0 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "real reconnect failure was not delivered");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        replacement.read_with(window, |view, _| view.runtime_read(1, 24, None).unwrap_err().code),
        "ssh_not_ready"
    );
    // The replaced entity has no live subscription/leaf and cannot close its successor.
    old_view.update(window, |view, cx| view.lifecycle_test_event(TermEvent::Exit, cx));
    assert_eq!(workspace.read_with(window, |workspace, _| workspace.tabs.len()), 1);
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let view = workspace.tabs[0].focused_view().unwrap().clone();
            workspace.on_terminal_event(&view, &TerminalViewEvent::RequestClose, window, cx);
            assert!(workspace.tabs.is_empty(), "explicit Close still dismisses a retained pane");
        });
    });
}
