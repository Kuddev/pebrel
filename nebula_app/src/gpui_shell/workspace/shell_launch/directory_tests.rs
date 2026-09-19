use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use crate::gpui_shell::workspace::NebulaWorkspace;
use crate::session::LaunchSession;

#[test]
fn explicit_directory_reaches_the_terminal_without_changing_the_saved_profile() {
    let saved = tempfile::tempdir().unwrap();
    let requested = tempfile::tempdir().unwrap();
    let profile = LaunchSession::Profile {
        name: "Project shell".into(),
        command: "custom-shell".into(),
        args: vec!["--login".into()],
        cwd: Some(saved.path().to_string_lossy().into_owned()),
        shell_id: Some("custom".into()),
    };
    let snapshot = at_explicit_directory(profile.clone(), Some(requested.path()));
    let TerminalLaunch::Local { cwd, shell, .. } =
        NebulaWorkspace::terminal_launch_from_session(&snapshot, Some(requested.path().into()))
    else {
        panic!("expected a local terminal");
    };
    assert_eq!(cwd.as_deref(), Some(requested.path()));
    let shell = shell.unwrap();
    assert_eq!(shell.program(), "custom-shell");
    assert_eq!(shell.args(), &["--login"]);

    // Starting the same saved profile again without an override keeps its default.
    let unchanged = at_explicit_directory(profile.clone(), None);
    assert_eq!(unchanged, profile);
    let TerminalLaunch::Local { cwd, .. } =
        NebulaWorkspace::terminal_launch_from_session(&unchanged, None)
    else {
        panic!("expected a local terminal");
    };
    assert_eq!(cwd.as_deref(), Some(saved.path()));
}

#[test]
fn explicit_host_directory_replaces_wsl_profile_directory_and_keeps_guest_arguments() {
    let requested = tempfile::tempdir().unwrap();
    let args = [
        "-d",
        "Team Linux",
        "--cd",
        "/old",
        "--user",
        "guest",
        "--cd=/older",
        "--exec",
        "zsh",
        "--cd",
        "/guest-option",
    ]
    .map(String::from)
    .to_vec();
    for launch in [
        LaunchSession::Profile {
            name: "WSL project".into(),
            command: "wsl.exe".into(),
            args: args.clone(),
            cwd: None,
            shell_id: Some("wsl:Team Linux".into()),
        },
        LaunchSession::Shell {
            name: "WSL project".into(),
            program: "wsl.exe".into(),
            args: args.clone(),
        },
    ] {
        assert_eq!(at_explicit_directory(launch.clone(), None), launch);
        let snapshot = at_explicit_directory(launch, Some(requested.path()));
        let TerminalLaunch::Local { cwd, shell, .. } =
            NebulaWorkspace::terminal_launch_from_session(&snapshot, Some(requested.path().into()))
        else {
            panic!("expected WSL to launch through the local PTY");
        };
        assert_eq!(cwd.as_deref(), Some(requested.path()));
        assert_eq!(
            shell.unwrap().args(),
            &["-d", "Team Linux", "--user", "guest", "--exec", "zsh", "--cd", "/guest-option",]
        );
    }
}

#[test]
fn directory_override_cannot_turn_an_invalid_shell_request_into_a_default_shell() {
    let requested = tempfile::tempdir().unwrap();
    assert!(resolve_shell_at("pebrel-missing-shell-id", Some(requested.path())).is_err());
}
