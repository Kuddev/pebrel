//! Antigravity CLI's global `hooks.json`. One named group holds every entry we own; any other
//! group, and a group of that name that is not entirely ours, is preserved.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::extended::{command_for, owns_command};

const GROUP: &str = "pebrel";

/// Antigravity event and helper event. The payload carries no event name, so each command
/// passes it as an argument. Tool events are left out: 1.2.7 was observed ignoring them.
const EVENTS: &[(&str, &str)] =
    &[("SessionStart", "session-start"), ("PreInvocation", "tool-complete"), ("Stop", "done")];

pub(super) fn directory() -> Option<PathBuf> {
    Some(crate::platform::dirs::home_dir()?.join(".gemini/antigravity-cli"))
}

pub(super) fn path() -> Option<PathBuf> {
    Some(crate::platform::dirs::home_dir()?.join(".gemini/config/hooks.json"))
}

fn command(helper: &str, event: &str) -> String {
    command_for(helper, &format!("antigravity --event {event}"))
}

fn owned(entry: &Value) -> bool {
    entry.get("command").and_then(Value::as_str).is_some_and(|command| {
        EVENTS
            .iter()
            .any(|(_, event)| owns_command(command, &format!("antigravity --event {event}")))
    })
}

fn entries<'a>(group: &'a Value, event: &str) -> &'a [Value] {
    group.get(event).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn holds_ours(group: &Value) -> bool {
    EVENTS.iter().any(|(event, _)| entries(group, event).iter().any(owned))
}

/// Only `enabled` and our events, each holding nothing but our commands.
fn only_ours(group: &Value) -> bool {
    group.as_object().is_some_and(|group| {
        group.iter().all(|(key, value)| {
            key == "enabled"
                || (EVENTS.iter().any(|(event, _)| event == key)
                    && value.as_array().is_some_and(|entries| entries.iter().all(owned)))
        })
    })
}

fn desired_group(helper: &str) -> Value {
    let mut group = json!({"enabled": true});
    for (event, native) in EVENTS {
        group[*event] =
            json!([{"type": "command", "command": command(helper, native), "timeout": 10}]);
    }
    group
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Returns the edited text, whether the input held our hooks, and whether the edit changed it.
fn edit(raw: &str, helper: &str, enabled: bool) -> io::Result<(String, bool, bool)> {
    let mut doc: Value = serde_json::from_str(raw).map_err(io::Error::other)?;
    let root = doc.as_object_mut().ok_or_else(|| invalid("Expected a JSON object"))?;
    let desired = desired_group(helper);
    let current = root.get(GROUP).cloned();
    let installed = current.as_ref().is_some_and(holds_ours);
    let mut changed = false;
    match current {
        None if enabled => {
            root.insert(GROUP.into(), desired);
            changed = true;
        },
        Some(group) if only_ours(&group) => {
            if !enabled {
                root.shift_remove(GROUP);
                changed = true;
            } else if group != desired {
                root.insert(GROUP.into(), desired);
                changed = true;
            }
        },
        Some(group) if enabled || holds_ours(&group) => {
            return Err(invalid("Preserving an edited integration; file preserved"));
        },
        _ => {},
    }
    let text = if changed {
        serde_json::to_string_pretty(&doc).map_err(io::Error::other)? + "\n"
    } else {
        raw.to_owned()
    };
    Ok((text, installed, changed))
}

pub(super) fn installed_at(path: &Path) -> io::Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(edit(&raw, "", false)?.1),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn current(raw: &str, helper: &str) -> io::Result<bool> {
    Ok(!edit(raw, helper, true)?.2)
}

pub(super) fn apply_at(path: &Path, helper: &str, enabled: bool) -> io::Result<()> {
    if enabled && helper.is_empty() {
        return Err(io::Error::other("The Pebrel hook helper is missing from this installation."));
    }
    if !enabled && !path.exists() {
        return Ok(());
    }
    let _lock = crate::atomic_file::try_lock(path)?
        .ok_or_else(|| io::Error::other("Antigravity hook configuration is busy"))?;
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => "{}".into(),
        Err(error) => return Err(error),
    };
    let (updated, _, changed) = edit(&raw, helper, enabled)?;
    if changed {
        crate::atomic_file::write(path, updated.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELPER: &str = "C:/Program Files/用户's Tools/pebrel-hook.exe";

    #[test]
    fn owns_one_named_group_and_preserves_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/hooks.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original =
            r#"{"lint":{"enabled":true,"Stop":[{"type":"command","command":"foreign"}]},"note":1}"#;
        std::fs::write(&path, original).unwrap();
        apply_at(&path, "C:/old/nebula-hook.exe", true).unwrap();
        assert!(installed_at(&path).unwrap());
        assert!(!current(&std::fs::read_to_string(&path).unwrap(), HELPER).unwrap());
        apply_at(&path, HELPER, true).unwrap();
        let installed = std::fs::read_to_string(&path).unwrap();
        assert!(current(&installed, HELPER).unwrap());
        let parsed: Value = serde_json::from_str(&installed).unwrap();
        assert_eq!(parsed["pebrel"]["enabled"], true);
        assert_eq!(parsed["pebrel"]["Stop"][0]["command"], command(HELPER, "done"));
        assert_eq!(
            parsed["pebrel"]["PreInvocation"][0]["command"],
            command(HELPER, "tool-complete")
        );
        assert_eq!(parsed["lint"]["Stop"][0]["command"], "foreign");
        apply_at(&path, HELPER, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), installed);
        apply_at(&path, "", false).unwrap();
        assert!(!installed_at(&path).unwrap());
        let removed = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&removed).unwrap(),
            serde_json::from_str::<Value>(original).unwrap()
        );
    }

    #[test]
    fn a_group_with_the_same_name_that_is_not_ours_is_never_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        let mine = command(HELPER, "done");
        for group in [
            json!({"enabled": true, "Stop": [{"type": "command", "command": "foreign"}]}),
            json!({"Stop": [{"command": mine}, {"command": "foreign"}]}),
        ] {
            let raw = json!({"pebrel": group}).to_string();
            std::fs::write(&path, &raw).unwrap();
            assert!(apply_at(&path, HELPER, true).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        }
        let foreign = r#"{"pebrel":{"Stop":[{"command":"foreign"}]}}"#;
        std::fs::write(&path, foreign).unwrap();
        apply_at(&path, "", false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);
    }

    #[test]
    fn malformed_and_busy_configuration_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        for raw in ["broken", "[]"] {
            std::fs::write(&path, raw).unwrap();
            assert!(apply_at(&path, HELPER, true).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        }
        std::fs::write(&path, "{}").unwrap();
        let _lock = crate::atomic_file::try_lock(&path).unwrap().unwrap();
        assert!(apply_at(&path, HELPER, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        assert!(apply_at(&path, "", true).is_err());
    }
}
