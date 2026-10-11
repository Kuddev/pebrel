use super::*;

fn profile(command: &str) -> Profile {
    Profile {
        name: "Synthetic".into(),
        command: command.into(),
        args: vec![],
        cwd: None,
        shell_id: Some("sh".into()),
        terminal_profile_id: Some("same-stable-id".into()),
        env: crate::profile_environment::ProfileEnvironment::from_pairs([
            ("QA_DEFAULT_OVERLAY".into(), "profile".into()),
            ("QA_EMPTY".into(), String::new()),
        ])
        .unwrap(),
    }
}

#[test]
fn local_profile_legacy_default_resolution_preserves_env_identity_and_cli_precedence() {
    let profiles = [profile("/bin/sh")];
    let selected =
        selected_local_profile(&profiles, Some("profile:old-family|same-stable-id"), false)
            .unwrap();
    assert_eq!(selected, profiles[0]);
    assert!(
        selected_local_profile(&profiles, Some("profile:sh|same-stable-id"), true).is_none(),
        "an explicit CLI command cannot receive another profile's variables"
    );
    assert!(selected_local_profile(&profiles, Some("profile:sh|missing-id"), false).is_none());
    assert!(selected_local_profile(&profiles, None, false).is_none());
    let launch = snapshot(selected.clone());
    assert_eq!(resolve(&launch).unwrap().env, selected.env);
    assert!(matches!(
        launch,
        LaunchSession::Profile { profile_id: Some(_), env_snapshot: Some(_), .. }
    ));
}

#[path = "../platform/profile_launch_pty_tests.rs"]
mod pty;
