//! Bounded Windows ZIP inspection and staging. The native helper owns replacement.
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read as _, Seek, Write as _};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const MAX_FILES: usize = 256;
const MAX_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub(super) struct Payload {
    directory: std::path::PathBuf,
    files: Vec<PayloadFile>,
}

#[derive(Serialize, Deserialize)]
struct PayloadFile {
    path: String,
    bytes: u64,
    sha256: String,
}

impl Payload {
    pub(super) fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|file| file.path.as_str())
    }
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }
}

/// Additional bytes needed before extraction: staging, then backups and copies.
pub(super) fn reservations(package: &Path, installation: &Path) -> Result<(u64, u64), String> {
    let mut archive = archive(File::open(package).map_err(|error| error.to_string())?)?;
    let mut expanded = 0_u64;
    let mut backup = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if entry.is_dir() {
            continue;
        }
        expanded += entry.size();
        let target = installation.join(entry.name());
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || !crate::platform::update_installation::canonical(&target)
                        .map_err(|error| error.to_string())?
                        .starts_with(installation)
                {
                    return Err(format!("Unsafe existing package file: {}", target.display()));
                }
                backup = backup.checked_add(metadata.len()).ok_or("Backup size overflow")?;
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok((expanded, backup.checked_add(expanded).ok_or("Replacement size overflow")?))
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or_default().to_ascii_uppercase();
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part.chars().any(|ch| ch.is_control() || "\\:<>\"|?*".contains(ch))
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
        && !name.split('/').next().unwrap().to_ascii_lowercase().starts_with(".pebrel-update")
        && !matches!(name.to_ascii_lowercase().as_str(), "unins000.exe" | "pebrel-distribution")
}

fn archive<R: std::io::Read + Seek>(reader: R) -> Result<zip::ZipArchive<R>, String> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|error| error.to_string())?;
    if archive.is_empty() || archive.len() > MAX_FILES {
        return Err("Portable package file count exceeds limit".into());
    }
    let mut names = BTreeSet::new();
    let mut files = BTreeSet::new();
    let mut expanded = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().trim_end_matches('/');
        if !safe_name(name)
            || !names.insert(name.to_ascii_lowercase())
            || entry.unix_mode().is_some_and(|mode| {
                let kind = mode & 0o170000;
                kind != 0 && kind != 0o100000 && kind != 0o040000
            })
        {
            return Err(format!("Unsafe portable package path: {}", entry.name()));
        }
        expanded = expanded.checked_add(entry.size()).ok_or("Portable package size overflow")?;
        if expanded > MAX_EXPANDED_BYTES || entry.size() > super::MAX_INSTALLER_BYTES {
            return Err("Portable package expanded size exceeds limit".into());
        }
        if !entry.is_dir() {
            files.insert(name.to_ascii_lowercase());
        }
    }
    for name in &names {
        let mut parent = name.as_str();
        while let Some((prefix, _)) = parent.rsplit_once('/') {
            if files.contains(prefix) {
                return Err("Portable package file/directory collision".into());
            }
            parent = prefix;
        }
    }
    if !files.contains("pebrel.exe") {
        return Err("Portable package is missing pebrel.exe".into());
    }
    Ok(archive)
}

pub(super) fn inspect(file: &mut File) -> Result<(), String> {
    archive(file).map(|_| ())
}

/// Keep the verified archive immutable while deriving its staged-file manifest.
pub(super) fn lock_package(package: &Path) -> Result<File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1); // FILE_SHARE_READ, no write or delete sharing.
    }
    options.open(package).map_err(|error| error.to_string())
}

pub(super) fn extract(package: &Path, transaction: &Path) -> Result<Payload, String> {
    let mut archive = archive(File::open(package).map_err(|error| error.to_string())?)?;
    let directory = transaction.join("portable");
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let path = entry.name().to_owned();
        let destination = directory.join(&path);
        std::fs::create_dir_all(destination.parent().unwrap())
            .map_err(|error| error.to_string())?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| error.to_string())?;
        let mut hash = Sha256::new();
        let mut bytes = 0_u64;
        let mut header = Vec::new();
        let mut buffer = [0; super::DOWNLOAD_CHUNK_BYTES];
        loop {
            let read = entry.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            bytes += read as u64;
            if bytes > entry.size() {
                return Err("Portable ZIP entry exceeds declared size".into());
            }
            if header.len() < 2 {
                header.extend_from_slice(&buffer[..read.min(2 - header.len())]);
            }
            hash.update(&buffer[..read]);
            output.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        }
        if bytes != entry.size() {
            return Err("Truncated portable ZIP entry".into());
        }
        if path.eq_ignore_ascii_case("pebrel.exe") && header != b"MZ" {
            return Err("Portable executable is not Windows PE".into());
        }
        output.sync_all().map_err(|error| error.to_string())?;
        files.push(PayloadFile {
            path,
            bytes,
            sha256: hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect(),
        });
    }
    Ok(Payload {
        directory: crate::platform::update_installation::canonical(&directory)
            .map_err(|error| error.to_string())?,
        files,
    })
}

#[cfg(test)]
#[path = "portable/tests.rs"]
mod tests;
