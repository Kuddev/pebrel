//! Fresh-child isolation for profile/workspace regressions; no native credentials.

use std::path::{Path, PathBuf};

const MARKER: &str = "PEBREL_TEST_PROFILE_CHILD";
const ROOT: &str = "PEBREL_TEST_PROFILE_ROOT";

pub(crate) fn root() -> PathBuf {
    std::env::var_os(ROOT).map(PathBuf::from).expect("isolated child root")
}

pub(crate) fn prepare(directory: &Path) -> std::io::Result<()> {
    let config = directory.join("config");
    std::fs::create_dir_all(&config)?;
    std::fs::create_dir_all(directory.join("home/.ssh"))?;
    std::fs::write(directory.join("home/.ssh/config"), "")?;
    let mut updates = nebula_settings::AgentHook::all_updates(false);
    updates.extend([
        ("auto_check_updates", "0".into()),
        ("auto_download_updates", "0".into()),
        ("restore_session", "0".into()),
        ("resume_ai", "0".into()),
        ("refresh_environment", "0".into()),
    ]);
    std::fs::write(config.join("pebrel_settings.txt"), nebula_settings::apply_updates("", &updates))
}

/// Return true only in the parent after the exact child scenario succeeded.
pub(crate) fn run_exact_child(filter: &str, artifact: Option<(&str, usize)>) -> bool {
    if std::env::var(MARKER).ok().as_deref() == Some(filter) {
        assert_isolated();
        return false;
    }
    let directory = tempfile::tempdir().unwrap();
    prepare(directory.path()).unwrap();
    let config = directory.path().join("config");
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", filter, "--nocapture", "--test-threads=1"])
        .env(MARKER, filter)
        .env(ROOT, directory.path())
        .env("PEBREL_CONFIG_DIR", &config)
        .env("NEBULA_CONFIG_DIR", &config)
        .env("PEBREL_TEST_SSH_HOME", directory.path().join("home"))
        .env_remove("PEBREL_CONFIG_FILE")
        .env_remove("NEBULA_CONFIG_FILE")
        .env_remove("PEBREL_GPUI_CONFIG")
        .env_remove("NEBULA_GPUI_CONFIG")
        .env_remove("NEBULA_GPUI_OPEN_DOC")
        .env_remove("PEBREL_GPUI_OPEN_DOC")
        .env_remove("PEBREL_PROFILE_ENV_QA")
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("PEBREL_SSH_CONFIG")
        .env_remove("NEBULA_SSH_CONFIG")
        .env_remove("NEBULA_PTY_RECORD")
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("PEBREL_PANE_ID")
        .env_remove("NEBULA_PANE_ID")
        .env_remove("PEBREL_RUNTIME_ENDPOINT")
        .env_remove("PEBREL_NOTIFY_PIPE")
        .env_remove("NEBULA_NOTIFY_PIPE")
        .env_remove("PEBREL_HOOK_EXE")
        .env_remove("NEBULA_HOOK_EXE")
        .env_remove("PEBREL_REMOTE_HOOK_TOKEN")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "isolated exact child failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("1 passed"),
        "child must execute the exact test"
    );
    if let Some((file, lines)) = artifact {
        assert_eq!(
            std::fs::read_to_string(directory.path().join(file)).unwrap().lines().count(),
            lines
        );
    }
    true
}

pub(crate) fn assert_isolated() {
    let directory = root();
    let config = directory.join("config");
    assert!(directory.is_absolute() && directory.is_dir());
    assert_eq!(super::dirs::data_dir(), config);
    assert_eq!(nebula_settings::settings_path(), config.join("pebrel_settings.txt"));
    let raw = nebula_settings::RawSettings::try_load().unwrap();
    assert_eq!(raw.bool_on("ai_hooks"), Some(false));
    for agent in nebula_settings::AgentHook::ALL {
        assert!(!agent.enabled(&raw));
    }
    let settings = nebula_settings::RuntimeSettings::from_raw(&raw);
    assert!(!settings.auto_check_updates && !settings.auto_download_updates);
    assert!(!settings.restore_session && !settings.resume_ai && !settings.refresh_environment);
    assert_eq!(super::dirs::ssh_config_home(), Some(directory.join("home")));
    assert_eq!(crate::ssh::ssh_config_path(), Some(directory.join("home/.ssh/config")));
    assert_eq!(crate::ssh::read_ssh_config().unwrap(), "");
    assert!(crate::ssh::ssh_config_hosts().is_empty());
    for name in [
        "SSH_AUTH_SOCK",
        "NEBULA_GPUI_OPEN_DOC",
        "PEBREL_GPUI_OPEN_DOC",
        "NEBULA_PTY_RECORD",
        "PEBREL_RUNTIME_ENDPOINT",
        "PEBREL_NOTIFY_PIPE",
        "NEBULA_NOTIFY_PIPE",
        "PEBREL_HOOK_EXE",
        "NEBULA_HOOK_EXE",
        "PEBREL_REMOTE_HOOK_TOKEN",
    ] {
        assert!(std::env::var_os(name).is_none(), "unexpected test side-effect override: {name}");
    }
}

#[test]
fn owned_profile_fixture_resolves_settings_and_all_ssh_routes_before_construction() {
    let filter = "platform::profile_test_isolation::owned_profile_fixture_resolves_settings_and_all_ssh_routes_before_construction";
    if run_exact_child(filter, None) {
        return;
    }
    assert_isolated();
}
