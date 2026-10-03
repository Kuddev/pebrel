//! Portable ZIP themes. Cold, synchronous operations run outside render callbacks.

mod archive;
pub(crate) mod cli;
mod envelope;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

pub(crate) use archive::{CheckedPackage, export_package};

pub(crate) const MAX_ARCHIVE_BYTES: u64 = 48 * 1024 * 1024;
pub(crate) const MAX_UNPACKED_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_VIDEO_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_ENTRIES: usize = 64;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MANIFEST: &str = "manifest.json";
const THEME: &str = "theme.pebrel-theme.json";

#[derive(Debug)]
pub(crate) struct PackageError(String);
impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PackageError {}
impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}
impl From<zip::result::ZipError> for PackageError {
    fn from(error: zip::result::ZipError) -> Self {
        Self(error.to_string())
    }
}
impl From<serde_json::Error> for PackageError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}
pub(crate) type Result<T> = std::result::Result<T, PackageError>;

fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition { Ok(()) } else { Err(PackageError(message.into())) }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResourceKind {
    Image,
    AnimatedImage,
    Video,
    Shader,
    Preview,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resource {
    pub path: String,
    pub kind: ResourceKind,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub package_version: u16,
    pub name: String,
    pub author: Author,
    pub version: String,
    pub license: String,
    pub theme: String,
    pub resources: Vec<Resource>,
}

impl Manifest {
    pub(crate) fn validate(&self) -> Result<()> {
        require(self.package_version == 1, "unsupported theme package version")?;
        require(self.theme == THEME, "theme entry must be theme.pebrel-theme.json")?;
        for (field, text, max) in [
            ("name", self.name.as_str(), 128),
            ("author", self.author.name.as_str(), 128),
            ("version", self.version.as_str(), 64),
            ("license", self.license.as_str(), 128),
        ] {
            require(
                !text.trim().is_empty() && text.len() <= max && !text.chars().any(char::is_control),
                format!("invalid {field}"),
            )?;
        }
        if let Some(handle) = &self.author.github {
            require(
                !handle.is_empty()
                    && handle.len() <= 39
                    && handle.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
                "invalid author GitHub handle",
            )?;
        }
        require(self.resources.len() + 2 <= MAX_ENTRIES, "too many theme resources")?;
        let mut paths = BTreeSet::new();
        let mut total = 0u64;
        let mut video = 0u64;
        for resource in &self.resources {
            validate_path(&resource.path)?;
            let extension = resource.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let media_matches = match resource.kind {
                ResourceKind::Image | ResourceKind::Preview => {
                    matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "webp")
                },
                ResourceKind::AnimatedImage => matches!(extension.as_str(), "gif" | "webp"),
                ResourceKind::Video => matches!(extension.as_str(), "mp4" | "webm"),
                ResourceKind::Shader => matches!(extension.as_str(), "glsl" | "wgsl"),
            };
            require(media_matches, "resource extension does not match its kind")?;
            require(resource.path.starts_with("assets/"), "resources must be under assets/")?;
            require(paths.insert(resource.path.to_ascii_lowercase()), "duplicate resource path")?;
            require(
                resource.bytes > 0 && resource.bytes <= MAX_UNPACKED_BYTES,
                "invalid resource size",
            )?;
            require(
                resource.sha256.len() == 64
                    && resource
                        .sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                "invalid resource SHA-256",
            )?;
            total = total
                .checked_add(resource.bytes)
                .ok_or_else(|| PackageError("resource size overflow".into()))?;
            require(total <= MAX_UNPACKED_BYTES, "unpacked resources exceed 64 MiB")?;
            if resource.kind == ResourceKind::Video {
                require(resource.bytes <= MAX_VIDEO_BYTES, "video exceeds 32 MiB")?;
                video += resource.bytes;
                require(video <= MAX_VIDEO_BYTES, "total video size exceeds 32 MiB")?;
            }
        }
        Ok(())
    }
}

fn validate_path(path: &str) -> Result<()> {
    require(
        !path.is_empty() && path.len() <= 256 && !path.starts_with('/') && !path.ends_with('/'),
        "invalid package path",
    )?;
    for part in path.split('/') {
        require(
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' ']),
            "invalid path component",
        )?;
        require(
            !part.chars().any(|c| c.is_control() || "\\<>:\"|?*".contains(c)),
            "path is not portable",
        )?;
        let stem = part.split('.').next().unwrap().to_ascii_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
        ) || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'));
        require(!device, "reserved device name in package path")?;
    }
    Ok(())
}
