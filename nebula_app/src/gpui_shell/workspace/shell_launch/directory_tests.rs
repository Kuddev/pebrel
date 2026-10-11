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
        profile_id: None,
        env_snapshot: None,
    };
    let snapshot = at_explicit_directory(profile.clone(), Some(requested.path()));
    let TerminalLaunch::Profile { cwd, profile: resolved } =
        NebulaWorkspace::terminal_launch_from_session(&snapshot, Some(requested.path().into()))
    else {
        panic!("expected a local terminal");
    };
    assert_eq!(cwd.as_deref(), Some(requested.path()));
    let shell = resolved.shell();
    assert_eq!(shell.program(), "custom-shell");
    assert_eq!(shell.args(), &["--login"]);

    // Starting the same saved profile again without an override keeps its default.
    let unchanged = at_explicit_directory(profile.clone(), None);
    assert_eq!(unchanged, profile);
    let TerminalLaunch::Profile { cwd, .. } =
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
            profile_id: None,
            env_snapshot: None,
        },
        LaunchSession::Shell {
            name: "WSL project".into(),
            program: "wsl.exe".into(),
            args: args.clone(),
        },
    ] {
        assert_eq!(at_explicit_directory(launch.clone(), None), launch);
        let snapshot = at_explicit_directory(launch, Some(requested.path()));
        let (cwd, shell) = match NebulaWorkspace::terminal_launch_from_session(
            &snapshot,
            Some(requested.path().into()),
        ) {
            TerminalLaunch::Local { cwd, shell, .. } => (cwd, shell.unwrap()),
            TerminalLaunch::Profile { cwd, profile } => (cwd, profile.shell()),
            _ => panic!("expected WSL to launch through the local PTY"),
        };
        assert_eq!(cwd.as_deref(), Some(requested.path()));
        assert_eq!(
            shell.args(),
            &["-d", "Team Linux", "--user", "guest", "--exec", "zsh", "--cd", "/guest-option",]
        );
    }
}

#[test]
fn directory_override_cannot_turn_an_invalid_shell_request_into_a_default_shell() {
    let requested = tempfile::tempdir().unwrap();
    assert!(resolve_shell_at("pebrel-missing-shell-id", Some(requested.path())).is_err());
}

#[test]
fn wsl_home_launch_ignores_host_fallback_and_yields_to_explicit_host_directory() {
    let host = tempfile::tempdir().unwrap();
    let profile = crate::config::ui_config::Profile {
        name: "Synthetic WSL".into(),
        command: r"C:\Windows\System32\wsl.exe".into(),
        args: ["-d", "Team Linux", "--user", "guest"].map(String::from).to_vec(),
        cwd: None,
        shell_id: Some("wsl:Team Linux".into()),
        terminal_profile_id: Some("synthetic-home-id".into()),
        env: Default::default(),
    };
    let profile = crate::profile_launch::snapshot(profile);
    let args = ["--cd", "~", "-d", "Team Linux", "--user", "guest"].map(String::from).to_vec();
    let shell = LaunchSession::Shell {
        name: "Synthetic WSL".into(),
        program: r"C:\Windows\System32\wsl.exe".into(),
        args,
    };
    for launch in [profile, shell] {
        let home = NebulaWorkspace::terminal_launch_from_session(&launch, Some(host.path().into()));
        let (cwd, home_shell) = match home {
            TerminalLaunch::Profile { cwd, profile } => (cwd, profile.shell()),
            TerminalLaunch::Local { cwd, shell: Some(shell), .. } => (cwd, shell),
            _ => panic!("expected WSL local adapter"),
        };
        assert!(cwd.is_none(), "an inherited host path is not the guest home");
        assert_eq!(home_shell.args(), &["--cd", "~", "-d", "Team Linux", "--user", "guest"]);
        let explicit = at_explicit_directory(launch, Some(host.path()));
        let launch =
            NebulaWorkspace::terminal_launch_from_session(&explicit, Some(host.path().into()));
        let (cwd, shell) = match launch {
            TerminalLaunch::Profile { cwd, profile } => (cwd, profile.shell()),
            TerminalLaunch::Local { cwd, shell: Some(shell), .. } => (cwd, shell),
            _ => panic!("expected explicit WSL local adapter"),
        };
        assert_eq!(cwd.as_deref(), Some(host.path()));
        assert_eq!(shell.args(), &["-d", "Team Linux", "--user", "guest"]);
    }
}
