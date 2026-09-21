//! POSIX hook installation policy. This module plans owned file edits; it never opens
//! a connection, changes a terminal, or interprets screen output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{CLAUDE_EVENTS, bridges, installation};

pub(crate) const FILE_ADAPTER: &str = include_str!("../../res/hooks/remote_files.py");
const BRIDGE: &str = include_str!("../../res/hooks/remote_bridge.py");
const SHELL: &str = include_str!("../../res/hooks/remote_shell.py");
pub(crate) const TOKEN_ENV: &str = "PEBREL_REMOTE_HOOK_TOKEN";

pub(crate) fn new_token() -> std::io::Result<String> {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut token = String::with_capacity(32);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

#[derive(Debug, Deserialize)]
pub(crate) struct Snapshot {
    version: u8,
    pub root: String,
    pub python: String,
    files: BTreeMap<String, File>,
    providers: BTreeMap<String, bool>,
    codex_version: String,
    codex_features: String,
}

#[derive(Debug, Deserialize)]
struct File {
    path: String,
    sha256: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Edit {
    name: String,
    expected: Option<String>,
    content: Option<String>,
}

/// The marker is an ownership receipt, never a trust grant. Exact provider
/// groups and asset hashes let removal preserve everything a user has edited.
#[derive(Default, Deserialize, Serialize)]
struct Manifest {
    version: u8,
    #[serde(default)]
    groups: BTreeMap<String, Value>,
    #[serde(default)]
    assets: BTreeMap<String, String>,
    notify: Option<Notify>,
    #[serde(default)]
    enabled_feature: bool,
}

#[derive(Deserialize, Serialize)]
struct Notify {
    original: Option<Vec<String>>,
    installed: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Automatic,
    Install,
    Remove,
}

pub(crate) fn digest(value: &str) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value.as_bytes()) {
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

/// POSIX command arguments, including paths with spaces, quotes and dollar signs.
pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn request(action: &Value) -> Vec<u8> {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(action.to_string());
    format!("{FILE_ADAPTER}\nrun(json.loads(base64.b64decode('{encoded}')))\n").into_bytes()
}

pub(crate) fn response(raw: &str) -> Result<Value, String> {
    let line = raw
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("PEBREL_INTEGRATION="))
        .ok_or("remote integration response is missing")?;
    let result: Value = serde_json::from_str(line).map_err(|_| "invalid integration response")?;
    if result.get("version").and_then(Value::as_u64) != Some(1) || result.get("error").is_some() {
        return Err("remote integration could not read or update its owned files".into());
    }
    Ok(result)
}

impl Snapshot {
    /// WSL keeps its normal login shell. Only command hooks have a complete
    /// transport there; plugin adapters currently need the SSH bootstrap env.
    pub(crate) fn for_wsl(mut self) -> Self {
        self.providers.retain(|name, _| matches!(name.as_str(), "claude" | "codex"));
        self
    }

    fn file(&self, name: &str) -> Result<&File, String> {
        self.files.get(name).ok_or_else(|| format!("missing integration file: {name}"))
    }

    fn raw(&self, name: &str) -> Result<Option<&str>, String> {
        Ok(self.file(name)?.content.as_deref())
    }

    fn edit(
        &self,
        edits: &mut Vec<Edit>,
        name: &str,
        content: Option<String>,
    ) -> Result<(), String> {
        let current = self.file(name)?;
        if current.content != content {
            edits.push(Edit { name: name.into(), expected: current.sha256.clone(), content });
        }
        Ok(())
    }

    fn present(&self, provider: &str) -> bool {
        self.providers.get(provider) == Some(&true)
    }

    pub(crate) fn bootstrap(&self, token: &str) -> Result<String, String> {
        if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid remote hook token".into());
        }
        Ok(format!(
            "exec {} {} {}",
            quote(&self.python),
            quote(&self.file("shell.py")?.path),
            quote(token)
        ))
    }

    pub(crate) fn plan(&self, action: Action) -> Result<Option<Vec<Edit>>, String> {
        if self.version != 1 || !self.root.starts_with('/') || !self.python.starts_with('/') {
            return Err("unsupported remote integration environment".into());
        }
        if action == Action::Automatic && self.raw("disabled")?.is_some() {
            return Ok(None);
        }
        let mut manifest: Manifest = self
            .raw("manifest")?
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| "invalid integration ownership receipt")?
            .unwrap_or_default();
        if manifest.version > 1 {
            return Err("unsupported integration ownership receipt".into());
        }
        let mut edits = Vec::new();
        if action == Action::Remove {
            self.remove(&mut manifest, &mut edits)?;
        } else {
            self.install(&mut manifest, &mut edits)?;
            self.edit(&mut edits, "disabled", None)?;
            manifest.version = 1;
            self.edit(
                &mut edits,
                "manifest",
                Some(serde_json::to_string_pretty(&manifest).unwrap() + "\n"),
            )?;
        }
        Ok(Some(edits))
    }

    fn merge_provider(
        &self,
        manifest: &mut Manifest,
        edits: &mut Vec<Edit>,
        provider: &str,
        groups: Value,
    ) -> Result<(), String> {
        let previous = manifest.groups.get(provider).map(Value::to_string);
        let (content, _) =
            installation::merge_groups(self.raw(provider)?, previous.as_deref(), &groups)?;
        self.edit(edits, provider, Some(content))?;
        manifest.groups.insert(provider.into(), groups);
        Ok(())
    }

    fn install(&self, manifest: &mut Manifest, edits: &mut Vec<Edit>) -> Result<(), String> {
        let helper = &self.file("pebrel-hook")?.path;
        if self.present("claude") {
            let command = format!("{} claude", quote(helper));
            let groups = CLAUDE_EVENTS.into_iter().map(|event| {
                (event.to_owned(), json!([{"matcher":"", "hooks":[{"type":"command", "command":command, "timeout":3}]}]))
            }).collect::<serde_json::Map<_, _>>();
            self.merge_provider(manifest, edits, "claude", Value::Object(groups))?;
        }
        if self.present("codex") {
            self.install_codex(manifest, edits, helper)?;
        }
        let launcher = format!(
            "#!/bin/sh\nexec {} {} \"$@\"\n",
            quote(&self.python),
            quote(&self.file("bridge.py")?.path)
        );
        let mut assets = vec![
            ("pebrel-hook", launcher.as_str()),
            ("bridge.py", BRIDGE),
            ("shell.py", SHELL),
            ("bashrc", include_str!("../../res/shell/bashrc")),
            (".zshenv", include_str!("../../res/shell/zshenv")),
            (".zprofile", include_str!("../../res/shell/zprofile")),
            (".zshrc", include_str!("../../res/shell/zshrc")),
        ];
        if self.present("opencode") {
            assets.push(("opencode", bridges::OPENCODE_PLUGIN_JS));
        }
        if self.present("pi") {
            assets.push(("pi", bridges::PI_EXTENSION_TS));
        }
        for (name, content) in assets {
            let file = self.file(name)?;
            if file.content.as_deref().is_some_and(|raw| raw != content)
                && file.sha256.as_ref() != manifest.assets.get(name)
            {
                return Err(format!("edited remote {name} preserved"));
            }
            self.edit(edits, name, Some(content.into()))?;
            manifest.assets.insert(name.into(), digest(content));
        }
        Ok(())
    }

    fn install_codex(
        &self,
        manifest: &mut Manifest,
        edits: &mut Vec<Edit>,
        helper: &str,
    ) -> Result<(), String> {
        let raw = self.raw("codex_config")?.unwrap_or("");
        let mut config = raw.parse::<toml_edit::DocumentMut>().map_err(|_| "invalid Codex TOML")?;
        let current = notify(&config)?;
        if let Some(owned) = &manifest.notify
            && current.as_ref() != Some(&owned.installed)
        {
            return Err("edited remote Codex notify preserved".into());
        }
        if let Some(desired) =
            installation::desired_codex_notify(current.as_deref().unwrap_or_default(), helper)
        {
            let original = manifest.notify.take().map(|owned| owned.original).unwrap_or(current);
            set_notify(&mut config, Some(&desired));
            manifest.notify = Some(Notify { original, installed: desired });
        }
        let mut raw = config.to_string();
        if let Some(mode) = installation::codex_mode(&self.codex_version, &self.codex_features)
            && let Some((enabled, introduced)) = installation::enable_codex_feature(&raw)?
        {
            let command = format!("{} codex {}", quote(helper), mode.argument());
            self.merge_provider(
                manifest,
                edits,
                "codex",
                installation::codex_groups(&command, None, mode),
            )?;
            manifest.enabled_feature |= introduced;
            raw = enabled;
        }
        self.edit(edits, "codex_config", Some(raw))
    }

    fn remove(&self, manifest: &mut Manifest, edits: &mut Vec<Edit>) -> Result<(), String> {
        let mut codex_hooks_remain = false;
        for (provider, groups) in &manifest.groups {
            let (content, _) = installation::merge_groups(
                self.raw(provider)?,
                Some(&groups.to_string()),
                &json!({}),
            )?;
            if provider == "codex" {
                let root: Value = serde_json::from_str(&content).unwrap();
                codex_hooks_remain =
                    root["hooks"].as_object().is_some_and(|hooks| !hooks.is_empty());
            }
            self.edit(edits, provider, Some(content))?;
        }
        if (manifest.notify.is_some() || manifest.enabled_feature)
            && let Some(raw) = self.raw("codex_config")?
        {
            let mut config =
                raw.parse::<toml_edit::DocumentMut>().map_err(|_| "invalid Codex TOML")?;
            if let Some(owned) = &manifest.notify {
                if notify(&config)?.as_ref() != Some(&owned.installed) {
                    return Err("edited remote Codex notify preserved".into());
                }
                set_notify(&mut config, owned.original.as_deref());
            }
            if manifest.enabled_feature
                && !codex_hooks_remain
                && config.get("hooks").is_none()
                && let Some(features) =
                    config.get_mut("features").and_then(toml_edit::Item::as_table_like_mut)
                && features.get("hooks").and_then(toml_edit::Item::as_bool) == Some(true)
            {
                features.remove("hooks");
            }
            self.edit(edits, "codex_config", Some(config.to_string()))?;
        }
        for (name, hash) in &manifest.assets {
            let file = self.file(name)?;
            if file.sha256.as_ref().is_some_and(|current| current != hash) {
                return Err(format!("edited remote {name} preserved"));
            }
            self.edit(edits, name, None)?;
        }
        self.edit(edits, "manifest", None)?;
        self.edit(
            edits,
            "disabled",
            Some("Automatic integration disabled by setup-ai --ssh --remove.\n".into()),
        )
    }
}

fn notify(config: &toml_edit::DocumentMut) -> Result<Option<Vec<String>>, String> {
    let Some(item) = config.get("notify") else { return Ok(None) };
    item.as_array()
        .ok_or("Codex notify must be an array")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "Codex notify arguments must be strings".into())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn set_notify(config: &mut toml_edit::DocumentMut, value: Option<&[String]>) {
    match value {
        Some(value) => {
            config["notify"] = toml_edit::value(value.iter().cloned().collect::<toml_edit::Array>())
        },
        None => {
            config.remove("notify");
        },
    }
}

#[cfg(test)]
mod tests;
