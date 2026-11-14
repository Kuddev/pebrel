//! CLI 配置导出保持独立于 UI；只在明确应用时写入，并保留原文件备份。

use super::*;
use std::path::Path;

pub fn apply_claude_provider(provider: &AiProvider) -> Result<PathBuf, String> {
    if provider.kind != ProviderKind::Anthropic {
        return Err("Claude Code requires an Anthropic Messages endpoint".into());
    }
    if provider
        .subagent_model
        .as_ref()
        .is_some_and(|model| !configured_models(provider).contains(model))
    {
        return Err("Subagent model is not configured for this provider".into());
    }
    let key = Zeroizing::new(
        String::from_utf8(
            load_api_key(&provider.id)
                .map_err(|_| "Credential read failed")?
                .ok_or("Save an API key first")?,
        )
        .map_err(|_| "Invalid credential encoding")?,
    );
    let home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| crate::platform::dirs::home_dir().map(|p| p.join(".claude")))
        .ok_or("Claude configuration directory is not available")?;
    write_claude_settings(&home, provider, &key).map_err(|error| error.to_string())?;
    Ok(home)
}

fn write_claude_settings(home: &Path, provider: &AiProvider, key: &str) -> io::Result<()> {
    let path = home.join("settings.json");
    std::fs::create_dir_all(home)?;
    let _lock = crate::atomic_file::try_lock(&path)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "Claude configuration is busy; retry after hook configuration finishes",
        )
    })?;
    let old = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut config: serde_json::Value = match old.as_ref() {
        Some(bytes) => serde_json::from_slice(bytes).map_err(io::Error::other)?,
        None => serde_json::json!({}),
    };
    let object = config
        .as_object_mut()
        .ok_or_else(|| io::Error::other("settings.json must be an object"))?;
    let env = object
        .entry("env")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| io::Error::other("settings.env must be an object"))?;
    env.insert("ANTHROPIC_API_KEY".into(), key.into());
    env.remove("ANTHROPIC_AUTH_TOKEN");
    env.insert(
        "ANTHROPIC_BASE_URL".into(),
        provider
            .base_url
            .trim_end_matches('/')
            .strip_suffix("/v1")
            .unwrap_or(provider.base_url.trim_end_matches('/'))
            .into(),
    );
    env.insert("ANTHROPIC_MODEL".into(), provider.model.as_str().into());
    for (name, value) in [
        ("CLAUDE_CODE_SUBAGENT_MODEL", provider.subagent_model.as_deref().unwrap_or("")),
        ("CLAUDE_CODE_EFFORT_LEVEL", provider.reasoning_effort.as_str()),
    ] {
        if value.is_empty() {
            env.remove(name);
        } else {
            env.insert(name.into(), value.into());
        }
    }
    // 选择的是默认值，保留 CLI 的显式子 Agent 模型覆盖能力。
    env.remove("CLAUDE_CODE_SUBAGENT_MODEL_FORCE");
    let text = Zeroizing::new(serde_json::to_string_pretty(&config).map_err(io::Error::other)?);
    if let Some(old) = old {
        crate::atomic_file::write_private(&home.join("settings.json.pebrel.bak"), &old)?;
    }
    crate::atomic_file::write_private(&path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_export_respects_hook_writer_and_preserves_its_update() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();
        let guard = crate::atomic_file::try_lock(&path).unwrap().unwrap();
        let provider = AiProvider::preset(ProviderKind::Anthropic, "shared-lock");
        let error = std::thread::scope(|scope| {
            scope
                .spawn(|| write_claude_settings(temp.path(), &provider, "key"))
                .join()
                .unwrap()
                .unwrap_err()
        });
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        crate::atomic_file::write(&path, br#"{"hooks":{"Stop":[{"command":"hook-updated"}]}}"#)
            .unwrap();
        drop(guard);
        write_claude_settings(temp.path(), &provider, "key").unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("hook-updated"));
    }

    #[test]
    fn claude_export_keeps_hooks_and_uses_same_provider_subagent_model() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("settings.json"),
            r#"{"hooks":{"Stop":[]},"env":{"KEEP":"yes"}}"#,
        )
        .unwrap();
        let mut provider = AiProvider::preset(ProviderKind::Anthropic, "claude");
        provider.models = vec!["claude-small".into()];
        provider.subagent_model = Some("claude-small".into());
        provider.reasoning_effort = "max".into();
        write_claude_settings(temp.path(), &provider, "test-key").unwrap();
        let result: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temp.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(result["hooks"]["Stop"], serde_json::json!([]));
        assert_eq!(result["env"]["KEEP"], "yes");
        assert_eq!(result["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "claude-small");
        assert_eq!(result["env"]["CLAUDE_CODE_EFFORT_LEVEL"], "max");
        assert_eq!(result["env"]["ANTHROPIC_BASE_URL"], "https://api.anthropic.com");
        assert!(temp.path().join("settings.json.pebrel.bak").exists());
        {
            for name in ["settings.json", "settings.json.pebrel.bak"] {
                crate::platform::private_file::assert_private(&temp.path().join(name));
            }
        }
    }
}
