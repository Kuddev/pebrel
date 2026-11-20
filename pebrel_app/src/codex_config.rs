//! Explicit export of one Pebrel provider to Codex CLI configuration.
//!
//! Pebrel normally keeps API keys in the OS credential store. Codex's
//! file auth mode consumes `OPENAI_API_KEY` from `auth.json`, so this module is
//! called only after a second user confirmation and always backs up both live
//! files before replacing them.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value as JsonValue};
use toml_edit::{DocumentMut, value};
use zeroize::Zeroizing;

use crate::ai_providers::AiProvider;

pub fn apply_provider(provider: &AiProvider) -> Result<PathBuf, String> {
    if provider.full_url {
        return Err("Codex requires an API base URL, not a full request URL".into());
    }
    if !provider.wire_api.is_empty() && provider.wire_api != "Responses" {
        return Err("Codex requires a Responses endpoint".to_owned());
    }
    if !provider.kind.uses_openai_protocol() {
        return Err("Codex 需要 Responses/OpenAI 兼容供应商".to_owned());
    }
    let secret = crate::ai_providers::load_api_key(&provider.id)
        .map_err(|_| "无法从凭据管理器读取 API Key".to_owned())?
        .ok_or_else(|| "请先保存 API Key".to_owned())?;
    let key = Zeroizing::new(
        String::from_utf8(secret).map_err(|_| "凭据管理器中的 API Key 编码无效".to_owned())?,
    );
    let home = codex_home().ok_or_else(|| "无法确定 Codex 配置目录".to_owned())?;
    let auth =
        crate::ai_providers::load_auth_document(&provider.id).map_err(|error| error.to_string())?;
    apply_to_dir_with_auth(&home, provider, key.as_str(), auth.as_deref().map(String::as_str))
        .map_err(|error| error.to_string())?;
    Ok(home)
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::platform::dirs::home_dir().map(|home| home.join(".codex")))
}

fn provider_key(provider: &AiProvider) -> String {
    let mut key = String::from("pebrel_");
    for character in provider.id.chars() {
        if character.is_ascii_alphanumeric() {
            key.push(character.to_ascii_lowercase());
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }
    key.trim_end_matches('_').to_owned()
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".pebrel.bak");
    PathBuf::from(name)
}

fn backup(path: &Path) -> io::Result<()> {
    if path.exists() {
        crate::atomic_file::write_private(&backup_path(path), &std::fs::read(path)?)?;
    }
    Ok(())
}

fn restore(path: &Path, old: Option<&[u8]>) {
    match old {
        Some(bytes) => {
            let _ = crate::atomic_file::write_private(path, bytes);
        },
        None => {
            let _ = std::fs::remove_file(path);
        },
    }
}

#[cfg(test)]
fn apply_to_dir(home: &Path, provider: &AiProvider, api_key: &str) -> io::Result<()> {
    apply_to_dir_with_auth(home, provider, api_key, None)
}

fn apply_to_dir_with_auth(
    home: &Path,
    provider: &AiProvider,
    api_key: &str,
    auth_document: Option<&str>,
) -> io::Result<()> {
    std::fs::create_dir_all(home)?;
    let auth_path = home.join("auth.json");
    let config_path = home.join("config.toml");
    // 与 hook 安装/自愈共用同一路径锁，覆盖整个读改写和失败回滚。
    let _config_lock = crate::atomic_file::try_lock(&config_path)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "Codex configuration is busy; retry after hook configuration finishes",
        )
    })?;
    let _auth_lock = crate::atomic_file::try_lock(&auth_path)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::WouldBlock, "Codex authentication configuration is busy")
    })?;
    let read_optional = |path: &Path| match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    };
    let old_auth = read_optional(&auth_path)?;
    let old_config = read_optional(&config_path)?;
    backup(&auth_path)?;
    backup(&config_path)?;

    let mut auth = match old_auth.as_deref() {
        Some(bytes) => serde_json::from_slice::<JsonValue>(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => JsonValue::Object(Map::new()),
    };
    let object = auth.as_object_mut().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Codex auth.json 顶层必须是 JSON 对象")
    })?;
    if let Some(text) = auth_document {
        let document: JsonValue = serde_json::from_str(text).map_err(io::Error::other)?;
        let document =
            document.as_object().ok_or_else(|| io::Error::other("auth.json must be an object"))?;
        for (key, value) in document {
            object.insert(key.clone(), value.clone());
        }
    }
    object.insert("OPENAI_API_KEY".to_owned(), JsonValue::String(api_key.to_owned()));
    let auth_text = Zeroizing::new(
        serde_json::to_string_pretty(&auth)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
    );

    let config_text = match old_config.as_deref() {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => "",
    };
    let mut document = if config_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_text
            .parse::<DocumentMut>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
    };
    if !provider.config_toml.trim().is_empty() {
        let draft = provider.config_toml.parse::<DocumentMut>().map_err(io::Error::other)?;
        merge_config_table(document.as_table_mut(), draft.as_table());
    }
    for name in ["agents", "features", "model_providers"] {
        if document.get(name).is_some_and(|value| value.as_table_like().is_none()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{name} must be a table"),
            ));
        }
    }
    let provider_key = if provider.config_toml.is_empty() {
        provider_key(provider)
    } else {
        document
            .get("model_provider")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| provider_key(provider))
    };
    document["model_provider"] = value(provider_key.clone());
    if document
        .get("model_providers")
        .and_then(|value| value.get(provider_key.as_str()))
        .is_some_and(|value| value.as_table_like().is_none())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The selected model provider must be a table",
        ));
    }
    document["model"] = value(provider.model.trim());
    if provider.reasoning_effort.is_empty() {
        document.remove("model_reasoning_effort");
    } else {
        document["model_reasoning_effort"] = value(provider.reasoning_effort.as_str());
    }
    if let Some(model) = &provider.subagent_model {
        if !crate::ai_providers::configured_models(provider).contains(model) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Subagent model is not configured for this provider",
            ));
        }
        document["agents"]["default_subagent_model"] = value(model.as_str());
    } else if let Some(agents) =
        document.get_mut("agents").and_then(|item| item.as_table_like_mut())
    {
        agents.remove("default_subagent_model");
    }
    document["model_providers"][provider_key.as_str()]["name"] = value(provider.name.trim());
    document["model_providers"][provider_key.as_str()]["base_url"] =
        value(provider.base_url.trim_end_matches('/'));
    document["model_providers"][provider_key.as_str()]["wire_api"] = value("responses");
    document["model_providers"][provider_key.as_str()]["env_key"] = value("OPENAI_API_KEY");
    document["model_providers"][provider_key.as_str()]["requires_openai_auth"] = value(true);
    document["features"]["goals"] = value(provider.codex_goals);
    document["features"]["remote_compaction_v2"] = value(provider.codex_remote_compaction);

    crate::atomic_file::write_private(&auth_path, auth_text.as_bytes())?;
    if let Err(error) =
        crate::atomic_file::write_private(&config_path, document.to_string().as_bytes())
    {
        restore(&auth_path, old_auth.as_deref());
        restore(&config_path, old_config.as_deref());
        return Err(error);
    }
    Ok(())
}

fn merge_config_table(target: &mut dyn toml_edit::TableLike, source: &dyn toml_edit::TableLike) {
    for (key, item) in source.iter() {
        if let Some(source_table) = item.as_table_like()
            && let Some(target_table) = target.get_mut(key).and_then(|v| v.as_table_like_mut())
        {
            merge_config_table(target_table, source_table);
        } else {
            target.insert(key, item.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_providers::ProviderKind;

    #[test]
    fn export_waits_for_hook_lock_and_merges_the_fresh_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "notify = ['original']\n").unwrap();
        let guard = crate::atomic_file::try_lock(&path).unwrap().unwrap();
        let provider = AiProvider::preset(ProviderKind::Custom, "shared-lock");
        let error = std::thread::scope(|scope| {
            scope.spawn(|| apply_to_dir(temp.path(), &provider, "key")).join().unwrap().unwrap_err()
        });
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(!temp.path().join("auth.json").exists());
        crate::atomic_file::write(&path, b"notify = ['hook-updated']\n[features]\nhooks = true\n")
            .unwrap();
        drop(guard);
        apply_to_dir(temp.path(), &provider, "key").unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("hook-updated"));
        assert!(text.contains("hooks = true"));
    }

    #[test]
    fn apply_preserves_unknown_config_and_auth_fields() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("config.toml"),
            "# keep this comment\nunknown_setting = 7\n\n[features]\napps = true\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("auth.json"), r#"{"tokens":{"access_token":"keep"}}"#)
            .unwrap();
        let legacy_backup = temp.path().join("config.toml.nebula.bak");
        std::fs::write(&legacy_backup, b"previous export backup").unwrap();
        let mut provider = AiProvider::preset(ProviderKind::Custom, "custom-7");
        provider.name = "Example".to_owned();
        provider.base_url = "https://example.test/v1".to_owned();
        provider.model = "gpt-test".to_owned();
        provider.codex_goals = true;
        apply_to_dir(temp.path(), &provider, "sk-secret").unwrap();

        let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
        assert!(config.contains("# keep this comment"));
        assert!(config.contains("unknown_setting = 7"));
        assert!(config.contains("apps = true"));
        assert!(config.contains("goals = true"));
        assert!(config.contains("model_provider = \"pebrel_custom_7\""));
        let auth: JsonValue =
            serde_json::from_slice(&std::fs::read(temp.path().join("auth.json")).unwrap()).unwrap();
        assert_eq!(auth["OPENAI_API_KEY"], "sk-secret");
        assert_eq!(auth["tokens"]["access_token"], "keep");
        assert!(backup_path(&temp.path().join("config.toml")).exists());
        assert!(backup_path(&temp.path().join("auth.json")).exists());
        assert_eq!(std::fs::read(legacy_backup).unwrap(), b"previous export backup");
        {
            for name in
                ["auth.json", "config.toml", "auth.json.pebrel.bak", "config.toml.pebrel.bak"]
            {
                crate::platform::private_file::assert_private(&temp.path().join(name));
            }
        }
    }

    #[test]
    fn subagent_export_uses_the_same_provider_and_raw_effort() {
        let temp = tempfile::tempdir().unwrap();
        let mut provider = AiProvider::preset(ProviderKind::Custom, "custom-1");
        provider.models = vec!["fast-model".into()];
        provider.subagent_model = Some("fast-model".into());
        provider.reasoning_effort = "ultracode".into();
        apply_to_dir(temp.path(), &provider, "test-key").unwrap();
        let config = std::fs::read_to_string(temp.path().join("config.toml"))
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(config["model_reasoning_effort"].as_str(), Some("ultracode"));
        assert_eq!(config["agents"]["default_subagent_model"].as_str(), Some("fast-model"));
        assert!(config["agents"].get("model_provider").is_none());
        provider.subagent_model = Some("different-provider-only".into());
        assert!(apply_to_dir(temp.path(), &provider, "test-key").is_err());
    }
}
