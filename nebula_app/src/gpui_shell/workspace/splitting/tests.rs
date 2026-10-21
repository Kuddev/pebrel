use super::*;
use std::path::PathBuf;
use tab_duplication::{FocusedGuest, PaneOrigin};

fn shell(name: &str, program: &str, args: &[&str]) -> LaunchSession {
    LaunchSession::Shell {
        name: name.into(),
        program: program.into(),
        args: args.iter().map(|arg| (*arg).into()).collect(),
    }
}

#[test]
fn source_mode_matrix_resolves_identity_arguments_and_directory_without_a_window() {
    let host = shell("Host", "host-shell", &["--login"]);
    let wsl = shell("Guest", "wsl.exe", &["-u", "dev"]);
    let ssh = LaunchSession::Ssh { host: "server".into() };
    let configured = shell("Default", "default-shell", &["--configured"]);
    let selected = shell("Chosen", "chosen-shell", &["--chosen"]);
    let host_cwd = PathBuf::from("host-project");
    let cases = [
        (&host, None, "host-project", Some(host_cwd.clone()), None),
        (&wsl, Some(FocusedGuest { distro: "Ubuntu", user: Some("dev") }), "/home/dev", None, None),
        (&ssh, None, "/srv/project", None, Some(("server", Some("/srv/project".into())))),
    ];
    for (focused, guest, cwd, local, remote) in cases {
        for mode in SplitShellSource::ALL {
            let source = match mode {
                SplitShellSource::Default => SplitLaunch::Default(configured.clone()),
                SplitShellSource::Focused => SplitLaunch::Focused,
                SplitShellSource::Ask => SplitLaunch::Selected(selected.clone()),
            };
            let plan = resolve_split_launch(
                source,
                focused,
                PaneOrigin { guest, cwd, host_cwd: local.clone() },
                remote.clone(),
            );
            let expected = match (mode, guest) {
                (SplitShellSource::Default, _) => configured.clone(),
                (SplitShellSource::Ask, _) => selected.clone(),
                (SplitShellSource::Focused, Some(_)) => {
                    shell("Guest", "wsl.exe", &["--cd", "/home/dev", "-d", "Ubuntu", "-u", "dev"])
                },
                (SplitShellSource::Focused, None) => focused.clone(),
            };
            assert_eq!(plan.identity, expected, "{focused:?} × {mode:?}");
            assert_eq!(plan.host_cwd, local, "{focused:?} × {mode:?}");
            let expected_remote = if mode == SplitShellSource::Focused {
                remote.clone().and_then(|(_, cwd)| cwd)
            } else {
                None
            };
            assert_eq!(plan.remote_cwd, expected_remote, "{focused:?} × {mode:?}");
        }
    }
}

#[test]
fn profiles_use_live_host_directory_except_for_an_explicit_choice() {
    let profile = LaunchSession::Profile {
        name: "Profile".into(),
        command: "shell".into(),
        args: vec!["--login".into()],
        cwd: Some("startup".into()),
        shell_id: Some("profile".into()),
    };
    for (mode, host_cwd, expected_cwd) in [
        (SplitShellSource::Default, None, Some("startup")),
        (SplitShellSource::Default, Some(PathBuf::from("live")), None),
        (SplitShellSource::Focused, Some(PathBuf::from("live")), None),
        (SplitShellSource::Ask, Some(PathBuf::from("live")), Some("startup")),
    ] {
        let source = match mode {
            SplitShellSource::Default => SplitLaunch::Default(profile.clone()),
            SplitShellSource::Focused => SplitLaunch::Focused,
            SplitShellSource::Ask => SplitLaunch::Selected(profile.clone()),
        };
        let plan = resolve_split_launch(
            source,
            &profile,
            PaneOrigin { guest: None, cwd: "live", host_cwd: host_cwd.clone() },
            None,
        );
        assert!(
            matches!(&plan.identity, LaunchSession::Profile { cwd, .. } if cwd.as_deref() == expected_cwd)
        );
        assert_eq!(plan.host_cwd, host_cwd);
    }
}

#[test]
fn explicit_wsl_and_ssh_choices_only_inherit_a_matching_destination() {
    let focused = shell("Guest", "wsl.exe", &["-d", "Ubuntu", "-u", "dev"]);
    for (selected, expected_args) in [
        (
            shell("Same", "wsl.exe", &["-d", "Ubuntu", "-u", "dev"]),
            vec!["--cd", "/home/dev", "-d", "Ubuntu", "-u", "dev"],
        ),
        (shell("Other", "wsl.exe", &["-d", "Debian"]), vec!["-d", "Debian"]),
        (
            shell("Root", "wsl.exe", &["-d", "Ubuntu", "-u", "root"]),
            vec!["-d", "Ubuntu", "-u", "root"],
        ),
        (
            shell("Own cwd", "wsl.exe", &["-d", "Ubuntu", "-u", "dev", "--cd", "~/own"]),
            vec!["-d", "Ubuntu", "-u", "dev", "--cd", "~/own"],
        ),
    ] {
        let plan = resolve_split_launch(
            SplitLaunch::Selected(selected),
            &focused,
            PaneOrigin {
                guest: Some(FocusedGuest { distro: "Ubuntu", user: Some("dev") }),
                cwd: "/home/dev",
                host_cwd: None,
            },
            None,
        );
        let LaunchSession::Shell { args, .. } = plan.identity else { unreachable!() };
        assert_eq!(args, expected_args);
        assert_eq!(plan.host_cwd, None);
    }
    for (target, expected_cwd) in [("server", Some("/srv")), ("other", None)] {
        let plan = resolve_split_launch(
            SplitLaunch::Selected(LaunchSession::Ssh { host: target.into() }),
            &LaunchSession::Ssh { host: "server".into() },
            PaneOrigin { guest: None, cwd: "/srv", host_cwd: None },
            Some(("server", Some("/srv".into()))),
        );
        assert_eq!(plan.identity, LaunchSession::Ssh { host: target.into() });
        assert_eq!(plan.remote_cwd.as_deref(), expected_cwd);
        assert_eq!(plan.host_cwd, None);
    }
}

#[test]
fn captured_request_keeps_its_source_and_direction_and_rejects_a_closed_source() {
    for direction in [SplitDirection::LeftRight, SplitDirection::TopBottom] {
        let mut tree = SplitTree::Leaf(1);
        assert!(tree.split_leaf(1, 2, SplitDirection::LeftRight, 0.5));
        let request = PendingSplit { pane_id: 1, direction };
        // A different current focus does not participate in applying the request.
        assert!(request.apply(&mut tree, 3));
        let SplitTree::Split { first, .. } = &tree else { unreachable!() };
        let SplitTree::Split { direction: captured, .. } = first.as_ref() else { unreachable!() };
        assert_eq!(*captured, direction);
        assert_eq!(first.leaves(), [1, 3]);

        let mut tree = SplitTree::Leaf(1);
        tree.split_leaf(1, 2, SplitDirection::LeftRight, 0.5);
        tree.remove_leaf(1);
        assert!(!request.apply(&mut tree, 3));
        assert_eq!(tree.leaves(), [2], "never substitute the surviving pane for a closed source");
    }
}

#[test]
fn default_wsl_target_does_not_copy_the_focused_guest_or_user() {
    let configured = shell("Configured", "wsl.exe", &["-d", "Debian", "-u", "chosen"]);
    for host_cwd in [None, Some(PathBuf::from("mounted-host-directory"))] {
        let plan = resolve_split_launch(
            SplitLaunch::Default(configured.clone()),
            &LaunchSession::Default,
            PaneOrigin {
                guest: Some(FocusedGuest { distro: "Ubuntu", user: Some("dev") }),
                cwd: "/home/dev",
                host_cwd: host_cwd.clone(),
            },
            None,
        );
        assert_eq!(plan.identity, configured);
        assert_eq!(plan.host_cwd, host_cwd);
    }
}
