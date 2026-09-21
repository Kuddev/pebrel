//! Native hook adapters for additional agents.
//! Shared preferences and managed-file ownership govern installation and removal.

use std::io;
use std::path::PathBuf;

use nebula_settings::AgentHook;
use serde_json::{Map, json};

use super::managed_files;

pub(super) fn directory(agent: AgentHook) -> Option<PathBuf> {
    if agent == AgentHook::Copilot {
        if let Some(dir) = std::env::var_os("COPILOT_HOME").filter(|dir| !dir.is_empty()) {
            return Some(PathBuf::from(dir));
        }
    }
    let home = crate::platform::dirs::home_dir()?;
    Some(match agent {
        AgentHook::Copilot => home.join(".copilot"),
        AgentHook::Grok => home.join(".grok"),
        AgentHook::OhMyPi => home.join(".omp/agent"),
        _ => return None,
    })
}

pub(super) fn path(agent: AgentHook) -> Option<PathBuf> {
    let suffix =
        if agent == AgentHook::OhMyPi { "extensions/pebrel.ts" } else { "hooks/pebrel.json" };
    Some(directory(agent)?.join(suffix))
}

pub(super) fn content(agent: AgentHook, helper: &str) -> io::Result<String> {
    if agent == AgentHook::OhMyPi {
        return Ok(super::PI_EXTENSION_TS
            .replace("Pebrel ↔ Pi", "Pebrel ↔ Oh My Pi")
            .replace("@earendil-works/pi-coding-agent", "@oh-my-pi/pi-coding-agent")
            .replace("const SOURCE: string = \"pi\";", "const SOURCE: string = \"omp\";"));
    }
    let source = match agent {
        AgentHook::Copilot => "copilot",
        AgentHook::Grok => "grok",
        _ => return Err(io::Error::other("Unsupported managed hook provider")),
    };
    let mut hooks = Map::new();
    let events = if agent == AgentHook::Copilot {
        vec![
            ("sessionStart", "session-start"),
            ("userPromptSubmitted", "prompt"),
            ("agentStop", "done"),
            ("postToolUse", "tool-complete"),
            ("postToolUseFailure", "tool-complete"),
            ("errorOccurred", "error"),
            ("sessionEnd", "session-end"),
            ("notification", "notification"),
        ]
    } else {
        vec![
            ("SessionStart", "session-start"),
            ("UserPromptSubmit", "prompt"),
            ("PostToolUse", "tool-complete"),
            ("PostToolUseFailure", "tool-complete"),
            ("Stop", "done"),
            ("StopFailure", "failed"),
            ("SessionEnd", "session-end"),
            ("Notification", "notification"),
        ]
    };
    for (native, event) in events {
        // Copilot has explicit shell fields. Grok executes command through a
        // shell; an encoded PowerShell command avoids path quoting differences
        // between cmd, PowerShell and Git Bash on Windows.
        let powershell = format!("& '{}' {source} --event {event}", helper.replace('\'', "''"));
        let command = encoded_powershell(&powershell);
        let value = if agent == AgentHook::Copilot {
            json!([{ "type": "command", "bash": command, "powershell": powershell, "timeoutSec": 10 }])
        } else {
            // Grok 文档的 matcher 匹配工具名；通知类别在载荷解析端筛选。
            json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 10 }] }])
        };
        hooks.insert(native.to_owned(), value);
    }
    let mut root = json!({ "hooks": hooks });
    if agent == AgentHook::Copilot {
        root["version"] = json!(1);
    }
    serde_json::to_string_pretty(&root).map_err(io::Error::other)
}

pub(super) fn encoded_powershell(script: &str) -> String {
    use base64::Engine as _;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

pub(super) fn installed(agent: AgentHook, helper: &str) -> io::Result<bool> {
    let Some(path) = path(agent) else { return Ok(false) };
    let legacy =
        path.with_file_name(if agent == AgentHook::OhMyPi { "nebula.ts" } else { "nebula.json" });
    managed_files::installed(&path, &legacy, &content(agent, helper)?, &[])
}

pub(super) fn apply(agent: AgentHook, helper: &str, enabled: bool) -> io::Result<()> {
    let path =
        path(agent).ok_or_else(|| io::Error::other("Agent configuration directory unavailable"))?;
    let legacy =
        path.with_file_name(if agent == AgentHook::OhMyPi { "nebula.ts" } else { "nebula.json" });
    let content = content(agent, helper)?;
    if !enabled && !path.exists() && !legacy.exists() {
        return Ok(());
    }
    let _lock = crate::atomic_file::try_lock(&path)?
        .ok_or_else(|| io::Error::other("Hook configuration is busy"))?;
    if enabled {
        if managed_files::install(&path, &legacy, &content, &[])?
            == managed_files::Install::Conflict
        {
            return Err(io::Error::other(format!(
                "Preserving edited or unmanaged integration: {}",
                path.display()
            )));
        }
    } else {
        managed_files::remove(&path, &legacy, &content, &[])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn native_event_names_and_shell_paths_are_preserved() {
        let helper = "C:/Program Files/用户's Tools/pebrel-hook.exe";
        let copilot: Value =
            serde_json::from_str(&content(AgentHook::Copilot, helper).unwrap()).unwrap();
        assert_eq!(copilot["version"], 1);
        let command = &copilot["hooks"]["agentStop"][0];
        assert!(command["powershell"].as_str().unwrap().contains("用户''s Tools"));
        assert!(command["powershell"].as_str().unwrap().ends_with("copilot --event done"));
        let grok: Value = serde_json::from_str(&content(AgentHook::Grok, helper).unwrap()).unwrap();
        assert!(grok["hooks"]["Notification"][0].get("matcher").is_none());
        assert!(grok["hooks"]["StopFailure"].is_array());
        assert!(
            grok["hooks"]["Stop"][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .starts_with("powershell.exe ")
        );
        let omp = content(AgentHook::OhMyPi, helper).unwrap();
        assert!(omp.contains("@oh-my-pi/pi-coding-agent"));
        assert!(omp.contains("const SOURCE: string = \"omp\";"));
        assert!(!omp.contains("const SOURCE: string = \"pi\";"));
        assert!(omp.contains("if (SOURCE !== \"pi\") return false;"));
    }
}
