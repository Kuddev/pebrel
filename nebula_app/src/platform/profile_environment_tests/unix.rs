//! Real concurrent local PTYs through workspace launch, duplicate and split paths.
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use std::time::{Duration, Instant};

fn profile(
    name: &str,
    value: &str,
    output: &Path,
    global_override: Option<&str>,
) -> crate::config::ui_config::Profile {
    let mut variables = vec![
        ("QA_LOCAL_ID".into(), value.into()),
        ("QA_EMPTY".into(), String::new()),
        ("QA_OUTPUT".into(), output.to_string_lossy().into_owned()),
    ];
    if let Some(value) = global_override {
        variables.push(("QA_GLOBAL_ONLY".into(), value.into()));
    }
    // Keep fixture script text out of exported fallback argv; the environment
    // name/value assertions must inspect the overlay rather than a command literal.
    let script = output.parent().unwrap().join("profile-child.sh");
    std::fs::write(&script, "printf '%s|%s|%s\\n' \"$QA_LOCAL_ID\" \"$QA_GLOBAL_ONLY\" \"${QA_EMPTY+x}:$QA_EMPTY\" >> \"$QA_OUTPUT\"; while IFS= read -r line; do [ \"$line\" = stop ] && exit; done").unwrap();
    crate::config::ui_config::Profile {
        name: name.into(),
        command: "/bin/sh".into(),
        args: vec![script.to_string_lossy().into_owned()],
        cwd: output.parent().map(Path::to_path_buf),
        shell_id: Some("sh".into()),
        terminal_profile_id: Some(format!("qa-{name}")),
        env: crate::profile_environment::ProfileEnvironment::from_pairs(variables).unwrap(),
    }
}

// The native PTY and production mailbox/parser stay real. Drain on the
// owning foreground; the pinned TestScheduler rejects native-thread wakes.
fn drain_local_ptys(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|_, cx| crate::gpui_shell::terminal::LocalPtyPump::drain(cx));
}

fn wait_lines(path: &Path, count: usize, cx: &mut VisualTestContext) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        drain_local_ptys(cx);
        let lines: Vec<String> =
            std::fs::read_to_string(path).unwrap_or_default().lines().map(String::from).collect();
        if lines.len() >= count {
            return lines;
        }
        assert!(Instant::now() < deadline, "real profile child output timeout: {lines:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn local_profile_environment_real_ptys_preserve_independent_overlays_on_duplicate_and_split(
    cx: &mut TestAppContext,
) {
    if crate::platform::profile_test_isolation::run_exact_child(
        "gpui_shell::workspace::profile_environment_tests::unix::local_profile_environment_real_ptys_preserve_independent_overlays_on_duplicate_and_split",
        None,
    ) {
        return;
    }

    let original_process_value = std::env::var_os("QA_LOCAL_ID");
    cx.update(|cx| {
        cx.set_global(crate::gpui_shell::terminal::LocalPtyPump::default());
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::workspace::init(cx);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
        let mut settings =
            crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
        settings.env.insert("QA_GLOBAL_ONLY".into(), "global".into());
        cx.set_global(settings);
    });
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let output = root.join("children.txt");
    let alpha = profile("Alpha", "alpha", &output, Some("profile"));
    let beta = profile("Beta", "beta", &output, None);
    let alpha_launch = crate::profile_launch::snapshot(alpha.clone());
    let beta_launch = crate::profile_launch::snapshot(beta);
    let mut owner = None;
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
            workspace.add_terminal_with(alpha_launch.clone(), Some(root.clone()), None, window, cx);
            workspace.add_terminal_with(beta_launch.clone(), Some(root.clone()), None, window, cx);
            workspace.activate_tab(0, window, cx);
            workspace.split_focused(SplitDirection::LeftRight, window, cx).unwrap();
            workspace.duplicate_tab(0, window, cx);
            workspace
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let workspace = owner.unwrap();
    let lines = wait_lines(&output, 5, window);
    assert_eq!(lines.iter().filter(|line| *line == "alpha|profile|x:").count(), 4);
    assert_eq!(lines.iter().filter(|line| *line == "beta|global|x:").count(), 1);
    // Editing the profile is a new launch; existing panes retain their own overlay.
    let mut edited = alpha;
    edited.env.0.insert("QA_LOCAL_ID".into(), "edited".into());
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.add_terminal_with(
                crate::profile_launch::snapshot(edited),
                Some(root.clone()),
                None,
                window,
                cx,
            );
            for tab in &workspace.tabs {
                if let WorkspaceTab::Terminal { panes, .. } = tab {
                    for pane in panes {
                        let view = pane.view.read(cx);
                        assert!(view.session.is_some(), "real local PTY must spawn");
                        assert!(matches!(
                            view.session_launch,
                            crate::session::LaunchSession::Profile { env_snapshot: Some(_), .. }
                        ));
                        assert!(view.exec_context.is_some());
                    }
                }
            }
        })
    });
    let lines = wait_lines(&output, 6, window);
    assert_eq!(lines.iter().filter(|line| *line == "edited|profile|x:").count(), 1);
    assert_eq!(lines.iter().filter(|line| *line == "alpha|profile|x:").count(), 4);
    window.update(|_, cx| {
        workspace.read(cx).tabs.iter().for_each(|tab| {
            if let WorkspaceTab::Terminal { panes, .. } = tab {
                for pane in panes {
                    pane.view.read(cx).shutdown();
                }
            }
        })
    });
    assert_eq!(
        std::env::var_os("QA_LOCAL_ID"),
        original_process_value,
        "profile must not mutate process environment"
    );
}

#[gpui::test]
fn local_profile_cold_restore_uses_current_store_and_running_duplicates_keep_snapshot(
    cx: &mut TestAppContext,
) {
    if crate::platform::profile_test_isolation::run_exact_child(
        "gpui_shell::workspace::profile_environment_tests::unix::local_profile_cold_restore_uses_current_store_and_running_duplicates_keep_snapshot",
        Some(("children.txt", 3)),
    ) {
        return;
    }
    let directory = crate::platform::profile_test_isolation::root();
    assert_eq!(crate::platform::dirs::data_dir(), directory.join("config"));
    cx.update(|cx| {
        cx.set_global(crate::gpui_shell::terminal::LocalPtyPump::default());
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::workspace::init(cx);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
        let mut settings =
            crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
        settings.env.insert("QA_GLOBAL_ONLY".into(), "global".into());
        cx.set_global(settings);
    });
    let output = directory.join("children.txt");
    let original = profile("Restore", "original", &output, None);
    let frozen = crate::profile_launch::snapshot(original.clone());
    let mut tab =
        crate::session::TabSession::single(directory.to_string_lossy().into_owned(), None, None);
    tab.launch = Some(frozen.clone());
    let exported = serde_json::to_string(&tab).unwrap();
    assert!(!exported.contains("QA_LOCAL_ID"));
    assert!(!exported.contains("original"));
    let restored: crate::session::TabSession = serde_json::from_str(&exported).unwrap();
    let mut saved = crate::terminal_profiles::TerminalProfile {
        id: original.terminal_profile_id.clone().unwrap(),
        name: original.name.clone(),
        command: original.command.into(),
        args: original.args.clone(),
        cwd: original.cwd.clone(),
        shell_id: original.shell_id,
        env: original.env.clone(),
    };
    saved.env.0.insert("QA_LOCAL_ID".into(), "current".into());
    crate::terminal_profiles::TerminalProfiles::edit_at(
        &crate::terminal_profiles::store_path(),
        None,
        Some(saved),
    )
    .unwrap();
    let mut owner = None;
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
            workspace.add_terminal_with(frozen, Some(directory.clone()), None, window, cx);
            assert!(workspace.restore_tab(&restored, false, window, cx));
            workspace.duplicate_tab(0, window, cx);
            workspace
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let lines = wait_lines(&output, 3, window);
    assert_eq!(lines.iter().filter(|line| *line == "original|global|x:").count(), 2);
    assert_eq!(lines.iter().filter(|line| *line == "current|global|x:").count(), 1);
    let workspace = owner.unwrap();
    window.update(|_, cx| {
        workspace.read(cx).tabs.iter().for_each(|tab| {
            if let WorkspaceTab::Terminal { panes, .. } = tab {
                for pane in panes {
                    pane.view.read(cx).shutdown();
                }
            }
        })
    });
}

#[gpui::test]
fn local_profile_split_real_pty_follows_reported_current_directory(cx: &mut TestAppContext) {
    if crate::platform::profile_test_isolation::run_exact_child(
        "gpui_shell::workspace::profile_environment_tests::unix::local_profile_split_real_pty_follows_reported_current_directory",
        None,
    ) {
        return;
    }

    use nebula_terminal::event::Notify as _;
    cx.update(|cx| {
        cx.set_global(crate::gpui_shell::terminal::LocalPtyPump::default());
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::workspace::init(cx);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let initial = root.join("initial");
    let current = root.join("current");
    std::fs::create_dir(&initial).unwrap();
    std::fs::create_dir(&current).unwrap();
    let output = root.join("cwd.txt");
    let mut profile = profile("Cwd", "frozen-overlay", &output, None);
    profile.cwd = Some(initial.clone());
    profile.env.0.insert("QA_NEXT_CWD".into(), current.to_string_lossy().into_owned());
    profile.args = vec!["-c".into(), "printf '%s|%s\\n' \"$PWD\" \"$QA_LOCAL_ID\" >> \"$QA_OUTPUT\"; while IFS= read -r line; do eval \"$line\"; done".into()];
    let mut owner = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
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
        workspace.update(cx, |workspace, cx| {
            workspace.add_terminal_with(
                crate::profile_launch::snapshot(profile),
                Some(initial.clone()),
                None,
                window,
                cx,
            )
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let workspace = owner.unwrap();
    let view = workspace
        .read_with(window, |workspace, _| workspace.tabs[0].focused_view().unwrap().clone());
    assert_eq!(wait_lines(&output, 1, window), [format!("{}|frozen-overlay", initial.display())]);
    window.update(|_, cx| {
        view.read(cx).session.as_ref().unwrap().notifier.notify(
            b"cd \"$QA_NEXT_CWD\"; printf '\\033]7;file://localhost%s\\007' \"$PWD\"; printf '%s|%s\\n' \"$PWD\" \"$QA_LOCAL_ID\" >> \"$QA_OUTPUT\"\n".to_vec());
    });
    assert_eq!(wait_lines(&output, 2, window)[1], format!("{}|frozen-overlay", current.display()));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        drain_local_ptys(window);
        if view.read_with(window, |view, _| view.local_cwd()).as_deref() == Some(current.as_path())
        {
            break;
        }
        assert!(Instant::now() < deadline, "real OSC 7 cwd report did not reach the pane");
        std::thread::sleep(Duration::from_millis(10));
    }
    window.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.split_focused(SplitDirection::LeftRight, window, cx).unwrap();
        })
    });
    let lines = wait_lines(&output, 3, window);
    assert_eq!(
        lines[2],
        format!("{}|frozen-overlay", current.display()),
        "split spawns in current cwd with the same environment"
    );
    window.update(|_, cx| {
        if let WorkspaceTab::Terminal { panes, .. } = &workspace.read(cx).tabs[0] {
            for pane in panes {
                pane.view.read(cx).shutdown();
            }
        }
    });
}
