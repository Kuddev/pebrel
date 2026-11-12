//! `settings.json` hooks for agents that document Claude Code's event contract.
//! Only complete helper invocations are claimed; other hooks and settings are preserved.

use std::io;
use std::path::{Path, PathBuf};

use nebula_settings::AgentHook;
use serde_json::{Value, json};

use super::extended::{command_for, owns_command};

const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "Notification",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

/// Droid documents neither permission-request nor failure events.
const DROID_EVENTS: &[&str] =
    &["SessionStart", "UserPromptSubmit", "Notification", "PostToolUse", "Stop", "SessionEnd"];

/// Hook source, configuration directory under the home directory, subscribed events.
fn provider(agent: AgentHook) -> io::Result<(&'static str, &'static str, &'static [&'static str])> {
    match agent {
        AgentHook::Qoder => Ok(("qoder", ".qoder", EVENTS)),
        AgentHook::CodeBuddy => Ok(("codebuddy", ".codebuddy", EVENTS)),
        AgentHook::Qwen => Ok(("qwen", ".qwen", EVENTS)),
        AgentHook::Droid => Ok(("droid", ".factory", DROID_EVENTS)),
        _ => Err(invalid("Unsupported hook provider")),
    }
}

pub(super) fn directory(agent: AgentHook) -> Option<PathBuf> {
    Some(crate::platform::dirs::home_dir()?.join(provider(agent).ok()?.1))
}

pub(super) fn path(agent: AgentHook) -> Option<PathBuf> {
    let dir = directory(agent)?;
    Some(if agent == AgentHook::Droid { droid_path(&dir) } else { dir.join("settings.json") })
}

/// Follow Droid's effective user configuration without masking legacy user hooks.
fn droid_path(dir: &Path) -> PathBuf {
    ["hooks.json", "hooks/hooks.json", "settings.json"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| dir.join("hooks.json"))
}

pub(super) fn apply_droid_at(dir: &Path, helper: &str, enabled: bool) -> io::Result<()> {
    if enabled {
        return apply_at(AgentHook::Droid, &droid_path(dir), helper, true);
    }
    // Remove inactive fallback entries too: deleting a native file must not
    // resurrect a hook the user explicitly disabled.
    for name in ["hooks.json", "hooks/hooks.json", "settings.json"] {
        apply_at(AgentHook::Droid, &dir.join(name), helper, false)?;
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Returns the edited text, whether the input held our hooks, and whether the edit changed it.
fn edit(
    agent: AgentHook,
    raw: &str,
    standalone: bool,
    helper: &str,
    enabled: bool,
) -> io::Result<(String, bool, bool)> {
    let (source, _, events) = provider(agent)?;
    let mut doc: Value = serde_json::from_str(raw).map_err(io::Error::other)?;
    let root = doc.as_object_mut().ok_or_else(|| invalid("Expected a JSON object"))?;
    let mut changed = false;
    let mut installed = false;
    if !standalone && !root.contains_key("hooks") && enabled {
        root.insert("hooks".into(), json!({}));
        changed = true;
    }
    let hooks = if standalone {
        Some(root)
    } else {
        root.get_mut("hooks")
            .map(|hooks| hooks.as_object_mut().ok_or_else(|| invalid("Expected a hooks object")))
            .transpose()?
    };
    if let Some(hooks) = hooks {
        let desired =
            json!({"type": "command", "command": command_for(helper, source), "timeout": 10});
        let is_owned = |entry: &Value| {
            entry.get("command").and_then(Value::as_str).is_some_and(|c| owns_command(c, source))
        };
        let entries_of = |group: &Value| group.get("hooks").and_then(Value::as_array).cloned();
        for event in events {
            if !hooks.contains_key(*event) && !enabled {
                continue;
            }
            let groups = hooks
                .entry(*event)
                .or_insert(json!([]))
                .as_array_mut()
                .ok_or_else(|| invalid("Expected a hook group array; file preserved"))?;
            let ours: Vec<_> = groups
                .iter()
                .filter_map(entries_of)
                .flatten()
                .filter(|entry| is_owned(entry))
                .collect();
            installed |= !ours.is_empty();
            if (enabled && ours == [desired.clone()]) || (!enabled && ours.is_empty()) {
                continue;
            }
            // A group that held only our entries goes away with them.
            groups.retain(|group| {
                !entries_of(group).is_some_and(|entries| {
                    !entries.is_empty() && entries.iter().all(|entry| is_owned(entry))
                })
            });
            for group in groups.iter_mut() {
                if let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    entries.retain(|entry| !is_owned(entry));
                }
            }
            if enabled {
                groups.push(json!({"hooks": [desired.clone()]}));
            }
            changed = true;
        }
    }
    let text = if changed {
        serde_json::to_string_pretty(&doc).map_err(io::Error::other)? + "\n"
    } else {
        raw.to_owned()
    };
    Ok((text, installed, changed))
}

pub(super) fn installed_at(agent: AgentHook, path: &Path) -> io::Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(edit(agent, &raw, standalone(agent, path), "", false)?.1),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn standalone(agent: AgentHook, path: &Path) -> bool {
    agent == AgentHook::Droid && path.file_name().is_some_and(|name| name == "hooks.json")
}

pub(super) fn current(agent: AgentHook, path: &Path, raw: &str, helper: &str) -> io::Result<bool> {
    Ok(!edit(agent, raw, standalone(agent, path), helper, true)?.2)
}

pub(super) fn apply_at(
    agent: AgentHook,
    path: &Path,
    helper: &str,
    enabled: bool,
) -> io::Result<()> {
    if enabled && helper.is_empty() {
        return Err(io::Error::other("The Pebrel hook helper is missing from this installation."));
    }
    if !enabled && !path.exists() {
        return Ok(());
    }
    let _lock = crate::atomic_file::try_lock(path)?
        .ok_or_else(|| io::Error::other("Hook configuration is busy"))?;
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => "{}".into(),
        Err(error) => return Err(error),
    };
    let (updated, _, changed) = edit(agent, &raw, standalone(agent, path), helper, enabled)?;
    if changed {
        crate::atomic_file::write(path, updated.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELPER: &str = "C:/Program Files/用户's Tools/pebrel-hook.exe";
    const ALL: [AgentHook; 4] =
        [AgentHook::Qoder, AgentHook::CodeBuddy, AgentHook::Qwen, AgentHook::Droid];

    #[test]
    fn preserves_user_settings_and_hooks_and_installs_each_event_once() {
        for agent in ALL {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            let original = r#"{"model":"keep","hooks":{"Stop":[{"hooks":[{"type":"command","command":"foreign"}]}]}}"#;
            std::fs::write(&path, original).unwrap();
            apply_at(agent, &path, "C:/old/nebula-hook.exe", true).unwrap();
            assert!(installed_at(agent, &path).unwrap());
            assert!(
                !current(agent, &path, &std::fs::read_to_string(&path).unwrap(), HELPER).unwrap()
            );
            apply_at(agent, &path, HELPER, true).unwrap();
            let installed = std::fs::read_to_string(&path).unwrap();
            assert!(current(agent, &path, &installed, HELPER).unwrap());
            let parsed: Value = serde_json::from_str(&installed).unwrap();
            let (source, _, events) = provider(agent).unwrap();
            assert_eq!(parsed["hooks"].as_object().unwrap().len(), events.len());
            for event in events {
                let owned = parsed["hooks"][event]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|group| group["hooks"].as_array().unwrap())
                    .filter(|entry| owns_command(entry["command"].as_str().unwrap(), source))
                    .count();
                assert_eq!(owned, 1, "{agent:?} {event}");
            }
            assert_eq!(parsed["hooks"]["Stop"][0]["hooks"][0]["command"], "foreign");
            assert_eq!(parsed["model"], "keep");
            apply_at(agent, &path, HELPER, true).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), installed);
            apply_at(agent, &path, "", false).unwrap();
            assert!(!installed_at(agent, &path).unwrap());
            let removed: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(removed["hooks"]["Stop"][0]["hooks"][0]["command"], "foreign");
            assert_eq!(removed["hooks"]["Stop"].as_array().unwrap().len(), 1);
            assert_eq!(removed["model"], "keep");
        }
    }

    #[test]
    fn droid_never_subscribes_to_events_it_does_not_document() {
        let text = edit(AgentHook::Droid, "{}", false, HELPER, true).unwrap().0;
        let hooks: Value = serde_json::from_str(&text).unwrap();
        for event in ["PermissionRequest", "StopFailure", "PostToolUseFailure", "PreToolUse"] {
            assert!(hooks["hooks"].get(event).is_none(), "{event}");
        }
        assert!(hooks["hooks"]["Notification"].is_array());
    }

    #[test]
    fn malformed_and_busy_configuration_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for raw in ["broken", "[]", r#"{"hooks":42}"#, r#"{"hooks":{"Stop":42}}"#] {
            std::fs::write(&path, raw).unwrap();
            assert!(apply_at(AgentHook::Qwen, &path, HELPER, true).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        }
        std::fs::write(&path, "{}").unwrap();
        let _lock = crate::atomic_file::try_lock(&path).unwrap().unwrap();
        assert!(apply_at(AgentHook::Qwen, &path, HELPER, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        assert!(apply_at(AgentHook::Qwen, &path, "", true).is_err());
    }

    #[test]
    fn another_agents_entry_or_an_edited_command_is_not_claimed() {
        let qoder = command_for(HELPER, "qoder");
        assert!(owns_command(&qoder, "qoder"));
        assert!(!owns_command(&qoder, "qwen"));
        assert!(!owns_command(&format!("{qoder}; echo custom"), "qoder"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let foreign =
            json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": qoder}]}]}});
        std::fs::write(&path, foreign.to_string()).unwrap();
        assert!(!installed_at(AgentHook::Qwen, &path).unwrap());
    }
}

#[cfg(test)]
mod droid_tests {
    use super::*;

    #[test]
    fn native_droid_file_takes_precedence_and_uses_flat_events() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        apply_at(AgentHook::Droid, &settings, "C:/old/pebrel-hook.exe", true).unwrap();
        let native = dir.path().join("hooks.json");
        std::fs::write(&native, r#"{"Stop":[{"hooks":[{"command":"user-hook"}]}]}"#).unwrap();
        assert_eq!(droid_path(dir.path()), native);
        apply_droid_at(dir.path(), "C:/new tools/pebrel-hook.exe", true).unwrap();
        let raw = std::fs::read_to_string(&native).unwrap();
        let doc: Value = serde_json::from_str(&raw).unwrap();
        assert!(doc.get("hooks").is_none());
        assert!(doc["SessionStart"].is_array());
        assert_eq!(doc["Stop"][0]["hooks"][0]["command"], "user-hook");
        assert!(current(AgentHook::Droid, &native, &raw, "C:/new tools/pebrel-hook.exe").unwrap());
        apply_droid_at(dir.path(), "C:/new tools/pebrel-hook.exe", true).unwrap();
        assert_eq!(std::fs::read_to_string(&native).unwrap(), raw);
        apply_droid_at(dir.path(), "", false).unwrap();
        for path in [&native, &settings] {
            assert!(!installed_at(AgentHook::Droid, path).unwrap());
        }
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(native).unwrap()).unwrap();
        assert_eq!(doc["Stop"][0]["hooks"][0]["command"], "user-hook");
    }

    #[test]
    fn droid_preserves_effective_legacy_or_settings_configuration() {
        for name in ["hooks/hooks.json", "settings.json", "hooks.json"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "{}").unwrap();
            assert_eq!(droid_path(dir.path()), path);
            apply_droid_at(dir.path(), "C:/tools/pebrel-hook.exe", true).unwrap();
            assert!(installed_at(AgentHook::Droid, &path).unwrap());
            let raw = std::fs::read_to_string(&path).unwrap();
            assert!(current(AgentHook::Droid, &path, &raw, "C:/tools/pebrel-hook.exe").unwrap());
            if name != "hooks.json" {
                assert!(!dir.path().join("hooks.json").exists());
            }
        }
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(droid_path(dir.path()), dir.path().join("hooks.json"));
    }
}
