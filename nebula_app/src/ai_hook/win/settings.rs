//! Hook inspection and serialized settings operations, shared with the guard/CLI.

use std::io;
use std::path::{Path, PathBuf};

use nebula_settings::{AgentHook, RawSettings};
use serde_json::Value;

use super::super::installation::{desired_codex_notify, has_codex_notify};
use super::super::integrations::HookInspection;
use super::*;

pub(super) fn lock() -> io::Result<crate::atomic_file::LifetimeFileLock> {
    crate::atomic_file::try_lifetime_lock(&nebula_settings::settings_dir().join("agent-hooks"))?
        .ok_or_else(|| io::Error::other("Another hook operation is running. Please retry."))
}

pub(super) fn configuration(agent: AgentHook) -> Option<(PathBuf, bool)> {
    let (path, footprint) = match agent {
        AgentHook::Claude => {
            let dir = claude_config_dir()?;
            (dir.join("settings.json"), dir.is_dir())
        },
        AgentHook::Codex => {
            let path = codex_config_dir()?.join("config.toml");
            let footprint = path.is_file();
            (path, footprint)
        },
        AgentHook::OpenCode => {
            let dir = opencode_config_dir()?;
            (dir.join("plugins/pebrel.js"), dir.is_dir())
        },
        AgentHook::Pi => {
            let dir = pi_agent_dir()?;
            (dir.join("extensions/pebrel.ts"), dir.is_dir())
        },
        AgentHook::Kimi => {
            let dir = kimi::kimi_config_dir()?;
            (dir.join("config.toml"), dir.is_dir())
        },
        AgentHook::Cursor => {
            let path = cursor::path()?;
            let footprint = path.parent()?.is_dir();
            (path, footprint)
        },
        _ => (extended::path(agent)?, extended::directory(agent)?.is_dir()),
    };
    Some((path, footprint))
}

pub(crate) fn inspect(agent: AgentHook) -> HookInspection {
    let helper = helper_command();
    let mut result = HookInspection { helper_missing: helper.is_none(), ..Default::default() };
    if let Some((path, footprint)) = configuration(agent) {
        result.available = footprint;
        match installed_at(agent, &path, helper.as_deref().unwrap_or_default()) {
            Ok(installed) => {
                result.installed = installed;
                // Installed hooks must be visible even if the saved intent is off:
                // a previous removal may have failed, or another tool wrote them.
                result.enabled = installed;
                if installed && let Some(helper) = helper.as_deref() {
                    match current_at(agent, &path, helper) {
                        Ok(current) => result.needs_repair = !current,
                        Err(error) => result.error = Some(error.to_string()),
                    }
                }
            },
            Err(error) => result.error = Some(error.to_string()),
        }
        result.config_path = Some(path);
    }
    result
}

fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn installed_at(agent: AgentHook, path: &Path, helper: &str) -> io::Result<bool> {
    match agent {
        AgentHook::Claude => {
            let Some(raw) = read_optional(path)? else { return Ok(false) };
            let value: Value = serde_json::from_str(&raw).map_err(io::Error::other)?;
            let root =
                value.as_object().ok_or_else(|| io::Error::other("Expected a JSON object"))?;
            let Some(hooks) = root.get("hooks") else { return Ok(false) };
            let hooks = hooks
                .as_object()
                .ok_or_else(|| io::Error::other("Expected hooks to be an object"))?;
            Ok(CLAUDE_EVENTS.iter().any(|event| {
                hooks.get(*event).and_then(Value::as_array).is_some_and(|groups| {
                    groups.iter().any(|group| {
                        group
                            .get("hooks")
                            .and_then(Value::as_array)
                            .is_some_and(|commands| commands.iter().any(is_our_claude_hook))
                    })
                })
            }))
        },
        AgentHook::Codex => {
            let native = path.parent().map(codex_hooks::installed_at).transpose()?.unwrap_or(false);
            let Some(raw) = read_optional(path)? else { return Ok(native) };
            let doc = raw.parse::<toml_edit::DocumentMut>().map_err(io::Error::other)?;
            Ok(native
                || doc.get("notify").and_then(|v| v.as_array()).is_some_and(|array| {
                    let current: Vec<_> = array
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .collect();
                    has_codex_notify(&current)
                }))
        },
        AgentHook::Kimi => {
            let Some(raw) = read_optional(path)? else { return Ok(false) };
            Ok(kimi::inspect(&raw, Path::new(helper))?.0)
        },
        AgentHook::Cursor => cursor::installed_at(path),
        AgentHook::OpenCode | AgentHook::Pi => {
            let bridge = if agent == AgentHook::Pi { Bridge::Pi } else { Bridge::Opencode };
            let (name, legacy, content, hashes) = bridge.files();
            let dir =
                path.parent().ok_or_else(|| io::Error::other("Missing configuration directory"))?;
            managed_files::installed(&dir.join(name), &dir.join(legacy), content, hashes)
        },
        _ => extended::installed(agent, helper),
    }
}

/// Installation presence and completeness are different facts. A partial old
/// configuration stays visible as installed, but cannot pass write verification.
fn current_at(agent: AgentHook, path: &Path, helper: &str) -> io::Result<bool> {
    let Some(raw) = read_optional(path)? else { return Ok(false) };
    match agent {
        AgentHook::Cursor => cursor::current(&raw, helper),
        AgentHook::Kimi => Ok(kimi::inspect(&raw, Path::new(helper))?.1),
        AgentHook::Claude => {
            let mut value: Value = serde_json::from_str(&raw).map_err(io::Error::other)?;
            Ok(install_into(&mut value, helper) == Some(false))
        },
        AgentHook::Codex => {
            let doc = raw.parse::<toml_edit::DocumentMut>().map_err(io::Error::other)?;
            let Some(array) = doc.get("notify").and_then(|value| value.as_array()) else {
                return Ok(false);
            };
            let Some(current) = array
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
            else {
                return Ok(false);
            };
            Ok(has_codex_notify(&current)
                && desired_codex_notify(&current, helper).is_none()
                && codex_hooks::current_at(
                    path.parent()
                        .ok_or_else(|| io::Error::other("Missing configuration directory"))?,
                    helper,
                )?)
        },
        AgentHook::OpenCode | AgentHook::Pi => {
            let bridge = if agent == AgentHook::Pi { Bridge::Pi } else { Bridge::Opencode };
            let (_, legacy, content, _) = bridge.files();
            Ok(raw == content && !path.with_file_name(legacy).exists())
        },
        _ => Ok(raw == extended::content(agent, helper)?),
    }
}

pub(crate) fn set_enabled(agent: AgentHook, enabled: bool) -> io::Result<()> {
    let _lock = lock()?;
    if enabled {
        let (_, footprint) = configuration(agent)
            .ok_or_else(|| io::Error::other("Agent configuration unavailable"))?;
        if !footprint {
            return Err(io::Error::other(
                "Start this agent once to create its configuration, then refresh.",
            ));
        }
        if helper_path().is_none() {
            return Err(io::Error::other("pebrel-hook.exe is missing from this installation."));
        }
    }
    // Persist first so another instance's guard cannot reinstall a removed hook.
    nebula_settings::persist_keys(&[(
        agent.settings_key(),
        if enabled { "1" } else { "0" }.into(),
    )])?;
    apply_and_verify(agent, enabled)
}

pub(super) fn apply_and_verify(agent: AgentHook, enabled: bool) -> io::Result<()> {
    apply(agent, enabled)?;
    let state = inspect(agent);
    if let Some(error) = state.error {
        return Err(io::Error::other(error));
    }
    if state.installed != enabled || (enabled && state.needs_repair) {
        return Err(io::Error::other(
            "Hook configuration could not be verified. Check file permissions and configuration, then retry.",
        ));
    }
    Ok(())
}

pub(super) fn apply(agent: AgentHook, enabled: bool) -> io::Result<()> {
    match (agent, enabled) {
        (AgentHook::Claude, true) => {
            ensure_claude_hooks();
        },
        (AgentHook::Codex, true) => {
            ensure_codex_notify();
            ensure_codex_hooks();
        },
        (AgentHook::OpenCode, true) => {
            ensure_opencode_plugin();
        },
        (AgentHook::Pi, true) => {
            ensure_pi_extension();
        },
        (AgentHook::Kimi, true) => {
            kimi::ensure_kimi_hooks();
        },
        (AgentHook::Claude, false) => {
            remove_hooks()?;
        },
        (AgentHook::Codex, false) => {
            let native = remove_codex_hooks();
            let notify = remove_codex_notify();
            native?;
            notify?;
        },
        (AgentHook::OpenCode, false) => {
            remove_opencode_plugin()?;
        },
        (AgentHook::Pi, false) => {
            remove_pi_extension()?;
        },
        (AgentHook::Kimi, false) => {
            kimi::remove_kimi_hooks()?;
        },
        (AgentHook::Cursor, _) => {
            let path = cursor::path()
                .ok_or_else(|| io::Error::other("Cursor configuration unavailable"))?;
            cursor::apply_at(&path, &helper_command().unwrap_or_default(), enabled)?;
        },
        (_, _) => extended::apply(agent, &helper_command().unwrap_or_default(), enabled)?,
    }
    Ok(())
}

pub(super) fn heal_enabled(raw: &RawSettings) {
    for agent in AgentHook::ALL {
        if agent.enabled(raw) && configuration(agent).is_some_and(|(_, footprint)| footprint) {
            if let Err(error) = apply(agent, true) {
                log::warn!("ai_hook: {}: {error}", agent.settings_key());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_existing_hooks_without_a_saved_preference_or_current_helper() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"hooks":{"Stop":[{"hooks":[{"command":"C:/old/nebula-hook.exe claude"}]}]}}"#,
        )
        .unwrap();
        assert!(installed_at(AgentHook::Claude, &path, "").unwrap());
        std::fs::write(&path, r#"{"hooks":{"Stop":[{"hooks":[{"command":"user-notifier"}]}]}}"#)
            .unwrap();
        assert!(!installed_at(AgentHook::Claude, &path, "").unwrap());
        std::fs::write(&path, "broken JSON").unwrap();
        assert!(installed_at(AgentHook::Claude, &path, "").is_err());
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "notify = ['C:/old/pebrel-hook.exe', 'codex', '--chain', 'my-notifier']\n",
        )
        .unwrap();
        assert!(installed_at(AgentHook::Codex, &path, "").unwrap());
        std::fs::write(&path, "notify = ['my-notifier']\n").unwrap();
        assert!(!installed_at(AgentHook::Codex, &path, "").unwrap());
    }

    #[test]
    fn discovers_current_and_legacy_plugins_and_reports_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pebrel.ts");
        let legacy = dir.path().join("nebula.ts");
        std::fs::write(&legacy, PI_EXTENSION_TS).unwrap();
        assert!(installed_at(AgentHook::Pi, &path, "").unwrap());
        std::fs::write(&legacy, "user extension").unwrap();
        assert!(installed_at(AgentHook::Pi, &path, "").is_err());
        assert_eq!(std::fs::read_to_string(&legacy).unwrap(), "user extension");
    }

    #[test]
    fn partial_claude_install_is_visible_but_never_verified_as_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let helper = "C:/tools/pebrel-hook.exe";
        let mut value = json!({"hooks":{"Stop":[{"hooks":[{"type":"command","command":helper,"args":HELPER_ARGS}]}]}});
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(installed_at(AgentHook::Claude, &path, helper).unwrap());
        assert!(!current_at(AgentHook::Claude, &path, helper).unwrap());
        install_into(&mut value, helper).unwrap();
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(current_at(AgentHook::Claude, &path, helper).unwrap());
        value["hooks"]["SessionStart"] = json!("invalid shape");
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(!current_at(AgentHook::Claude, &path, helper).unwrap());
    }
}
