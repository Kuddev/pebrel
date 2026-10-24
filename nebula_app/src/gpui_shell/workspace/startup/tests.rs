use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn configure() -> impl Drop {
    nebula_settings::persist_keys(&[
        ("terminal_startup", "welcome".into()),
        ("restore_session", "0".into()),
        ("new_tab_position", "end".into()),
        ("tabs_position", "top".into()),
        ("startup_directory", String::new()),
        ("shell", crate::platform::shell::default_shell_id()),
        ("language", "en-US".into()),
    ])
    .unwrap();
    crate::gpui_shell::terminal::session::spawn_fixture::fail()
}

fn open_workspace(
    cx: &mut TestAppContext,
    startup: windowing::WorkspaceStartup,
) -> (Entity<NebulaWorkspace>, VisualTestContext) {
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        windowing::initialize(cx, hub.clone());
    });
    let mut workspace_out = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                hub,
                startup,
                windowing::WindowRole::Regular,
                cx,
            )
        });
        workspace_out = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    window.run_until_parked();
    window.simulate_resize(gpui::size(px(1000.0), px(700.0)));
    (workspace_out.unwrap(), window.clone())
}

fn bounds(window: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    window.debug_bounds(selector).expect("welcome control is visible")
}

#[gpui::test]
fn welcome_windows_and_new_tabs_remain_without_terminal_processes(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    let (workspace, mut window) = open_workspace(cx, windowing::WorkspaceStartup::RestoreOrDefault);
    let next = workspace.read_with(&window, |workspace, _| workspace.next_pane_id);
    window.update(|window, cx| window.dispatch_action(Box::new(super::super::NewTerminal), cx));
    window.run_until_parked();
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert_eq!(workspace.tabs.len(), 2);
            assert_eq!(workspace.next_pane_id, next);
            assert!(workspace.tabs.iter().all(|tab| matches!(tab, WorkspaceTab::Welcome { .. })));
            assert!(workspace.snapshot_session(cx).tabs.is_empty());
            let (_, snapshot) = workspace.runtime_window_snapshot(window, cx);
            assert!(snapshot.focused_pane_id.is_none());
            assert!(snapshot.tabs.iter().all(|tab| tab.kind == "welcome" && tab.panes.is_empty()));
            workspace.close_tab(0, window, cx);
            assert_eq!(workspace.tabs.len(), 1);
            assert!(workspace.active_welcome().is_some());
        })
    });
    let local = bounds(&mut window, "welcome-local");
    assert!(local.size.width > px(300.0) && local.size.height == px(48.0));
}

#[gpui::test]
fn local_choice_preserves_position_and_identity_and_rejects_retired_view_events(
    cx: &mut TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    let directory = tempfile::tempdir().unwrap();
    let (workspace, mut window) = open_workspace(cx, windowing::WorkspaceStartup::Empty);
    let (source, identity, expected_launch) = window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            for _ in 0..3 {
                workspace.add_welcome(Some(directory.path().into()), window, cx);
            }
            workspace.tab_meta[1].custom_name = Some("My session".into());
            workspace.activate_tab(1, window, cx);
            (
                workspace.active_welcome().unwrap(),
                workspace.meta(1).runtime_id,
                shell_launch::configured_local_launch(cx),
            )
        })
    });
    let local = bounds(&mut window, "welcome-local");
    // The entire 48px row, including its left padding, starts the selected session.
    window.simulate_click(
        gpui::point(local.left() + px(3.0), local.center().y),
        Modifiers::default(),
    );
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert_eq!(workspace.tabs.len(), 3);
            assert!(workspace.tabs[1].is_terminal());
            assert_eq!(workspace.active, 1);
            assert_eq!(workspace.meta(1).runtime_id, identity);
            assert_eq!(workspace.meta(1).custom_name.as_deref(), Some("My session"));
            assert_eq!(workspace.meta(1).launch, Some(expected_launch));
            assert_eq!(
                workspace.tabs[1].focused_view().unwrap().read(cx).local_cwd().as_deref(),
                Some(directory.path())
            );
            let next = workspace.next_pane_id;
            workspace.activate_tab(2, window, cx);
            workspace.on_welcome_event(&source, &WelcomeEvent::Local, window, cx);
            assert_eq!(workspace.next_pane_id, next);
            assert_eq!(workspace.active, 2);
            assert!(workspace.active_welcome().is_some());
            workspace.close_tab(1, window, cx);
        })
    });
}

#[gpui::test]
fn ssh_choice_reaches_existing_host_controls_and_return_keeps_the_same_welcome(
    cx: &mut TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    let (workspace, mut window) =
        open_workspace(cx, windowing::WorkspaceStartup::Configured { cwd: None });
    let (identity, next) = workspace
        .read_with(&window, |workspace, _| (workspace.meta(0).runtime_id, workspace.next_pane_id));
    let ssh = bounds(&mut window, "welcome-ssh");
    window.simulate_click(ssh.center(), Modifiers::default());
    bounds(&mut window, "ssh-library-controls");
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.settings_open);
            assert_eq!(workspace.next_pane_id, next);
            workspace.close_settings(window, cx);
            assert_eq!(workspace.meta(0).runtime_id, identity);
            assert!(workspace.active_welcome().is_some());
            assert_eq!(workspace.tabs.len(), 1);
        })
    });
    bounds(&mut window, "welcome-local");
}

#[gpui::test]
fn choosing_an_ssh_host_promotes_the_welcome_tab_in_place(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    let destination = "welcome-fixture@localhost:2222";
    nebula_settings::persist_keys(&[("saved_hosts", destination.into())]).unwrap();
    let (workspace, mut window) =
        open_workspace(cx, windowing::WorkspaceStartup::Configured { cwd: None });
    let identity = workspace.read_with(&window, |workspace, _| workspace.meta(0).runtime_id);
    let ssh = bounds(&mut window, "welcome-ssh");
    window.simulate_click(ssh.center(), Modifiers::default());
    let connect = bounds(&mut window, "ssh-connect-0");
    window.simulate_click(connect.center(), Modifiers::default());
    window.run_until_parked();
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(!workspace.settings_open);
            assert_eq!(workspace.tabs.len(), 1);
            assert_eq!(workspace.meta(0).runtime_id, identity);
            assert_eq!(
                workspace.meta(0).launch,
                Some(crate::session::LaunchSession::Ssh { host: destination.into() })
            );
            assert!(workspace.tabs[0].is_terminal());
            workspace.add_welcome(None, window, cx);
            workspace.close_tab(0, window, cx);
        })
    });
}

#[gpui::test]
fn explicit_launches_and_saved_sessions_keep_their_launch_identity(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    let directory = tempfile::tempdir().unwrap();
    let launch = crate::session::LaunchSession::Shell {
        name: "Explicit missing shell".into(),
        program: directory.path().join("missing-shell-executable").to_string_lossy().into_owned(),
        args: vec!["--literal=two words".into()],
    };
    let (workspace, mut window) = open_workspace(
        cx,
        windowing::WorkspaceStartup::LaunchTerminal {
            cwd: Some(directory.path().into()),
            launch: launch.clone(),
        },
    );
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.tabs[0].is_terminal());
            assert_eq!(workspace.meta(0).launch.as_ref(), Some(&launch));
            let saved = workspace.snapshot_session(cx);
            workspace.remove_tab_at(0);
            workspace.initialize_tabs(
                windowing::WorkspaceStartup::RestoreUpdate(saved),
                &RuntimeSettings::load(),
                window,
                cx,
            );
            assert_eq!(workspace.tabs.len(), 1);
            assert!(workspace.tabs[0].is_terminal());
            assert_eq!(workspace.meta(0).launch.as_ref(), Some(&launch));
        })
    });
}

#[gpui::test]
fn runtime_terminal_and_quick_window_requests_bypass_the_welcome_preference(
    cx: &mut TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    let (workspace, mut window) =
        open_workspace(cx, windowing::WorkspaceStartup::NewTerminal { cwd: None });
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.tabs[0].is_terminal());
            workspace.add_welcome(None, window, cx);
            workspace.close_tab(0, window, cx);
            workspace.window_role = windowing::WindowRole::QuickTerminal;
            workspace.initialize_tabs(
                windowing::WorkspaceStartup::Configured { cwd: None },
                &RuntimeSettings::load(),
                window,
                cx,
            );
            assert_eq!(workspace.tabs.len(), 2);
            assert!(workspace.tabs[workspace.active].is_terminal());
            workspace.close_tab(workspace.active, window, cx);
        })
    });
}

#[gpui::test]
fn keyboard_can_choose_both_actions_in_a_narrow_chinese_window(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    nebula_settings::persist_keys(&[("language", "zh-CN".into())]).unwrap();
    let (workspace, mut window) =
        open_workspace(cx, windowing::WorkspaceStartup::Configured { cwd: None });
    window.simulate_resize(gpui::size(px(420.0), px(400.0)));
    let page = bounds(&mut window, "terminal-welcome");
    for selector in ["welcome-local", "welcome-ssh"] {
        let control = bounds(&mut window, selector);
        assert!(control.left() >= page.left() && control.right() <= page.right());
        assert!(control.top() >= page.top() && control.bottom() <= page.bottom());
    }
    window.simulate_keystrokes("tab tab enter");
    window.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    window.run_until_parked();
    assert!(workspace.read_with(&window, |workspace, _| workspace.settings_open));
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.close_settings(window, cx);
        })
    });
    bounds(&mut window, "welcome-local");
    window.simulate_keystrokes("tab enter");
    window.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    window.run_until_parked();
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert_eq!(workspace.tabs.len(), 1);
            assert!(workspace.tabs[0].is_terminal());
            workspace.add_welcome(None, window, cx);
            workspace.close_tab(0, window, cx);
        })
    });
}

#[gpui::test]
fn runtime_tab_requests_only_use_welcome_when_explicitly_requested(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let _spawn = configure();
    let directory = tempfile::tempdir().unwrap();
    let (workspace, mut window) = open_workspace(cx, windowing::WorkspaceStartup::Empty);
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            for use_startup_setting in [true, false] {
                let result = workspace
                    .execute_runtime_command(
                        &crate::runtime_api::RuntimeCommand::NewTab {
                            window_id: Some(1),
                            cwd: Some(directory.path().into()),
                            shell_id: None,
                            use_startup_setting,
                        },
                        window,
                        cx,
                    )
                    .unwrap();
                assert_eq!(result["action"]["pane_id"].is_null(), use_startup_setting);
                assert_eq!(workspace.active_welcome().is_some(), use_startup_setting);
            }
            workspace.close_tab(workspace.active, window, cx);
            let WorkspaceTab::Welcome { cwd, .. } = &workspace.tabs[0] else {
                panic!("configured request did not retain its welcome page");
            };
            assert_eq!(cwd.as_deref(), Some(directory.path()));
        })
    });
}
