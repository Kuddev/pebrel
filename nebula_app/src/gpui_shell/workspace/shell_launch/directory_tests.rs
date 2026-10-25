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
            &[
                "--cd",
                requested.path().to_str().unwrap(),
                "-d",
                "Team Linux",
                "--user",
                "guest",
                "--exec",
                "zsh",
                "--cd",
                "/guest-option",
            ]
        );
    }
}

#[test]
fn directory_override_cannot_turn_an_invalid_shell_request_into_a_default_shell() {
    let requested = tempfile::tempdir().unwrap();
    assert!(resolve_shell_at("pebrel-missing-shell-id", Some(requested.path())).is_err());
}

#[test]
fn explicit_relative_host_directory_is_absolute_before_wsl_receives_it() {
    let launch =
        LaunchSession::Shell { name: "WSL".into(), program: "wsl.exe".into(), args: Vec::new() };
    let LaunchSession::Shell { args, .. } = at_explicit_directory(launch, Some(Path::new(".")))
    else {
        panic!("expected shell")
    };
    let directory = std::path::absolute(".").unwrap();
    let value = directory.to_str().unwrap();
    let encoded =
        if value.contains([' ', '\t']) { format!("\"{value}\"") } else { value.to_owned() };
    assert_eq!(args, vec!["--cd".to_owned(), encoded]);
}

#[test]
fn explicit_guest_directory_survives_to_the_wsl_launch() {
    let requested = Path::new("/home/pebrel-fixture/project space");
    let launch = LaunchSession::Shell {
        name: "WSL".into(),
        program: "wsl.exe".into(),
        args: vec!["-d".into(), "Ubuntu".into(), "--user".into(), "guest".into()],
    };
    let snapshot = at_explicit_directory(launch, Some(requested));
    let TerminalLaunch::Local { cwd, shell, .. } =
        NebulaWorkspace::terminal_launch_from_session(&snapshot, Some(requested.into()))
    else {
        panic!("expected local WSL launch")
    };
    let mut options =
        nebula_terminal::tty::Options { shell, working_directory: cwd, ..Default::default() };
    crate::platform::launch_directory::prepare_wsl(&mut options, Some("/different"));
    assert_eq!(
        options.shell.unwrap().args(),
        &["--cd", "\"/home/pebrel-fixture/project space\"", "-d", "Ubuntu", "--user", "guest"]
    );
    assert!(options.working_directory.is_none());
}

#[test]
fn profile_directory_precedes_guest_home_but_explicit_wsl_cd_keeps_precedence() {
    let directory = tempfile::tempdir().unwrap();
    for own_directory in [false, true] {
        let profile = LaunchSession::Profile {
            name: "Guest profile".into(),
            command: "wsl.exe".into(),
            args: if own_directory { vec!["--cd".into(), "/chosen".into()] } else { Vec::new() },
            cwd: Some(directory.path().to_string_lossy().into_owned()),
            shell_id: None,
        };
        let TerminalLaunch::Local { shell, cwd, .. } =
            NebulaWorkspace::terminal_launch_from_session(&profile, None)
        else {
            panic!("expected local WSL launch")
        };
        let mut options =
            nebula_terminal::tty::Options { shell, working_directory: cwd, ..Default::default() };
        crate::platform::launch_directory::prepare_wsl(&mut options, None);
        assert_eq!(
            options.shell.unwrap().args(),
            &["--cd", if own_directory { "/chosen" } else { directory.path().to_str().unwrap() }]
        );
    }
}
