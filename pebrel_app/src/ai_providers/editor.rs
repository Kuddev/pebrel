//! 编辑器只持久化非密文 TOML；auth.json 草稿的密钥交给系统凭据存储。

use super::*;
use toml_edit::{DocumentMut, value};

pub fn config_text(provider: &AiProvider) -> String {
    let mut doc = provider.config_toml.parse::<DocumentMut>().unwrap_or_default();
    if let Some(providers) = doc.get("model_providers")
        && (providers.as_table_like().is_none()
            || providers.as_table_like().is_some_and(|table| {
                table.iter().any(|(_, entry)| entry.as_table_like().is_none())
            }))
    {
        return provider.config_toml.clone();
    }
    let key = doc.get("model_provider").and_then(|v| v.as_str()).unwrap_or(&provider.id).to_owned();
    doc["model_provider"] = value(key.as_str());
    doc["model"] = value(provider.model.as_str());
    if provider.reasoning_effort.is_empty() {
        doc.remove("model_reasoning_effort");
    } else {
        doc["model_reasoning_effort"] = value(provider.reasoning_effort.as_str());
    }
    doc["model_providers"][key.as_str()]["base_url"] = value(provider.base_url.as_str());
    doc.to_string()
}

pub fn apply_config_draft(provider: &mut AiProvider, text: &str) -> Result<(), String> {
    let doc = parse_config_draft(text)?;
    if let Some(model) = doc.get("model") {
        provider.models = configured_models(provider);
        provider.model = model.as_str().ok_or("model must be a string")?.to_owned();
    }
    provider.reasoning_effort = doc
        .get("model_reasoning_effort")
        .map(|v| v.as_str().ok_or("model_reasoning_effort must be a string"))
        .transpose()?
        .unwrap_or("")
        .to_owned();
    let key = doc.get("model_provider").and_then(|v| v.as_str()).unwrap_or(&provider.id);
    if let Some(base) = doc
        .get("model_providers")
        .and_then(|v| v.get(key))
        .and_then(|v| v.get("base_url"))
        .or_else(|| doc.get("base_url"))
    {
        provider.base_url = base.as_str().ok_or("base_url must be a string")?.to_owned();
    }
    provider.config_toml = text.to_owned();
    Ok(())
}

pub(super) fn parse_config_draft(text: &str) -> Result<DocumentMut, String> {
    let doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    for name in ["agents", "features"] {
        if doc.get(name).is_some_and(|value| value.as_table_like().is_none()) {
            return Err(format!("{name} must be a table"));
        }
    }
    if doc.get("model_provider").is_some_and(|v| !v.is_str()) {
        return Err("model_provider must be a string".into());
    }
    if let Some(providers) = doc.get("model_providers") {
        let providers = providers.as_table_like().ok_or("model_providers must be a table")?;
        for (_, provider) in providers.iter() {
            let provider = provider.as_table_like().ok_or("Each model provider must be a table")?;
            // 任意供应商都可以使用自定义鉴权头，不能靠枚举 token 名称判断安全。
            // 环境变量引用不包含密钥本身，仍通过 env_http_headers 保留。
            if provider.contains_key("http_headers") {
                return Err(
                    "Literal model provider http_headers cannot be stored in provider metadata; use env_http_headers to reference environment variables".into(),
                );
            }
        }
    }
    // 凭据有独立编辑区，不能随着普通供应商元数据落盘。
    for forbidden in ["api_key", "experimental_bearer_token", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"]
    {
        if has_key(doc.as_table(), forbidden) {
            return Err(format!("{forbidden}: enter credentials in auth.json"));
        }
    }
    Ok(doc)
}

fn has_key(table: &dyn toml_edit::TableLike, key: &str) -> bool {
    table.iter().any(|(name, value)| name.eq_ignore_ascii_case(key) || item_has_key(value, key))
}

fn item_has_key(item: &toml_edit::Item, key: &str) -> bool {
    match item {
        toml_edit::Item::Table(table) => has_key(table, key),
        toml_edit::Item::ArrayOfTables(tables) => tables.iter().any(|table| has_key(table, key)),
        toml_edit::Item::Value(value) => value_has_key(value, key),
        _ => false,
    }
}
fn value_has_key(value: &toml_edit::Value, key: &str) -> bool {
    match value {
        toml_edit::Value::InlineTable(table) => has_key(table, key),
        toml_edit::Value::Array(values) => values.iter().any(|value| value_has_key(value, key)),
        _ => false,
    }
}

pub fn parse_auth_draft(text: &str, hint: &str) -> Result<Option<Zeroizing<String>>, String> {
    let object: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let object = object.as_object().ok_or("auth.json must contain a JSON object")?;
    let value = object.get("OPENAI_API_KEY").or_else(|| object.get("ANTHROPIC_API_KEY"));
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.as_str().ok_or("API key must be a string")?.trim();
    if value.is_empty() || value == hint {
        return Ok(None);
    }
    Ok(Some(Zeroizing::new(value.to_owned())))
}

pub fn load_auth_document(id: &str) -> io::Result<Option<Zeroizing<String>>> {
    let bytes =
        crate::ssh_credentials::load_generic_secret(&format!("{}/auth", credential_target(id)))?;
    bytes
        .map(|bytes| String::from_utf8(bytes).map(Zeroizing::new).map_err(io::Error::other))
        .transpose()
}

fn auth_document(
    provider: &AiProvider,
    text: &str,
    previous_key: Option<&[u8]>,
) -> io::Result<Zeroizing<String>> {
    let mut document: serde_json::Value = serde_json::from_str(text).map_err(io::Error::other)?;
    let object =
        document.as_object_mut().ok_or_else(|| io::Error::other("auth.json must be an object"))?;
    // 提示值只表示旧密钥；密文存储需要还原它，避免把提示值当成凭据。
    for name in ["OPENAI_API_KEY", "ANTHROPIC_API_KEY"] {
        if object
            .get(name)
            .and_then(|v| v.as_str())
            .is_some_and(|value| !value.is_empty() && value == provider.api_key_hint)
            && let Some(bytes) = previous_key
        {
            let key = std::str::from_utf8(bytes).map_err(io::Error::other)?;
            object.insert(name.into(), serde_json::Value::String(key.to_string()));
        }
    }
    // Linux Secret Service 的通用凭据适配器按单行读取；紧凑 JSON 不改变文档语义。
    let text = Zeroizing::new(serde_json::to_string(&document).map_err(io::Error::other)?);
    if text.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "auth.json exceeds the 4096-byte credential limit",
        ));
    }
    Ok(text)
}

/// 凭据与元数据作为一次用户保存提交；任何后续失败都恢复本次改过的凭据。
pub fn save_provider_draft(
    mut store: ProviderStore,
    mut provider: AiProvider,
    text: &str,
) -> io::Result<ProviderStore> {
    let index = store
        .providers
        .iter()
        .position(|p| p.id == provider.id)
        .ok_or_else(|| io::Error::other("Provider no longer exists"))?;
    let key = parse_auth_draft(text, &provider.api_key_hint).map_err(io::Error::other)?;
    let key_target = credential_target(&provider.id);
    let auth_target = format!("{key_target}/auth");
    let old_key = load_api_key(&provider.id)?.map(Zeroizing::new);
    // 先完成格式和长度验证，避免第二份凭据被拒绝时第一份已经变化。
    let auth = auth_document(&provider, text, old_key.as_deref().map(Vec::as_slice))?;
    let old_auth = crate::ssh_credentials::load_generic_secret(&auth_target)?.map(Zeroizing::new);
    let mut auth_written = false;
    let mut key_written = false;
    let result = (|| {
        crate::ssh_credentials::store_generic_secret(&auth_target, auth.as_bytes())?;
        auth_written = true;
        provider.auth_document_set = true;
        if let Some(key) = key {
            store_provider_api_key(&mut provider, &key)?;
            key_written = true;
        }
        store.providers[index] = provider;
        save(&store)
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (written, target, old) in
            [(key_written, key_target, old_key), (auth_written, auth_target, old_auth)]
        {
            if !written {
                continue;
            }
            let restored = match old {
                Some(bytes) => crate::ssh_credentials::store_generic_secret(&target, &bytes),
                None => crate::ssh_credentials::delete_generic_secret(&target),
            };
            if let Err(rollback) = restored {
                failures.push(rollback.to_string());
            }
        }
        return if failures.is_empty() {
            Err(error)
        } else {
            Err(io::Error::other(format!(
                "{error}; credential restore failed: {}",
                failures.join("; ")
            )))
        };
    }
    Ok(store)
}

pub(super) fn delete_auth_document(id: &str) -> io::Result<()> {
    crate::ssh_credentials::delete_generic_secret(&format!("{}/auth", credential_target(id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_document_is_single_line_bounded_and_restores_only_hints() {
        let mut provider = AiProvider::preset(ProviderKind::Custom, "test");
        provider.api_key_hint = "••••1234".into();
        let text = auth_document(
            &provider,
            "{\n\"OPENAI_API_KEY\":\"••••1234\",\"label\":\"中文\\n第二行\"\n}",
            Some(b"old-key"),
        )
        .unwrap();
        assert!(!text.contains(['\n', '\r']));
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["OPENAI_API_KEY"], "old-key");
        assert_eq!(parsed["label"], "中文\n第二行");
        let large = serde_json::json!({"OPENAI_API_KEY": "a".repeat(4096)}).to_string();
        assert!(auth_document(&provider, &large, None).is_err());
    }
    #[test]
    fn editable_config_preserves_unknown_fields_and_updates_model_route() {
        let mut provider = AiProvider::preset(ProviderKind::Custom, "one");
        let text = "model = 'new-model'\nmodel_provider = 'custom'\nmodel_reasoning_effort = 'xhigh'\ncustom_flag = true\n[model_providers.custom]\nbase_url = 'https://example.test/v1'\n";
        apply_config_draft(&mut provider, text).unwrap();
        assert_eq!(provider.model, "new-model");
        assert_eq!(provider.base_url, "https://example.test/v1");
        assert_eq!(provider.reasoning_effort, "xhigh");
        assert!(config_text(&provider).contains("custom_flag = true"));
        assert!(apply_config_draft(&mut provider, "model = [").is_err());
        assert!(apply_config_draft(&mut provider, "model_providers='invalid'").is_err());
        assert!(apply_config_draft(&mut provider, "model_provider=42").is_err());
        assert!(apply_config_draft(&mut provider, "secrets=[{OPENAI_API_KEY='hidden'}]").is_err());
        assert!(apply_config_draft(&mut provider, "[[secrets]]\napi_key='hidden'").is_err());
        assert!(
            apply_config_draft(
                &mut provider,
                "[model_providers.custom]\nexperimental_bearer_token='secret'"
            )
            .is_err()
        );
    }

    #[test]
    fn editable_config_rejects_literal_headers_before_changing_metadata() {
        for text in [
            "[model_providers.custom.http_headers]\nAuthorization = 'Bearer test-secret'\n",
            "[model_providers.custom]\nhttp_headers = { x-api-key = 'test-secret' }\n",
            "model_providers = { custom = { http_headers = { X-Tenant-Credential = 'test-secret' } } }\n",
            "model_providers.custom.http_headers.X-Tenant-Credential = 'test-secret'\n",
            "model_provider = 'main'\n[model_providers.other.http_headers]\nX-Tenant-Credential = 'test-secret'\n",
        ] {
            let mut provider = AiProvider::preset(ProviderKind::Custom, "custom");
            let original = provider.clone();
            let error = apply_config_draft(&mut provider, text).unwrap_err();
            assert!(error.contains("env_http_headers"), "{error}");
            assert!(!error.contains("test-secret"));
            assert_eq!(provider, original);
        }
    }

    #[test]
    fn editable_config_preserves_environment_header_references() {
        for headers in [
            "[model_providers.custom.env_http_headers]\nAuthorization = 'ACCESS_TOKEN'\nx-api-key = 'OPENAI_API_KEY'\n",
            "[model_providers.custom]\nenv_http_headers = { Authorization = 'ACCESS_TOKEN', X-Tenant-Credential = 'TENANT_TOKEN' }\n",
        ] {
            let text = format!("model_provider = 'custom'\ncustom_flag = true\n{headers}");
            let mut provider = AiProvider::preset(ProviderKind::Custom, "custom");
            apply_config_draft(&mut provider, &text).unwrap();
            assert_eq!(provider.config_toml, text);
            let rendered = config_text(&provider).parse::<DocumentMut>().unwrap();
            assert_eq!(rendered["custom_flag"].as_bool(), Some(true));
            assert_eq!(
                rendered["model_providers"]["custom"]["env_http_headers"]["Authorization"].as_str(),
                Some("ACCESS_TOKEN")
            );
        }
    }
    #[test]
    fn auth_draft_keeps_stored_keys_and_accepts_direct_input() {
        assert!(
            parse_auth_draft(r#"{"OPENAI_API_KEY":"••••1234"}"#, "••••1234").unwrap().is_none()
        );
        assert_eq!(
            parse_auth_draft(r#"{"ANTHROPIC_API_KEY":"test-key"}"#, " ").unwrap().unwrap().as_str(),
            "test-key"
        );
        assert!(parse_auth_draft("{broken", "").is_err());
        assert!(parse_auth_draft(r#"{"OPENAI_API_KEY":3}"#, "").is_err());
    }
}
