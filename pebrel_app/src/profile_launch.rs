//! Profile identity is portable; explicit environment values stay in local configuration.

use crate::config::ui_config::Profile;
use crate::session::LaunchSession;

pub(crate) fn snapshot(profile: Profile) -> LaunchSession {
    LaunchSession::Profile {
        name: profile.name,
        command: profile.command,
        args: profile.args,
        cwd: profile.cwd.map(|path| path.to_string_lossy().into_owned()),
        shell_id: profile.shell_id,
        profile_id: profile.terminal_profile_id.clone(),
        env_snapshot: (profile.terminal_profile_id.is_some() || !profile.env.is_empty())
            .then_some(profile.env),
    }
}

pub(crate) fn shell(profile: &crate::config::ui_config::Profile) -> pebrel_terminal::tty::Shell {
    let shell = profile.shell();
    if crate::platform::launch_directory::is_wsl_program(shell.program())
        && let Some(launch) = crate::shell_detect::wsl_launch(shell.program(), shell.args())
        && !launch.chooses_directory
        && let Some(cwd) =
            profile.cwd.as_deref().and_then(crate::platform::launch_directory::wsl_argument)
        && let Some(args) =
            crate::shell_detect::wsl_args_in_directory(shell.program(), shell.args(), &cwd)
    {
        return pebrel_terminal::tty::Shell::new(shell.program().to_owned(), args);
    }
    shell
}

/// Running duplicates carry a frozen overlay; cold restore resolves the local id.
/// Exported workspaces retain their embedded command fallback on another machine.
pub(crate) fn resolve(launch: &LaunchSession) -> Option<Profile> {
    let LaunchSession::Profile { name, command, args, cwd, shell_id, profile_id, env_snapshot } =
        launch
    else {
        return None;
    };
    let mut profile = Profile {
        name: name.clone(),
        command: command.clone(),
        args: args.clone(),
        cwd: cwd.as_deref().and_then(|cwd| {
            crate::platform::launch_directory::validate(
                cwd.into(),
                crate::platform::launch_directory::is_wsl_program(command),
            )
        }),
        shell_id: shell_id.clone(),
        terminal_profile_id: profile_id.clone(),
        env: env_snapshot.clone().unwrap_or_default(),
    };
    if env_snapshot.is_none() {
        let local = profile_id.as_ref().and_then(|id| {
            crate::terminal_profiles::TerminalProfiles::load()
                .ok()?
                .as_config_profiles()
                .into_iter()
                .find(|profile| profile.terminal_profile_id.as_ref() == Some(id))
        });
        apply_local_profile(&mut profile, local);
    }
    Some(profile)
}

fn apply_local_profile(profile: &mut Profile, local: Option<Profile>) {
    if let Some(local) = local {
        let previous_shell = shell(profile);
        let mut location = profile.cwd.take();
        // A saved guest directory is tied to its guest and user. Encode it in
        // the old launch before comparing identities with a newly saved profile.
        if crate::platform::launch_directory::is_wsl_program(&profile.command)
            && location
                .as_deref()
                .and_then(std::path::Path::to_str)
                .is_some_and(|path| path.starts_with('/') && !path.starts_with("//"))
        {
            location = None;
        }
        let guest_args = (location.is_none() && local.cwd.is_none())
            .then(|| {
                crate::shell_detect::wsl_args_with_restored_directory(
                    &profile.command,
                    previous_shell.args(),
                    &local.command,
                    &local.args,
                )
            })
            .flatten();
        *profile = local;
        profile.cwd = location.or(profile.cwd.take());
        if let Some(args) = guest_args {
            profile.args = args;
        }
    }
}

/// Explicit commands keep their own options; selected saved defaults retain
/// the complete profile rather than reducing it to an executable override.
pub(crate) fn selected_local_profile(
    profiles: &[Profile],
    requested: Option<&str>,
    explicit_command: bool,
) -> Option<Profile> {
    if explicit_command {
        return None;
    }
    let requested = requested?;
    profiles.iter().find(|profile| profile.matches_settings_id(requested)).cloned()
}

pub(crate) fn apply_local_profile_options(
    options: &mut pebrel_terminal::tty::Options,
    environment: &crate::profile_environment::ProfileEnvironment,
) {
    environment.apply(&mut options.env);
    if let Err(error) = crate::platform::shell_integration::prepare(options) {
        log::warn!("Could not prepare profile shell integration: {error}");
    }
}

#[cfg(test)]
mod legacy_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn saved_guest_directories_survive_legacy_restore_and_follow_only_the_same_guest() {
        let directory = "/home/pebrel-profile-fixture/project space";
        let launch = LaunchSession::Profile {
            name: "WSL fixture".into(),
            command: "wsl.exe".into(),
            args: ["-d", "Debian", "-u", "dev"].map(String::from).to_vec(),
            cwd: Some(directory.into()),
            shell_id: None,
            profile_id: None,
            env_snapshot: None,
        };
        let restored = resolve(&launch).unwrap();
        assert_eq!(restored.cwd.as_deref(), Some(std::path::Path::new(directory)));
        assert_eq!(
            shell(&restored).args(),
            &["--cd", &format!("\"{directory}\""), "-d", "Debian", "-u", "dev"]
        );
        let mut frozen = launch;
        if let LaunchSession::Profile { env_snapshot, .. } = &mut frozen {
            *env_snapshot = Some(Default::default());
        }
        assert_eq!(resolve(&frozen).unwrap().cwd, restored.cwd);

        for (distro, user, carry) in
            [("Debian", "dev", true), ("Ubuntu", "dev", false), ("Debian", "other", false)]
        {
            let mut local = restored.clone();
            local.cwd = None;
            local.args = ["-d", distro, "-u", user].map(String::from).to_vec();
            let expected = local.args.clone();
            let mut previous = restored.clone();
            apply_local_profile(&mut previous, Some(local));
            assert!(previous.cwd.is_none());
            if carry {
                assert_eq!(previous.args, shell(&restored).args());
            } else {
                assert_eq!(previous.args, expected);
            }
        }
    }

    #[test]
    fn cold_wsl_profile_reload_carries_directory_only_for_known_same_guest_and_user() {
        let previous = Profile {
            command: "wsl.exe".into(),
            args: ["--cd", "\"/project dir\"", "-d", "Debian", "-u", "dev"]
                .map(String::from)
                .to_vec(),
            name: "Synthetic WSL".into(),
            cwd: None,
            shell_id: None,
            terminal_profile_id: None,
            env: Default::default(),
        };
        for (distro, user, carry) in [
            (Some("Debian"), Some("dev"), true),
            (Some("Ubuntu"), Some("dev"), false),
            (Some("Debian"), Some("other"), false),
            (None, Some("dev"), false),
            (Some("Debian"), None, false),
        ] {
            let mut local = previous.clone();
            local.args.clear();
            if let Some(distro) = distro {
                local.args.extend(["-d".into(), distro.into()]);
            }
            if let Some(user) = user {
                local.args.extend(["-u".into(), user.into()]);
            }
            local.args.extend(["--exec", "tool", "--cd", "/guest-option"].map(String::from));
            let mut restored = previous.clone();
            apply_local_profile(&mut restored, Some(local.clone()));
            let expected = if carry {
                [vec!["--cd".into(), "\"/project dir\"".into()], local.args].concat()
            } else {
                assert!(
                    crate::shell_detect::wsl_args_with_restored_directory(
                        &local.command,
                        &local.args,
                        &previous.command,
                        &previous.args,
                    )
                    .is_none()
                );
                local.args
            };
            assert_eq!(restored.args, expected);
            assert!(restored.cwd.is_none());
        }
        for selector in ["--system", "--distribution-id=synthetic-guid"] {
            let mut opaque = previous.clone();
            opaque.args.insert(0, selector.into());
            assert!(
                crate::shell_detect::wsl_args_with_restored_directory(
                    &opaque.command,
                    &opaque.args,
                    &previous.command,
                    &previous.args,
                )
                .is_none()
            );
        }
        let directory = tempfile::tempdir().unwrap();
        let mut restored = previous.clone();
        restored.cwd = Some(directory.path().into());
        let mut local = previous;
        local.args = ["-d", "Debian", "-u", "dev"].map(String::from).to_vec();
        apply_local_profile(&mut restored, Some(local.clone()));
        assert_eq!(restored.cwd.as_deref(), Some(directory.path()));
        assert_eq!(restored.args, local.args);
    }

    #[test]
    fn local_profile_workspace_export_omits_values_and_old_schema_remains_readable() {
        let profile = Profile {
            name: "QA".into(),
            command: "/bin/sh".into(),
            args: vec![],
            cwd: None,
            shell_id: Some("sh".into()),
            terminal_profile_id: Some("qa-independent-id".into()),
            env: crate::profile_environment::ProfileEnvironment::from_pairs([(
                "QA_TOKEN".into(),
                "synthetic-secret".into(),
            )])
            .unwrap(),
        };
        let frozen = snapshot(profile.clone());
        let encoded = serde_json::to_string(&frozen).unwrap();
        assert!(!encoded.contains("synthetic-secret"));
        assert!(!encoded.contains("QA_TOKEN"));
        assert!(encoded.contains("qa-independent-id"));
        let decoded: LaunchSession = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded, LaunchSession::Profile { env_snapshot: None, .. }));
        let old: LaunchSession = serde_json::from_str(
            r#"{"kind":"profile","name":"old","command":"/bin/sh","args":[],"cwd":null}"#,
        )
        .unwrap();
        assert!(matches!(old, LaunchSession::Profile { profile_id: None, env_snapshot: None, .. }));
        // Duplicating a running launch must not consult the local store.
        assert_eq!(resolve(&frozen).unwrap().env, profile.env);
        let mut restored = profile.clone();
        restored.env = Default::default();
        let mut edited = profile;
        edited.env.0.insert("QA_TOKEN".into(), "edited-for-next-launch".into());
        apply_local_profile(&mut restored, Some(edited.clone()));
        assert_eq!(restored.env, edited.env);
    }
}
