//! Cursor 的共享 JSON 配置；只认领完整的自有命令，保留用户 Hook。

use std::io;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{Value, json};

const EVENTS: &[(&str, &str)] = &[
    ("sessionStart", "session-start"),
    ("sessionEnd", "session-end"),
    ("beforeSubmitPrompt", "prompt"),
    ("stop", "done"),
    ("afterShellExecution", "tool-complete"),
];

pub(super) fn path() -> Option<PathBuf> {
    Some(crate::platform::dirs::home_dir()?.join(".cursor/hooks.json"))
}

fn command(helper: &str, event: &str) -> String {
    super::extended::encoded_powershell(&format!(
        "& '{}' cursor --event {event}",
        helper.replace('\'', "''"),
    ))
}

fn owned(command: &str) -> bool {
    let Some(encoded) =
        command.strip_prefix("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand ")
    else {
        return false;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else { return false };
    if bytes.len() % 2 != 0 {
        return false;
    }
    let words: Vec<_> =
        bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let Ok(script) = String::from_utf16(&words) else { return false };
    let Some((escaped, tail)) = script.strip_prefix("& '").and_then(|s| s.rsplit_once("' ")) else {
        return false;
    };
    let helper = escaped.replace("''", "'");
    if helper.replace('\'', "''") != escaped {
        return false;
    }
    let filename = helper.rsplit(['/', '\\']).next().unwrap_or("");
    ["pebrel-hook.exe", "nebula-hook.exe"].iter().any(|name| filename.eq_ignore_ascii_case(name))
        && EVENTS.iter().any(|(_, event)| tail == format!("cursor --event {event}"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn edit(raw: &str, helper: &str, enabled: bool) -> io::Result<(String, bool, bool)> {
    let mut doc: Value = serde_json::from_str(raw).map_err(io::Error::other)?;
    let root = doc.as_object_mut().ok_or_else(|| invalid("Expected a JSON object"))?;
    if root.get("version").is_some_and(|version| version != 1) {
        return Err(invalid("Unsupported hooks configuration version; file preserved"));
    }
    let mut changed = false;
    let mut installed = false;
    if !root.contains_key("hooks") && enabled {
        root.insert("hooks".into(), json!({}));
        changed = true;
    }
    if let Some(hooks) = root.get_mut("hooks") {
        let hooks = hooks.as_object_mut().ok_or_else(|| invalid("Expected a hooks object"))?;
        for (native, event) in EVENTS {
            if !hooks.contains_key(*native) && !enabled {
                continue;
            }
            let entries = hooks
                .entry(*native)
                .or_insert(json!([]))
                .as_array_mut()
                .ok_or_else(|| invalid("Expected a hook command array; file preserved"))?;
            let is_owned =
                |entry: &Value| entry.get("command").and_then(Value::as_str).is_some_and(owned);
            let ours: Vec<_> = entries.iter().filter(|entry| is_owned(entry)).collect();
            installed |= !ours.is_empty();
            let desired = json!({"command": command(helper, event), "timeout": 10});
            if (enabled && ours == vec![&desired]) || (!enabled && ours.is_empty()) {
                continue;
            }
            entries.retain(|entry| !is_owned(entry));
            if enabled {
                entries.push(desired);
            }
            changed = true;
        }
    }
    if enabled && root.get("version").is_none() {
        root.insert("version".into(), json!(1));
        changed = true;
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
    if !enabled && !path.exists() {
        return Ok(());
    }
    let _lock = crate::atomic_file::try_lock(path)?
        .ok_or_else(|| io::Error::other("Cursor hook configuration is busy"))?;
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
    fn preserves_foreign_hooks_and_migrates_old_helpers_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        let original =
            r#"{"version":1,"custom":true,"hooks":{"stop":[{"command":"foreign-hook"}]}}"#;
        std::fs::write(&path, original).unwrap();
        apply_at(&path, "C:/old/nebula-hook.exe", true).unwrap();
        assert!(installed_at(&path).unwrap());
        assert!(!current(&std::fs::read_to_string(&path).unwrap(), HELPER).unwrap());
        apply_at(&path, HELPER, true).unwrap();
        let installed = std::fs::read_to_string(&path).unwrap();
        assert!(current(&installed, HELPER).unwrap());
        assert_eq!(installed.matches("EncodedCommand").count(), EVENTS.len());
        apply_at(&path, HELPER, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), installed);
        apply_at(&path, "", false).unwrap();
        assert!(!installed_at(&path).unwrap());
        let removed = std::fs::read_to_string(&path).unwrap();
        assert!(removed.contains("foreign-hook") && removed.contains("custom"));
        apply_at(&path, "", false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), removed);
    }

    #[test]
    fn malformed_future_and_busy_configuration_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        for raw in ["broken", r#"{"version":2,"hooks":{}}"#, r#"{"hooks":{"stop":42}}"#] {
            std::fs::write(&path, raw).unwrap();
            assert!(apply_at(&path, HELPER, true).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        }
        std::fs::write(&path, "{}").unwrap();
        let _lock = crate::atomic_file::try_lock(&path).unwrap().unwrap();
        assert!(apply_at(&path, HELPER, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    #[test]
    fn edited_commands_are_not_claimed_by_filename_substring() {
        assert!(owned(&command(HELPER, "done")));
        for script in [
            "echo 'pebrel-hook.exe'",
            "& 'C:/pebrel-hook.exe' cursor --event done; echo custom",
            "& 'C:/other.exe' cursor --event done",
            "& 'C:/pebrel-hook.exe' other --event done",
        ] {
            assert!(!owned(&super::super::extended::encoded_powershell(script)));
        }
    }
}
