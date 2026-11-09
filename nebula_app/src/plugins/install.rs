//! 插件安装库：只复制清单声明的文件，回执由宿主写入；授权只在安装时由用户给出。

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::package::{
    Grant, MAX_MANIFEST_BYTES, MAX_SOURCE_BYTES, Output, Package, identifier, package_file,
};
use super::{Error, Result};

const RECEIPT: &str = "pebrel-install.json";
const RECEIPT_VERSION: u16 = 1;
/// 复制上限；主题包自身的大小、条目和摘要仍由主题包校验负责。
const MAX_THEME_PACKAGE_BYTES: u64 = 48 * 1024 * 1024;

/// Host-written install state. Package files never supply it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    receipt_version: u16,
    version: String,
    enabled: bool,
    grants: Vec<Grant>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CommandSummary {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub grants: Vec<Grant>,
    pub bundled_themes: usize,
    /// Commands whose result the host turns into a library theme.
    pub theme_commands: Vec<CommandSummary>,
}

/// A directory that is not a valid installed plugin; it can still be uninstalled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Broken {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Listing {
    pub plugins: Vec<Installed>,
    pub broken: Vec<Broken>,
}

/// What a package asks for before it is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub name: String,
    pub requested: Vec<Grant>,
}

pub(crate) struct Library {
    root: PathBuf,
}

impl Default for Library {
    fn default() -> Self {
        Self::new(nebula_settings::settings_dir().join("plugins"))
    }
}

impl Library {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Read a package's name and requested grants without installing it, so
    /// the user can decide on the grants first.
    pub(crate) fn inspect(source: &Path) -> Result<Request> {
        let package = Package::open(source)?;
        Ok(Request {
            name: package.manifest.name.clone(),
            requested: package.manifest.requested_grants(),
        })
    }

    /// Copy a checked package into the library, replacing an older install of
    /// the same ID. Fails before touching the library when `granted` does not
    /// cover what the manifest requests.
    pub(crate) fn install(&self, source: &Path, granted: &[Grant]) -> Result<Installed> {
        let package = Package::open(source)?;
        let requested = package.manifest.requested_grants();
        let missing: Vec<_> = requested.iter().filter(|grant| !granted.contains(grant)).collect();
        if !missing.is_empty() {
            let mut error = Error::new("grant_required", "the plugin needs grants to install");
            error.details = Some(json!({ "requested": requested, "missing": missing }));
            return Err(error);
        }
        let id = package.manifest.id.clone();
        if reserved_device_name(&id) {
            return Err(Error::new("invalid_metadata", "plugin ID is a reserved device name"));
        }
        let root = self.root()?;
        let _lock = lock(&root)?;
        let stage =
            tempfile::Builder::new().prefix(".install-").tempdir_in(&root).map_err(io_error)?;
        let manifest = &package.manifest;
        copy(&package, "plugin.toml", stage.path(), MAX_MANIFEST_BYTES as u64)?;
        if let Some(entry) = &manifest.entry {
            copy(&package, entry, stage.path(), MAX_SOURCE_BYTES as u64)?;
        }
        for theme in &manifest.themes {
            copy(&package, theme, stage.path(), MAX_THEME_PACKAGE_BYTES)?;
        }
        super::themes::check_bundled(stage.path(), &manifest.themes)?;
        let receipt = Receipt {
            receipt_version: RECEIPT_VERSION,
            version: manifest.version.clone(),
            enabled: true,
            grants: requested,
        };
        write_receipt(stage.path(), &receipt)?;
        replace_directory(stage.keep(), &root.join(&id))?;
        self.load(&id)
    }

    pub(crate) fn list(&self) -> Result<Listing> {
        let mut listing = Listing::default();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(listing),
            Err(error) => return Err(io_error(error)),
        };
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
            if name.starts_with('.') || !entry.file_type().map_err(io_error)?.is_dir() {
                continue;
            }
            match self.load(&name) {
                Ok(plugin) => listing.plugins.push(plugin),
                Err(error) => listing.broken.push(Broken { id: name, error: error.to_string() }),
            }
        }
        listing.plugins.sort_by(|left, right| left.id.cmp(&right.id));
        listing.broken.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(listing)
    }

    pub(crate) fn load(&self, id: &str) -> Result<Installed> {
        let (package, receipt) = self.open(id)?;
        let manifest = &package.manifest;
        Ok(Installed {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            enabled: receipt.enabled,
            grants: receipt.grants,
            bundled_themes: manifest.themes.len(),
            theme_commands: manifest
                .commands
                .iter()
                .filter(|command| command.output == Output::Theme)
                .map(|command| CommandSummary {
                    id: command.id.clone(),
                    title: command.title.clone(),
                })
                .collect(),
        })
    }

    /// The installed package plus its grants; disabled plugins do not run.
    pub(super) fn open_enabled(&self, id: &str) -> Result<(Package, Vec<Grant>)> {
        let (package, receipt) = self.open(id)?;
        if !receipt.enabled {
            return Err(Error::new("plugin_disabled", format!("plugin is disabled: {id}")));
        }
        Ok((package, receipt.grants))
    }

    pub(crate) fn set_enabled(&self, id: &str, enabled: bool) -> Result<Installed> {
        let root = self.root()?;
        let _lock = lock(&root)?;
        let (package, mut receipt) = self.open(id)?;
        receipt.enabled = enabled;
        write_receipt(&package.root, &receipt)?;
        self.load(id)
    }

    /// Remove the plugin's files. Themes it added stay in the theme library.
    pub(crate) fn uninstall(&self, id: &str) -> Result<()> {
        let root = self.root()?;
        let _lock = lock(&root)?;
        let path = installed_path(&root, id)?;
        if !path.is_dir() {
            return Err(Error::new("plugin_not_installed", id));
        }
        let trash =
            tempfile::Builder::new().prefix(".remove-").tempdir_in(&root).map_err(io_error)?;
        fs::rename(&path, trash.path().join("plugin")).map_err(io_error)?;
        // TempDir 删除失败只留下以点开头的残留目录，列表会跳过它。
        drop(trash);
        Ok(())
    }

    fn open(&self, id: &str) -> Result<(Package, Receipt)> {
        let path = installed_path(&self.root, id)?;
        if !path.is_dir() {
            return Err(Error::new("plugin_not_installed", id));
        }
        let package = Package::open(&path)?;
        if package.manifest.id != id {
            return Err(Error::new("invalid_install", "directory and manifest IDs differ"));
        }
        let receipt = read_receipt(&package.root)?;
        if receipt.version != package.manifest.version {
            return Err(Error::new("invalid_install", "receipt and manifest versions differ"));
        }
        Ok((package, receipt))
    }

    fn root(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.root).map_err(io_error)?;
        if fs::symlink_metadata(&self.root).map_err(io_error)?.file_type().is_symlink() {
            return Err(Error::new("invalid_install", "plugin directory must not be a link"));
        }
        self.root.canonicalize().map_err(io_error)
    }
}

fn installed_path(root: &Path, id: &str) -> Result<PathBuf> {
    if !identifier(id) {
        return Err(Error::new("invalid_plugin_id", id));
    }
    Ok(root.join(id))
}

/// `con`、`com1.x` 之类在 Windows 上是设备名，不能作为插件目录。
fn reserved_device_name(id: &str) -> bool {
    let stem = id.split('.').next().unwrap_or(id);
    matches!(stem, "con" | "prn" | "aux" | "nul")
        || stem
            .strip_prefix("com")
            .or_else(|| stem.strip_prefix("lpt"))
            .is_some_and(|digit| digit.len() == 1 && (b'1'..=b'9').contains(&digit.as_bytes()[0]))
}

fn lock(root: &Path) -> Result<crate::atomic_file::FileLock> {
    crate::atomic_file::try_lock(&root.join("library"))
        .map_err(io_error)?
        .ok_or_else(|| Error::new("plugin_library_busy", "another plugin operation is running"))
}

fn copy(package: &Package, relative: &str, stage: &Path, limit: u64) -> Result<()> {
    let source = package_file(&package.root, relative)?;
    let target = stage.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let mut output =
        File::options().write(true).create_new(true).open(&target).map_err(io_error)?;
    let copied =
        std::io::copy(&mut File::open(source).map_err(io_error)?.take(limit + 1), &mut output)
            .map_err(io_error)?;
    if copied > limit {
        return Err(Error::new(
            "package_file_too_large",
            format!("{relative} exceeds {limit} bytes"),
        ));
    }
    output.sync_all().map_err(io_error)
}

fn read_receipt(directory: &Path) -> Result<Receipt> {
    let mut bytes = Vec::new();
    File::open(directory.join(RECEIPT))
        .map_err(io_error)?
        .take(4096)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|error| Error::new("invalid_install", format!("install receipt: {error}")))?;
    if receipt.receipt_version != RECEIPT_VERSION {
        return Err(Error::new("invalid_install", "unsupported install receipt version"));
    }
    Ok(receipt)
}

fn write_receipt(directory: &Path, receipt: &Receipt) -> Result<()> {
    let mut bytes =
        serde_json::to_vec_pretty(receipt).map_err(|error| Error::new("invalid_install", error))?;
    bytes.write_all(b"\n").map_err(io_error)?;
    crate::atomic_file::write(&directory.join(RECEIPT), &bytes).map_err(io_error)
}

/// 先把旧版本移开再换入新目录；换入失败时放回旧版本。
fn replace_directory(stage: PathBuf, target: &Path) -> Result<()> {
    let root = target.parent().expect("installed plugin has a parent");
    let previous = if target.exists() {
        let trash =
            tempfile::Builder::new().prefix(".replace-").tempdir_in(root).map_err(io_error)?;
        let previous = trash.path().join("plugin");
        fs::rename(target, &previous).map_err(io_error)?;
        Some((trash, previous))
    } else {
        None
    };
    if let Err(error) = fs::rename(&stage, target) {
        if let Some((_, previous)) = &previous {
            let _ = fs::rename(previous, target);
        }
        let _ = fs::remove_dir_all(&stage);
        return Err(io_error(error));
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> Error {
    Error::new("plugin_io", error)
}
