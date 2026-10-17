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
    Some(directory(agent)?.join("settings.json"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Returns the edited text, whether the input held our hooks, and whether the edit changed it.
fn edit(
    agent: AgentHook,
    raw: &str,
    helper: &str,
    enabled: bool,
) -> io::Result<(String, bool, bool)> {
    let (source, _, events) = provider(agent)?;
    let mut doc: Value = serde_json::from_str(raw).map_err(io::Error::other)?;
    let root = doc.as_object_mut().ok_or_else(|| invalid("Expected a JSON object"))?;
    let mut changed = false;
    let mut installed = false;
    if !root.contains_key("hooks") && enabled {
        root.insert("hooks".into(), json!({}));
        changed = true;
    }
    if let Some(hooks) = root.get_mut("hooks") {
        let hooks = hooks.as_object_mut().ok_or_else(|| invalid("Expected a hooks object"))?;
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
        Ok(raw) => Ok(edit(agent, &raw, "", false)?.1),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn current(agent: AgentHook, raw: &str, helper: &str) -> io::Result<bool> {
    Ok(!edit(agent, raw, helper, true)?.2)
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
    let (updated, _, changed) = edit(agent, &raw, helper, enabled)?;
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
            assert!(!current(agent, &std::fs::read_to_string(&path).unwrap(), HELPER).unwrap());
            apply_at(agent, &path, HELPER, true).unwrap();
            let installed = std::fs::read_to_string(&path).unwrap();
            assert!(current(agent, &installed, HELPER).unwrap());
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
        let text = edit(AgentHook::Droid, "{}", HELPER, true).unwrap().0;
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
