//! 只读取显式声明的文件；不扫描插件目录树，也不根据 Lua 内容发现元数据。

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{Error, Result, json};

const MAX_MANIFEST_BYTES: usize = 32 * 1024;
pub(super) const MAX_SOURCE_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub manifest_version: u16,
    pub api_version: u16,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub commands: Vec<Command>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Command {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler: Option<String>,
    #[serde(default = "empty_params")]
    pub params: Value,
}

fn empty_params() -> Value {
    Value::Object(Map::new())
}

#[derive(Debug)]
pub(super) struct Package {
    pub manifest: Manifest,
    pub root: PathBuf,
}

impl Package {
    pub fn open(path: &Path) -> Result<Self> {
        let directory = if path.is_dir() {
            path
        } else if path.file_name().is_some_and(|name| name == "plugin.toml") {
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
        } else {
            return Err(Error::new("invalid_package", "expected a directory or plugin.toml"));
        };
        let root = directory.canonicalize().map_err(|error| Error::new("package_io", error))?;
        let source = read_file(&root, "plugin.toml", MAX_MANIFEST_BYTES)?;
        let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
        let manifest: Manifest =
            toml::from_str(source).map_err(|error| Error::new("invalid_manifest", error))?;
        manifest.validate()?;
        let package = Self { manifest, root };
        if let Some(entry) = &package.manifest.entry {
            // check 只核实文件与大小，不读取或执行入口；混合包的原生命令仍免建 VM。
            let path = package_file(&package.root, entry)?;
            let size = fs::metadata(&path).map_err(|error| Error::new("package_io", error))?.len();
            if size > MAX_SOURCE_BYTES as u64 {
                return Err(Error::new("source_too_large", "Lua entry exceeds 256 KiB"));
            }
        }
        Ok(package)
    }

    pub(super) fn source(&self) -> Result<String> {
        let entry = self
            .manifest
            .entry
            .as_deref()
            .ok_or_else(|| Error::new("missing_entry", "Lua commands require an entry"))?;
        read_file(&self.root, entry, MAX_SOURCE_BYTES)
    }

    pub(super) fn allow_method(&self, method: &str) -> Result<()> {
        if method == "events.subscribe" {
            return Err(Error::new(
                "stream_not_supported",
                "this entry supports one-shot requests",
            ));
        }
        if !self.manifest.permissions.iter().any(|allowed| allowed == method) {
            return Err(Error::new(
                "permission_denied",
                format!("undeclared runtime method: {method}"),
            ));
        }
        Ok(())
    }
}

impl Manifest {
    fn validate(&self) -> Result<()> {
        if self.manifest_version != 1 || self.api_version != 1 {
            return Err(Error::new("unsupported_version", "supported manifest/API version: 1"));
        }
        if !identifier(&self.id) || !label(&self.name, 256) || !label(&self.version, 64) {
            return Err(Error::new("invalid_metadata", "invalid id, name, or version"));
        }
        if self.commands.is_empty() || self.commands.len() > 64 || self.permissions.len() > 32 {
            return Err(Error::new(
                "manifest_limit",
                "declare 1..=64 commands and at most 32 permissions",
            ));
        }
        if self.permissions.iter().enumerate().any(|(index, method)| {
            !identifier(method)
                || method == "events.subscribe"
                || self.permissions[..index].contains(method)
        }) {
            return Err(Error::new(
                "invalid_permissions",
                "permissions must be unique one-shot method names",
            ));
        }
        let mut has_lua = false;
        for (index, command) in self.commands.iter().enumerate() {
            if !identifier(&command.id)
                || !label(&command.title, 256)
                || self.commands[..index].iter().any(|other| other.id == command.id)
            {
                return Err(Error::new("invalid_command", "command IDs must be valid and unique"));
            }
            match (&command.method, &command.handler) {
                (Some(method), None) if self.permissions.contains(method) => {},
                (None, Some(handler)) if identifier(handler) => has_lua = true,
                _ => {
                    return Err(Error::new(
                        "invalid_command",
                        "declare exactly one permitted method or Lua handler",
                    ));
                },
            }
            if !command.params.is_object() {
                return Err(Error::new("invalid_params", "command params must be an object"));
            }
            json::validate(&command.params)?;
        }
        if has_lua != self.entry.is_some() {
            return Err(Error::new(
                "invalid_entry",
                "entry must be present exactly when Lua handlers are declared",
            ));
        }
        if let Some(entry) = &self.entry {
            valid_relative_path(entry)?;
            if !entry.ends_with(".lua") {
                return Err(Error::new("invalid_entry", "entry must end in .lua"));
            }
        }
        Ok(())
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().next().is_some_and(|byte| byte.is_ascii_lowercase())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

fn label(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn valid_relative_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || value.contains(['\\', ':', '\0'])
        || value.split('/').any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(Error::new(
            "invalid_entry_path",
            "use a package-relative path with normal /-separated components",
        ));
    }
    Ok(())
}

fn package_file(root: &Path, relative: &str) -> Result<PathBuf> {
    valid_relative_path(relative)?;
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| Error::new("package_io", error))?;
        let linked = metadata.file_type().is_symlink();
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            linked || metadata.file_attributes() & 0x400 != 0
        };
        if linked {
            return Err(Error::new(
                "linked_package_file",
                "package entries must not traverse links or reparse points",
            ));
        }
    }
    let resolved = path.canonicalize().map_err(|error| Error::new("package_io", error))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(Error::new(
            "invalid_package_file",
            "expected a regular file inside the package",
        ));
    }
    Ok(resolved)
}

fn read_file(root: &Path, relative: &str, limit: usize) -> Result<String> {
    let path = package_file(root, relative)?;
    let file = File::open(path).map_err(|error| Error::new("package_io", error))?;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::new("package_io", error))?;
    if bytes.len() > limit {
        return Err(Error::new(
            "package_file_too_large",
            format!("{relative} exceeds {limit} bytes"),
        ));
    }
    String::from_utf8(bytes).map_err(|error| Error::new("invalid_utf8", error))
}
