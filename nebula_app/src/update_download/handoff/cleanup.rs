//! Retire successful Windows ZIP artifacts only after workspace acknowledgement.
//! Failed/unacknowledged transactions and the newest successful backup are retained.
use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use super::{Plan, canonical, guard_base, read_json};

pub(super) fn spawn() {
    #[cfg(windows)]
    if let Err(error) = std::thread::Builder::new().name("update-cleanup".into()).spawn(run) {
        log::warn!("Could not start update cleanup: {error}");
    }
}

pub(super) fn run() {
    #[cfg(windows)]
    {
        let result = std::env::current_exe().and_then(|executable| {
            run_at(
                &nebula_settings::settings_dir(),
                &canonical(&executable)?,
                env!("CARGO_PKG_VERSION"),
            )
        });
        if let Err(error) = result {
            log::warn!("Update cleanup deferred: {error}");
        }
    }
}

fn sequence(name: &str) -> Option<u128> {
    let (pid, stamp) = name.split_once('-')?;
    pid.parse::<u32>().ok()?;
    stamp.parse().ok()
}

fn linked(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn plain_directory(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir() && !linked(&metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn completed(directory: &Path, config: &Path, executable: &Path) -> Option<Plan> {
    if !plain_directory(directory).ok()? {
        return None;
    }
    let plan: Plan = read_json(&directory.join("plan.json"))?;
    if plan.schema != 1
        || sequence(&plan.transaction).is_none()
        || directory.file_name()?.to_str()? != plan.transaction
        || canonical(&plan.executable).ok()?.as_path() != executable
        || canonical(&plan.installation).ok()?.as_path() != executable.parent()?
        || canonical(&plan.config_directory).ok()?.as_path() != config
        || plan.portable.is_none()
        || super::super::validate_asset(&plan.asset).is_err()
    {
        return None;
    }
    let result: serde_json::Value = read_json(&directory.join("result.json"))?;
    if result["success"] != true
        || result["transaction"] != plan.transaction
        || result["version"] != plan.version
        || plan.asset.version != plan.version
        || read_json::<serde_json::Value>(&directory.join("restored.json")).is_none()
    {
        return None;
    }
    Some(plan)
}

fn run_at(config: &Path, executable: &Path, current_version: &str) -> io::Result<()> {
    let config = canonical(config)?;
    let updates = config.join("updates");
    let root = updates.join("handoffs");
    if !plain_directory(&updates)? || !plain_directory(&root)? {
        return Ok(());
    }
    let executable = canonical(executable)?;
    let Some(_installation_guard) =
        crate::atomic_file::try_lifetime_lock(&guard_base(&executable))?
    else {
        return Ok(());
    };
    let mut directories: Vec<_> = std::fs::read_dir(&root)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            Some((sequence(name.to_str()?)?, entry.path()))
        })
        .collect();
    directories.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let protected: BTreeSet<_> = directories
        .iter()
        .filter_map(|(_, path)| {
            if completed(path, &config, &executable).is_some() {
                return None;
            }
            read_json::<Plan>(&path.join("plan.json")).map(|plan| plan.asset.name)
        })
        .chain(
            read_json::<crate::update_check::UpdateAsset>(&updates.join("install-next.json"))
                .map(|asset| asset.name),
        )
        .collect();
    let records: Vec<_> = directories
        .into_iter()
        .take(256)
        .filter_map(|(stamp, path)| {
            completed(&path, &config, &executable).map(|plan| (stamp, path, plan))
        })
        .collect();
    let Some((latest, _, _)) = records.iter().find(|(_, _, plan)| plan.version == current_version)
    else {
        return Ok(());
    };
    for (stamp, directory, plan) in &records {
        if stamp > latest {
            continue;
        }
        let payload = plan.portable.as_ref().unwrap();
        let allowed: BTreeSet<_> = payload.paths().map(|path| path.to_lowercase()).collect();
        let stage = directory.join("portable");
        if stage.exists() && canonical(payload.directory())? != canonical(&stage)? {
            return Err(io::Error::other("Update staging path changed"));
        }
        retire_tree(&stage, &allowed)?;
        if stamp < latest {
            let backup =
                executable.parent().unwrap().join(format!(".pebrel-update-{}", plan.transaction));
            retire_tree(&backup, &allowed)?;
        }
        // The URL/name contract and canonical updates root establish ownership of
        // this exact ZIP. A concurrent downloader's lifetime lock defers removal.
        let package = updates.join(&plan.asset.name);
        if plan.installer != package
            || !package.exists()
            || protected.contains(&plan.asset.name)
            || super::super::cache::supersedes_at(
                &updates.join("download.json"),
                &directory.join("result.json"),
            )
        {
            continue;
        }
        let Some(_download_guard) = crate::atomic_file::try_lifetime_lock(&package)? else {
            continue;
        };
        let metadata = std::fs::symlink_metadata(&package)?;
        if metadata.is_file() && !linked(&metadata) {
            std::fs::remove_file(package)?;
        }
    }
    Ok(())
}

/// Validate the whole reserved subtree before deleting anything; unexpected
/// files or reparse points preserve it. Never remove the installation/config root.
fn retire_tree(root: &Path, allowed: &BTreeSet<String>) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    if !plain_directory(root)? {
        return Err(io::Error::other("Linked update artifact directory"));
    }
    let root = canonical(root)?;
    let mut pending = vec![root.clone()];
    let mut directories = Vec::new();
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        if directories.len() > 1024 {
            return Err(io::Error::other("Update artifact tree exceeds limit"));
        }
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(&root)
                .map_err(io::Error::other)?
                .to_string_lossy()
                .replace('\\', "/")
                .to_lowercase();
            let metadata = std::fs::symlink_metadata(&path)?;
            if linked(&metadata) {
                return Err(io::Error::other("Linked update artifact"));
            }
            if metadata.is_dir()
                && allowed.iter().any(|file| file.starts_with(&(relative.clone() + "/")))
            {
                pending.push(path);
            } else if metadata.is_file() && allowed.contains(&relative) {
                files.push(path);
            } else {
                return Err(io::Error::other("Unexpected file in update artifacts; preserved"));
            }
        }
        directories.push(directory);
    }
    for file in files {
        std::fs::remove_file(file)?;
    }
    for directory in directories.into_iter().rev() {
        std::fs::remove_dir(directory)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "cleanup/tests.rs"]
mod tests;
