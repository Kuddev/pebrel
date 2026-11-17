//! On-demand community theme catalog. Nothing here runs at startup: the
//! catalog is fetched only when the user opens the community page or runs a
//! catalog command, and every install goes through the existing package checks.

#[cfg(test)]
mod tests;
mod transfer;

use super::ThemeDocument;
use super::package::{MAX_ARCHIVE_BYTES, MAX_UNPACKED_BYTES, MAX_VIDEO_BYTES, Origin};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[cfg(test)]
pub(crate) use transfer::install_file;
pub(crate) use transfer::{Error, fetch_catalog, fetch_preview, install};

/// Provenance name written into install receipts.
pub(crate) const CATALOG: &str = "pebrel-community/themes";
pub(crate) const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/pebrel-community/themes/main/catalog/index.json";
/// Same as the community repository's `policy.json`.
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;
const MAX_CATALOG_ENTRIES: usize = 1000;
pub(crate) const MAX_PREVIEW_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Author {
    pub name: String,
    pub github: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Archive {
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

/// One catalog row. Fields the client does not use are ignored, so additive
/// catalog changes do not hide the whole page; `schema_version` gates breaking ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: Author,
    pub source: String,
    pub license: String,
    pub archive: Archive,
    pub unpacked_bytes: u64,
    pub video_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
struct Document {
    schema_version: u16,
    themes: Vec<serde_json::Value>,
}

/// Validated rows. A malformed row is skipped instead of hiding the others;
/// `skipped` keeps that visible.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Listing {
    pub entries: Vec<Entry>,
    pub skipped: usize,
}

impl Listing {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, String> {
        let document: Document =
            serde_json::from_slice(bytes).map_err(|error| format!("invalid catalog: {error}"))?;
        if document.schema_version != 1 {
            return Err(format!("unsupported catalog schema {}", document.schema_version));
        }
        if document.themes.len() > MAX_CATALOG_ENTRIES {
            return Err("catalog lists too many themes".into());
        }
        let mut listing = Self::default();
        let mut ids = BTreeSet::new();
        for value in document.themes {
            match serde_json::from_value::<Entry>(value) {
                Ok(mut entry) if entry.validate().is_ok() && ids.insert(entry.id.clone()) => {
                    // Previews outside the theme's own repository are not
                    // fetched: the page must not contact arbitrary hosts.
                    if entry.preview.as_deref().is_some_and(|url| !entry.owns_preview(url)) {
                        entry.preview = None;
                    }
                    listing.entries.push(entry);
                },
                _ => listing.skipped += 1,
            }
        }
        Ok(listing)
    }
}

impl Entry {
    fn validate(&self) -> Result<(), &'static str> {
        let (author, name) = self.id.split_once('/').ok_or("id")?;
        require(
            self.id.len() <= 128 && slug(author) && slug(name),
            "id: use lowercase author/theme-name",
        )?;
        require(text(&self.name, 128), "name")?;
        require(text(&self.version, 64) && version(&self.version), "version")?;
        require(text(&self.author.name, 128), "author.name")?;
        require(github_handle(&self.author.github), "author.github")?;
        let (owner, repo) = github_repository(&self.source).ok_or("source")?;
        require(text(&self.license, 128), "license")?;
        let release = self
            .archive
            .url
            .strip_prefix("https://github.com/")
            .and_then(|path| path.strip_prefix(owner))
            .and_then(|path| path.strip_prefix('/'))
            .and_then(|path| path.strip_prefix(repo))
            .and_then(|path| path.strip_prefix("/releases/download/"))
            .ok_or("archive.url")?;
        let (tag, file) = release.split_once('/').ok_or("archive.url")?;
        require(
            url_component(tag)
                && !tag.eq_ignore_ascii_case("latest")
                && url_component(file)
                && file.len() > ".pebrel-theme.zip".len()
                && file.ends_with(".pebrel-theme.zip"),
            "archive.url: versioned .pebrel-theme.zip release required",
        )?;
        require(
            self.archive.sha256.len() == 64
                && self.archive.sha256.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')),
            "archive.sha256",
        )?;
        require((1..=MAX_ARCHIVE_BYTES).contains(&self.archive.bytes), "archive.bytes")?;
        require((1..=MAX_UNPACKED_BYTES).contains(&self.unpacked_bytes), "unpacked_bytes")?;
        require(
            self.video_bytes <= MAX_VIDEO_BYTES && self.video_bytes <= self.unpacked_bytes,
            "video_bytes",
        )?;
        if let Some(description) = &self.description {
            require(text(description, 1024), "description")?;
        }
        if let Some(preview) = &self.preview {
            require(preview.starts_with("https://") && text(preview, 2048), "preview")?;
        }
        require(self.tags.len() <= 12, "tags")?;
        let mut tags = BTreeSet::new();
        for tag in &self.tags {
            require(text(tag, 32) && tags.insert(tag.as_str()), "tags")?;
        }
        Ok(())
    }

    fn owns_preview(&self, url: &str) -> bool {
        let Some((owner, repo)) = github_repository(&self.source) else { return false };
        let Some(path) = url.strip_prefix("https://raw.githubusercontent.com/") else {
            return false;
        };
        let mut parts = path.split('/');
        parts.next().is_some_and(|part| part.eq_ignore_ascii_case(owner))
            && parts.next().is_some_and(|part| part.eq_ignore_ascii_case(repo))
            && {
                let rest: Vec<_> = parts.collect();
                rest.len() >= 2 && rest.iter().all(|part| url_component(part))
            }
    }

    /// Receipt identity: one catalog row at one version and archive digest.
    pub(crate) fn origin(&self) -> Origin {
        Origin {
            catalog: CATALOG.into(),
            id: self.id.clone(),
            version: self.version.clone(),
            sha256: self.archive.sha256.clone(),
        }
    }
}

/// How a catalog row relates to the user's library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Installed {
    No,
    /// The library holds this exact catalog version; carries its theme ID.
    Current(String),
    /// Only another version of the row is installed.
    Other,
}

pub(crate) fn installed(entry: &Entry, library: &[ThemeDocument]) -> Installed {
    let mut state = Installed::No;
    for document in library {
        let Some(origin) = Origin::of(document).filter(|origin| origin.catalog == CATALOG) else {
            continue;
        };
        if origin.id != entry.id {
            continue;
        }
        if origin.sha256 == entry.archive.sha256 {
            if let Some(id) = document.id() {
                return Installed::Current(id.to_owned());
            }
        }
        state = Installed::Other;
    }
    state
}

fn require(condition: bool, field: &'static str) -> Result<(), &'static str> {
    if condition { Ok(()) } else { Err(field) }
}

fn text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn slug(value: &str) -> bool {
    value.bytes().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && value.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn version(value: &str) -> bool {
    let (core, prerelease) = value.split_once('-').map_or((value, None), |(a, b)| (a, Some(b)));
    let numbers: Vec<_> = core.split('.').collect();
    numbers.len() == 3
        && numbers.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| c.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
        && prerelease.is_none_or(|tail| {
            !tail.is_empty()
                && tail.bytes().all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
        })
}

fn github_handle(value: &str) -> bool {
    value.len() <= 39
        && value.split('-').all(|part| !part.is_empty())
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

/// `https://github.com/<owner>/<repo>` with nothing else.
fn github_repository(url: &str) -> Option<(&str, &str)> {
    let (owner, repo) = url.strip_prefix("https://github.com/")?.split_once('/')?;
    (!owner.is_empty()
        && owner.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        && !repo.is_empty()
        && !matches!(repo, "." | "..")
        && repo.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)))
    .then_some((owner, repo))
}

/// A literal path segment: no escapes, queries, fragments or traversal.
fn url_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !matches!(value, "." | "..")
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
}
