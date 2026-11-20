//! Exercise parsed tab launches through the real window dispatcher and PTY owner.
use super::*;
use crate::runtime_api::RuntimeTaskState;
use gpui::TestAppContext;
use serde_json::Value;
use transfer_tests::{initialize_test, open_test_window};

fn isolated(case: &str) -> bool {
    crate::platform::profile_test_isolation::run_exact_child(
        &format!("gpui_shell::workspace::windowing::tab_launch_tests::{case}"),
        None,
    )
}

fn configure(behavior: &str) {
    let path = pebrel_settings::settings_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        pebrel_settings::apply_updates(
            &raw,
            &[
                ("windowing_behavior", behavior.into()),
                ("restore_session", "0".into()),
                ("tray", "0".into()),
                ("fetch", "0".into()),
            ],
        ),
    )
    .unwrap();
}

fn deliver(params: Value, cx: &mut App) -> Result<Value, ApiError> {
    let command = RuntimeCommand::test_request("tab.new", params)?;
    let (dispatch, reply) = RuntimeDispatch::test(command);
    dispatch_runtime(dispatch, cx);
    reply.try_recv().unwrap()
}

fn assert_launch(
    workspace: &Entity<NebulaWorkspace>,
    directory: &std::path::Path,
    argument: &str,
    cx: &App,
) {
    let workspace = workspace.read(cx);
    assert_eq!(
        workspace.tab_meta[workspace.active].launch,
        Some(crate::session::LaunchSession::Command {
            name: "pebrel-test-missing-shell-executable".into(),
            program: "pebrel-test-missing-shell-executable".into(),
            args: vec![argument.into()],
        })
    );
    let view = workspace.tabs[workspace.active].focused_view().unwrap().read(cx);
    assert_eq!(view.local_cwd().as_deref(), Some(directory));
    assert_eq!(view.runtime_task_state(), RuntimeTaskState::Failed);
}

#[gpui::test]
fn runtime_tab_launch_selects_mru_or_explicit_window_and_retains_failed_programs(
    cx: &mut TestAppContext,
) {
    if isolated("runtime_tab_launch_selects_mru_or_explicit_window_and_retains_failed_programs") {
        return;
    }
    configure("use_any_existing");
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        let (first, first_workspace) = open_test_window(cx, 0);
        let (second, second_workspace) = open_test_window(cx, 0);
        let entry = entry_by_id(first, cx).unwrap();
        entry.handle.update(cx, |_, window, cx| mark_active(first, window, cx)).unwrap();
        let params = json!({"cwd": directory.path(), "argv": ["pebrel-test-missing-shell-executable", "two words"]});
        let result = deliver(params.clone(), cx).unwrap();
        assert_eq!(result["action"]["window_id"], first);
        assert_launch(&first_workspace, directory.path(), "two words", cx);
        assert!(second_workspace.read(cx).tabs.is_empty());
        let mut explicit = params.clone();
        explicit["window_id"] = json!(second);
        // Explicit targeting must also override the create-new-window preference.
        configure("use_new");
        let result = deliver(explicit.clone(), cx).unwrap();
        assert_eq!(result["action"]["window_id"], second);
        assert_launch(&second_workspace, directory.path(), "two words", cx);
        explicit["window_id"] = json!(u64::MAX);
        assert_eq!(deliver(explicit, cx).unwrap_err().code, "target_not_found");
        assert_eq!(first_workspace.read(cx).tabs.len(), 1);
        assert_eq!(second_workspace.read(cx).tabs.len(), 1);
        assert_eq!(cx.global::<WindowRegistry>().entries.len(), 2);
    });
}

#[gpui::test]
fn runtime_tab_launch_preserves_command_when_a_window_must_be_created(cx: &mut TestAppContext) {
    if isolated("runtime_tab_launch_preserves_command_when_a_window_must_be_created") {
        return;
    }
    configure("use_any_existing");
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        for behavior in ["use_any_existing", "use_new"] {
            configure(behavior);
            let result = deliver(json!({"cwd": directory.path(), "argv": ["pebrel-test-missing-shell-executable", behavior]}), cx).unwrap();
            let id = result["action"]["window_id"].as_u64().unwrap();
            let workspace = entry_by_id(id, cx).unwrap().workspace.upgrade().unwrap();
            assert_launch(&workspace, directory.path(), behavior, cx);
        }
        assert_eq!(cx.global::<WindowRegistry>().entries.len(), 2);
    });
}

#[gpui::test]
fn runtime_and_initial_launch_apply_wsl_directory_only_on_windows(cx: &mut TestAppContext) {
    if isolated("runtime_and_initial_launch_apply_wsl_directory_only_on_windows") {
        return;
    }
    configure("use_any_existing");
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    // A missing executable exercises the real launch routes without starting WSL.
    let program = directory.path().join("missing").join("wsl").to_str().unwrap().to_owned();
    let command = crate::config::ui_config::Program::WithArgs {
        program: program.clone(),
        args: vec!["literal value".into()],
    };
    let WorkspaceStartup::LaunchTerminal { cwd, launch } =
        startup::initial_startup(Some(directory.path().into()), Some(command), false)
    else {
        panic!("expected a direct startup launch");
    };
    assert_eq!(cwd.as_deref(), Some(directory.path()));
    let crate::session::LaunchSession::Command { program: actual, args, .. } = &launch else {
        panic!("expected the requested command");
    };
    assert_eq!(actual, &program);
    if crate::platform::Platform::current() == crate::platform::Platform::Windows {
        assert_eq!(args, &["--cd", directory.path().to_str().unwrap(), "literal value"]);
    } else {
        assert_eq!(args, &["literal value"]);
    }
    cx.update(|cx| {
        let (id, workspace) = open_test_window(cx, 0);
        deliver(
            json!({"window_id": id, "cwd": directory.path(), "argv": [program, "literal value"]}),
            cx,
        )
        .unwrap();
        let workspace = workspace.read(cx);
        assert_eq!(workspace.tab_meta[workspace.active].launch.as_ref(), Some(&launch));
        let view = workspace.tabs[workspace.active].focused_view().unwrap().read(cx);
        assert_eq!(view.local_cwd().as_deref(), Some(directory.path()));
        assert_eq!(view.runtime_task_state(), RuntimeTaskState::Failed);
    });
}

#[gpui::test]
fn runtime_tab_launch_executes_literal_arguments_in_the_requested_directory(
    cx: &mut TestAppContext,
) {
    if isolated("runtime_tab_launch_executes_literal_arguments_in_the_requested_directory") {
        return;
    }
    configure("use_any_existing");
    cx.background_executor.allow_parking();
    initialize_test(cx);
    let fixture = command_launch_tests::LaunchFixture::new();
    let argv: Vec<_> = std::iter::once(fixture.program.program().to_owned())
        .chain(fixture.program.args().iter().cloned())
        .collect();
    let view = cx.update(|cx| {
        let (id, workspace) = open_test_window(cx, 0);
        let result =
            deliver(json!({"window_id": id, "cwd": fixture.directory, "argv": argv}), cx).unwrap();
        assert_eq!(result["action"]["window_id"], id);
        assert_eq!(workspace.read(cx).tabs.len(), 1);
        workspace.read(cx).tabs[0].focused_view().unwrap().clone()
    });
    let record = fixture.wait_for_record();
    command_launch_tests::stop_reader(cx, &view);
    fixture.verify(record);
}

fn imported_profile(
    directory: &std::path::Path,
    name: &str,
) -> crate::terminal_profiles::TerminalProfile {
    // Discovery can read this PE header; process creation deliberately fails.
    // The routed test inspects the retained launch without starting a guest OS.
    let command = directory.join(name);
    let mut bytes = vec![0_u8; 0x86];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x014c_u16.to_le_bytes());
    std::fs::write(&command, bytes).unwrap();
    crate::terminal_profiles::TerminalProfile {
        id: format!("fixture-{name}"),
        name: name.into(),
        command,
        args: vec!["literal value".into()],
        cwd: None,
        shell_id: None,
        env: Default::default(),
    }
}

#[gpui::test]
fn cli_directory_is_interpreted_by_the_residents_default_or_imported_program(
    cx: &mut TestAppContext,
) {
    if isolated("cli_directory_is_interpreted_by_the_residents_default_or_imported_program") {
        return;
    }
    let root = crate::platform::profile_test_isolation::root();
    let host = root.join("caller directory");
    std::fs::create_dir(&host).unwrap();
    let profiles =
        [imported_profile(&root, "wsl.exe"), imported_profile(&root, "native-profile.exe")];
    let mut store = crate::terminal_profiles::TerminalProfiles::default();
    for profile in &profiles {
        store.upsert(profile.clone()).unwrap();
    }
    store.save().unwrap();
    let raw_directory = crate::platform::launch_directory::test_ambiguous_cli_directory(&host);
    let resident_directory = std::env::current_dir().unwrap();
    let windows = crate::platform::Platform::current() == crate::platform::Platform::Windows;
    configure("use_any_existing");
    // The resident's live default intentionally differs from the saved CLI settings.
    assert!(pebrel_settings::RuntimeSettings::load().shell.is_none());
    initialize_test(cx);
    cx.update(|cx| {
        cx.set_global(crate::gpui_shell::config::Settings::load(pebrel_settings::ThemeName::Nord));
        let mut first_window = None;
        for route in ["fallback", "existing", "new"] {
            configure(if route == "fallback" { "use_any_existing" } else { "use_new" });
            for profile in &profiles {
                let id = format!("profile:custom|{}", profile.id);
                cx.global_mut::<crate::gpui_shell::config::Settings>().shell_id = Some(id.clone());
                for explicit in [false, true] {
                    std::env::set_current_dir(&host).unwrap();
                    let params =
                        crate::runtime_api::test_tab_launch_params(crate::cli::TabLaunchOptions {
                            window: if route == "existing" { first_window } else { None },
                            cwd: Some(raw_directory.clone()),
                            shell: explicit.then_some(id.clone()),
                            command: Vec::new(),
                        });
                    std::env::set_current_dir(&resident_directory).unwrap();
                    if windows {
                        assert_eq!(params["cwd"], raw_directory.to_str().unwrap());
                        let alternate = PathBuf::from(params["host_cwd"].as_str().unwrap());
                        assert_eq!(alternate.canonicalize().unwrap(), host.canonicalize().unwrap());
                    } else {
                        assert!(params.get("host_cwd").is_none());
                    }
                    if windows && profile.name == "wsl.exe" {
                        let before = cx.global::<WindowRegistry>().entries.len();
                        for alternate in [true, false] {
                            let mut invalid = params.clone();
                            invalid["cwd"] = json!("/guest\"directory");
                            if !alternate {
                                invalid.as_object_mut().unwrap().remove("host_cwd");
                            }
                            assert_eq!(deliver(invalid, cx).unwrap_err().code, "invalid_params");
                            assert_eq!(cx.global::<WindowRegistry>().entries.len(), before);
                        }
                    }
                    let result = deliver(params, cx).unwrap();
                    let window_id = result["action"]["window_id"].as_u64().unwrap();
                    if route == "existing" {
                        assert_eq!(Some(window_id), first_window);
                    }
                    if route == "new" {
                        assert_ne!(Some(window_id), first_window);
                    }
                    first_window.get_or_insert(window_id);
                    let workspace =
                        entry_by_id(window_id, cx).unwrap().workspace.upgrade().unwrap();
                    let workspace = workspace.read(cx);
                    let Some(crate::session::LaunchSession::Profile {
                        command,
                        args,
                        cwd,
                        profile_id,
                        ..
                    }) = workspace.tab_meta[workspace.active].launch.as_ref()
                    else {
                        panic!("resolved profile identity was discarded")
                    };
                    assert_eq!(command, profile.command.to_str().unwrap());
                    assert_eq!(profile_id.as_deref(), Some(profile.id.as_str()));
                    if windows && profile.name == "wsl.exe" {
                        assert_eq!(cwd.as_deref(), raw_directory.to_str());
                        assert_eq!(
                            args,
                            &["--cd", &format!("\"{}\"", raw_directory.display()), "literal value"]
                        );
                    } else {
                        assert_eq!(
                            PathBuf::from(cwd.as_ref().unwrap()).canonicalize().unwrap(),
                            host.canonicalize().unwrap()
                        );
                        assert_eq!(args, &profile.args);
                    }
                    let view = workspace.tabs[workspace.active].focused_view().unwrap().read(cx);
                    assert_eq!(view.runtime_task_state(), RuntimeTaskState::Failed);
                }
            }
        }
    });
}

#[gpui::test]
fn elevated_directory_handover_uses_the_imported_default_profile(cx: &mut TestAppContext) {
    use crate::platform::elevation::handover::{Dispatch, Request};
    use crate::session::LaunchSession;

    if isolated("elevated_directory_handover_uses_the_imported_default_profile") {
        return;
    }
    let root = crate::platform::profile_test_isolation::root();
    let mut profile = imported_profile(&root, "pwsh.exe");
    profile.shell_id = Some("pwsh".into());
    let mut store = crate::terminal_profiles::TerminalProfiles::default();
    store.upsert(profile.clone()).unwrap();
    store.save().unwrap();
    configure("use_any_existing");
    initialize_test(cx);
    cx.update(|cx| {
        let (_, workspace) = open_test_window(cx, 0);
        let mut settings =
            crate::gpui_shell::config::Settings::load(pebrel_settings::ThemeName::Nord);
        settings.shell_id = Some(format!("profile:pwsh|{}", profile.id));
        cx.set_global(settings);
        let (dispatch, reply) = Dispatch::new(Request {
            cwd: Some(root.clone()),
            command: None,
            shell_id: None,
            update_restore: None,
        });
        dispatch_shell_events(vec![GpuiShellEvent::ElevatedLaunch(dispatch)], cx);
        reply.try_recv().unwrap().unwrap();
        let workspace = workspace.read(cx);
        let metadata = workspace.meta(workspace.active);
        let Some(LaunchSession::Profile { command, args, profile_id, cwd, .. }) = &metadata.launch
        else {
            panic!("elevated directory launch lost the imported profile")
        };
        assert_eq!(profile_id.as_deref(), Some(profile.id.as_str()));
        assert_eq!(command, profile.command.to_str().unwrap());
        assert_eq!(args, &profile.args);
        assert_eq!(cwd.as_deref(), root.to_str());
        assert_eq!(metadata.shell_tag.as_deref(), Some("pwsh"));
    });
}
