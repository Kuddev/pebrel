//! Synthetic presets exercise the model without Windows discovery or shell execution.

use super::*;
use crate::shell_detect::DetectedShell;

#[test]
fn detected_presets_copy_raw_windows_arguments_into_distinct_saved_profile_models() {
    for shell in [
        DetectedShell {
            name: "PowerShell 7".into(),
            id: "pwsh".into(),
            program: r"C:\Program Files\PowerShell\7\pwsh.exe".into(),
            args: ["-NoLogo", "-NoProfile"].map(String::from).to_vec(),
        },
        DetectedShell {
            name: "WSL · Team Linux".into(),
            id: "wsl:Team Linux".into(),
            program: r"C:\Windows\System32\wsl.exe".into(),
            args: ["--distribution", "Team Linux", "--user", "guest"].map(String::from).to_vec(),
        },
    ] {
        let profile = TerminalProfiles::from_detected(&shell).unwrap();
        let second = TerminalProfiles::from_detected(&shell).unwrap();
        assert_ne!(profile.id, second.id);
        assert_eq!(profile.name, shell.name);
        assert_eq!(profile.command, PathBuf::from(&shell.program));
        assert_eq!(profile.args, shell.args, "save raw options, not injected integration scripts");
        assert_eq!(profile.shell_id.as_deref(), Some(shell.id.as_str()));
        assert!(profile.cwd.is_none());
        assert!(profile.env.is_empty());
    }
}

#[test]
fn detected_preset_custom_arguments_roundtrip_with_existing_cas_and_default_selection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles.json");
    // An executable copy also works when the test binary and temp directory use different volumes.
    // This fixture never launches it or discovers a user's Windows installation.
    let program = directory.path().join("wsl.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let detected = DetectedShell {
        name: "Synthetic WSL".into(),
        id: "wsl:Synthetic Linux".into(),
        program: program.to_string_lossy().into_owned(),
        args: ["-d", "Synthetic Linux"].map(String::from).to_vec(),
    };
    let mut profile = TerminalProfiles::from_detected(&detected).unwrap();
    profile.args.extend(["--user", "synthetic-user"].map(String::from));
    let store = TerminalProfiles::edit_at(&path, None, Some(profile.clone())).unwrap();
    assert_eq!(TerminalProfiles::load_from(&path).unwrap(), store);
    assert_eq!(store.profiles()[0], profile);
    assert_eq!(detected.args, ["-d", "Synthetic Linux"], "preset remains immutable");
    let options = store.as_config_profiles();
    let id = options[0].settings_id().unwrap();
    let selected =
        crate::profile_launch::selected_local_profile(&options, Some(&id), false).unwrap();
    let expected = ["--cd", "~", "-d", "Synthetic Linux", "--user", "synthetic-user"];
    assert_eq!(selected.shell().args(), expected);
    assert!(selected.cwd.is_none(), "guest home must never become a host directory");
    let snapshot = crate::profile_launch::snapshot(selected);
    let restored: crate::session::LaunchSession =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
    let crate::session::LaunchSession::Profile { args, cwd, profile_id, .. } = restored else {
        panic!("profile identity lost");
    };
    assert_eq!(args, expected, "cold snapshot retains WSL's own home policy");
    assert!(cwd.is_none());
    assert_eq!(profile_id.as_deref(), Some(profile.id.as_str()));
    let mut edited = profile.clone();
    edited.args.push("--shell-type=login".into());
    TerminalProfiles::edit_at(&path, Some(&profile), Some(edited.clone())).unwrap();
    assert!(TerminalProfiles::edit_at(&path, Some(&profile), Some(profile.clone())).is_err());
    assert_eq!(TerminalProfiles::load_from(&path).unwrap().profiles(), &[edited]);
}
