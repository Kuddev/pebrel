//! Shared installation policy. Platform adapters supply files and commands;
//! ownership, provider events and feature compatibility have one authority here.

use serde_json::{Value, json};

use super::CodexHookMode;

mod notify;
pub(crate) use notify::{desired_codex_notify, has_codex_notify, stripped_codex_notify};

pub(crate) const CODEX_TURN_EVENTS: &[&str] = &["SessionStart", "UserPromptSubmit", "Stop"];
pub(crate) const CODEX_FULL_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PermissionRequest",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "Interrupt",
    "SessionEnd",
];

/// 0.154.0 is the locally verified full contract, not a guessed historical
/// introduction version. Older clients advertising hooks use the three-event
/// contract; clients without that feature retain notify only.
pub(crate) fn codex_mode(version: &str, features: &str) -> Option<CodexHookMode> {
    if !features.lines().any(|line| line.split_whitespace().next() == Some("hooks")) {
        return None;
    }
    let version = version.trim().strip_prefix("codex-cli ")?;
    let mut parts = version.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: u32 = parts.next()?.split('-').next()?.parse().ok()?;
    Some(if (major, minor, patch) >= (0, 154, 0) {
        CodexHookMode::Full
    } else {
        CodexHookMode::Turns
    })
}

impl CodexHookMode {
    pub(crate) fn argument(self) -> &'static str {
        match self {
            Self::Turns => "--hooks=turns",
            Self::Full => "--hooks=full",
        }
    }

    pub(crate) fn events(self) -> &'static [&'static str] {
        match self {
            Self::Turns => CODEX_TURN_EVENTS,
            Self::Full => CODEX_FULL_EVENTS,
        }
    }
}

/// Use the provider's normal command shell. No hook may emit a permission
/// decision, stdout context or a nonzero status on delivery failure.
pub(crate) fn codex_groups(
    command: &str,
    windows_command: Option<&str>,
    mode: CodexHookMode,
) -> Value {
    let mut groups = serde_json::Map::new();
    for &event in mode.events() {
        let mut handler = json!({"type":"command", "command":command, "timeout":3});
        if let Some(windows) = windows_command {
            handler["commandWindows"] = json!(windows);
        }
        let mut group = json!({"hooks":[handler]});
        if event == "PreToolUse" {
            // User questions are a separate structured tool, not permission
            // requests. Ordinary tools must never become attention events.
            group["matcher"] = json!("^request_user_input$");
        }
        groups.insert(event.to_owned(), json!([group]));
    }
    Value::Object(groups)
}

/// The marker records exact groups we installed. Never claim a group merely
/// because a command contains our product name. Edited groups remain untouched.
pub(crate) fn merge_groups(
    raw: Option<&str>,
    previous: Option<&str>,
    desired: &Value,
) -> Result<(String, String), String> {
    let mut root: Value = match raw {
        Some(raw) => serde_json::from_str(raw).map_err(|error| error.to_string())?,
        None => json!({}),
    };
    let previous: Value = match previous {
        Some(raw) => serde_json::from_str(raw).map_err(|error| error.to_string())?,
        None => json!({}),
    };
    let object = root.as_object_mut().ok_or("hook config must be an object")?;
    let hooks = object.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or("hooks must be an object")?;
    let previous = previous.as_object().ok_or("hook ownership marker must be an object")?;
    let desired = desired.as_object().ok_or("desired hooks must be an object")?;
    for (event, owned) in previous {
        let owned = owned.as_array().ok_or("owned hook groups must be arrays")?;
        let Some(current) = hooks.get_mut(event) else { continue };
        let current = current.as_array_mut().ok_or("hook groups must be arrays")?;
        for group in owned {
            if current.iter().any(|item| item == group) {
                current.retain(|item| item != group);
            } else if !current.is_empty() {
                return Err(format!("edited {event} hook preserved"));
            }
        }
    }
    hooks.retain(|_, value| !value.as_array().is_some_and(Vec::is_empty));
    for (event, groups) in desired {
        let groups = groups.as_array().ok_or("desired hook groups must be arrays")?;
        let current = hooks.entry(event).or_insert_with(|| json!([]));
        let current = current.as_array_mut().ok_or("hook groups must be arrays")?;
        for group in groups {
            if !current.contains(group) {
                current.push(group.clone());
            }
        }
    }
    Ok((
        serde_json::to_string_pretty(&root).map_err(|error| error.to_string())? + "\n",
        serde_json::to_string_pretty(&Value::Object(desired.clone()))
            .map_err(|error| error.to_string())?
            + "\n",
    ))
}

/// Preserve comments and all unrelated TOML. An explicit opt-out wins over
/// auto-installation. Return whether we introduced the feature key so removal
/// can restore it without disabling other integrations.
pub(crate) fn enable_codex_feature(raw: &str) -> Result<Option<(String, bool)>, String> {
    let mut doc = raw.parse::<toml_edit::DocumentMut>().map_err(|error| error.to_string())?;
    for key in ["hooks", "codex_hooks"] {
        if doc.get("features").and_then(|item| item.get(key)).and_then(|item| item.as_bool())
            == Some(false)
        {
            return Ok(None);
        }
    }
    let current = doc.get("features").and_then(|item| item.get("hooks"));
    if current.is_some_and(|item| item.as_bool().is_none()) {
        return Err("features.hooks must be a boolean".into());
    }
    let introduced = current.is_none();
    if introduced {
        if doc.get("features").is_none() {
            doc["features"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        if !doc["features"].is_table_like() {
            return Err("features must be a table".into());
        }
        doc["features"]["hooks"] = toml_edit::value(true);
    }
    Ok(Some((doc.to_string(), introduced)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_advertised_feature_select_the_installed_contract() {
        assert_eq!(codex_mode("codex-cli 0.154.0", "hooks stable true"), Some(CodexHookMode::Full));
        assert_eq!(
            codex_mode("codex-cli 0.120.0", "hooks experimental false"),
            Some(CodexHookMode::Turns)
        );
        assert_eq!(codex_mode("codex-cli 0.154.0", "other stable true"), None);
        assert_eq!(codex_mode("invalid", "hooks stable true"), None);
    }

    #[test]
    fn merge_upgrade_and_remove_preserve_foreign_hooks_and_metadata() {
        let foreign =
            json!({"hooks":[{"type":"command","command":"user-tool"}],"matcher":"custom"});
        let original = json!({"description":"user","hooks":{"Stop":[foreign]}}).to_string();
        let desired = codex_groups("old-helper", None, CodexHookMode::Turns);
        let (first, marker) = merge_groups(Some(&original), None, &desired).unwrap();
        assert_eq!(
            merge_groups(Some(&first), Some(&marker), &desired).unwrap(),
            (first.clone(), marker.clone())
        );
        let full = codex_groups("new-helper", Some("windows-helper"), CodexHookMode::Full);
        let (upgraded, marker) = merge_groups(Some(&first), Some(&marker), &full).unwrap();
        let root: Value = serde_json::from_str(&upgraded).unwrap();
        assert_eq!(root["hooks"]["Stop"].as_array().unwrap().len(), 2);
        let (removed, _) = merge_groups(Some(&upgraded), Some(&marker), &json!({})).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&removed).unwrap(),
            serde_json::from_str::<Value>(&original).unwrap()
        );
    }

    #[test]
    fn edited_and_malformed_hook_configs_are_not_overwritten() {
        let desired = codex_groups("helper", None, CodexHookMode::Full);
        let (raw, marker) = merge_groups(None, None, &desired).unwrap();
        let mut edited: Value = serde_json::from_str(&raw).unwrap();
        edited["hooks"]["Stop"][0]["hooks"][0]["timeout"] = json!(9);
        assert!(merge_groups(Some(&edited.to_string()), Some(&marker), &desired).is_err());
        for bad in ["[]", "{broken", r#"{"hooks":[]}"#, r#"{"hooks":{"Stop":true}}"#] {
            assert!(merge_groups(Some(bad), None, &desired).is_err());
        }
    }

    #[test]
    fn feature_installation_respects_user_opt_out_and_comments() {
        assert!(enable_codex_feature("[features]\nhooks = false\n").unwrap().is_none());
        assert!(enable_codex_feature("[features]\ncodex_hooks = false\n").unwrap().is_none());
        let (result, introduced) =
            enable_codex_feature("# keep\nmodel = 'user-model'\n").unwrap().unwrap();
        assert!(introduced && result.contains("# keep") && result.contains("'user-model'"));
        assert!(!enable_codex_feature(&result).unwrap().unwrap().1);
        assert!(enable_codex_feature("features = 3\n").is_err());
    }
}
