//! Durable custom-theme library.
//!
//! Built-in themes remain virtual values from `nebula_settings`; only user
//! themes are files.  The store is deliberately synchronous and cold-path:
//! callers run it on their background executor and retain the returned
//! `ThemeDocument`/`ThemeDefinition` for rendering.

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::document::{DocumentError, MAX_DOCUMENT_BYTES, ThemeDocument, builtin_documents};

pub const MAX_CUSTOM_THEMES: usize = 128;
pub const THEME_FILE_SUFFIX: &str = ".pebrel-theme.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevisionPrecondition {
    /// Create a file only when no file with this ID exists.
    Absent,
    /// Update only when the current document has this revision.
    Exact(u64),
    /// Allow either creation or update, retaining the current revision chain.
    Any,
    /// Require the complete document snapshot to be unchanged.  This catches
    /// external edits that keep the same numeric revision.
    Unchanged(ThemeDocument),
}

#[derive(Debug)]
pub enum StoreError {
    Io { path: PathBuf, source: io::Error },
    Document(DocumentError),
    InvalidId(String),
    InvalidDocument(String),
    NotFound(String),
    BuiltinReadOnly(String),
    Busy,
    Conflict { expected: Option<u64>, actual: Option<u64> },
    Limit { limit: usize },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "theme store I/O at {}: {source}", path.display())
            },
            Self::Document(error) => error.fmt(f),
            Self::InvalidId(id) => write!(f, "invalid theme id: {id}"),
            Self::InvalidDocument(message) => write!(f, "invalid stored theme: {message}"),
            Self::NotFound(id) => write!(f, "theme not found: {id}"),
            Self::BuiltinReadOnly(id) => write!(f, "built-in theme is read-only: {id}"),
            Self::Busy => f.write_str("theme library is busy; retry the operation"),
            Self::Conflict { expected, actual } => {
                write!(f, "theme revision conflict: expected {expected:?}, found {actual:?}")
            },
            Self::Limit { limit } => write!(f, "custom theme limit reached ({limit})"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Document(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DocumentError> for StoreError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeLibrarySnapshot {
    pub builtins: Vec<ThemeDocument>,
    pub custom: Vec<ThemeDocument>,
    pub diagnostics: Vec<StoreDiagnostic>,
}

#[derive(Clone, Debug)]
pub struct ThemeLibraryStore {
    root: PathBuf,
}

impl Default for ThemeLibraryStore {
    fn default() -> Self {
        Self::new(nebula_settings::settings_dir().join("themes"))
    }
}

impl ThemeLibraryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the exact path used by a custom theme.  This is also useful to
    /// diagnostics and tests without exposing a second path construction rule.
    pub fn path_for(&self, id: &str) -> Result<PathBuf, StoreError> {
        validate_id(id)?;
        Ok(self.root.join(format!("{id}{THEME_FILE_SUFFIX}")))
    }

    pub fn list(&self) -> Result<ThemeLibrarySnapshot, StoreError> {
        let mut snapshot = ThemeLibrarySnapshot {
            builtins: builtin_documents(),
            ..ThemeLibrarySnapshot::default()
        };
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(snapshot),
            Err(error) => return Err(io_error(&self.root, error)),
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    snapshot.diagnostics.push(StoreDiagnostic {
                        path: self.root.clone(),
                        message: format!("unable to inspect theme entry: {error}"),
                    });
                    continue;
                },
            };
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(id) = file_name.strip_suffix(THEME_FILE_SUFFIX) else {
                continue;
            };
            if let Err(error) = validate_id(id) {
                snapshot.diagnostics.push(StoreDiagnostic { path, message: error.to_string() });
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    snapshot.diagnostics.push(StoreDiagnostic {
                        path,
                        message: format!("unable to inspect file type: {error}"),
                    });
                    continue;
                },
            };
            if file_type.is_symlink() {
                snapshot.diagnostics.push(StoreDiagnostic {
                    path,
                    message: "symbolic links are not accepted as theme files".to_owned(),
                });
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            match read_document(&path, id) {
                Ok(Some(document)) => snapshot.custom.push(document),
                Ok(None) => snapshot.diagnostics.push(StoreDiagnostic {
                    path,
                    message: "theme file disappeared while scanning".to_owned(),
                }),
                Err(error) => {
                    snapshot.diagnostics.push(StoreDiagnostic { path, message: error.to_string() })
                },
            }
        }

        snapshot.custom.sort_by(|left, right| {
            left.name().cmp(right.name()).then_with(|| left.id().cmp(&right.id()))
        });
        Ok(snapshot)
    }

    pub fn load(&self, id: &str) -> Result<ThemeDocument, StoreError> {
        if let Some(document) = builtin_documents()
            .into_iter()
            .find(|document| document.id() == Some(id) || document.name() == id)
        {
            return Ok(document);
        }
        let path = self.path_for(id)?;
        read_document(&path, id)?.ok_or_else(|| StoreError::NotFound(id.to_owned()))
    }

    pub fn load_theme(&self, id: &str) -> Result<ThemeDocument, StoreError> {
        self.load(id)
    }

    pub fn save(
        &self,
        document: &ThemeDocument,
        precondition: RevisionPrecondition,
    ) -> Result<ThemeDocument, StoreError> {
        if document.is_builtin() {
            return Err(StoreError::BuiltinReadOnly(document.name().to_owned()));
        }
        let id = document
            .id()
            .ok_or_else(|| StoreError::InvalidDocument("custom themes require an id".to_owned()))?;
        let path = self.path_for(id)?;
        // Resolve once before taking the filesystem locks.  This validates all
        // runtime scalar semantics without doing JSON work while the lock is held.
        document.definition()?;
        let _global_lock = self.acquire_global_lock()?;
        let _target_lock = self.acquire_target_lock(&path)?;
        let current = read_document(&path, id)?;
        let actual = current.as_ref().map(ThemeDocument::revision);
        let expected = match precondition {
            RevisionPrecondition::Absent => {
                if current.is_some() {
                    return Err(StoreError::Conflict { expected: None, actual });
                }
                None
            },
            RevisionPrecondition::Exact(expected) => {
                if actual != Some(expected) {
                    return Err(StoreError::Conflict { expected: Some(expected), actual });
                }
                Some(expected)
            },
            RevisionPrecondition::Any => actual,
            RevisionPrecondition::Unchanged(expected_document) => {
                if expected_document.id() != Some(id) {
                    return Err(StoreError::InvalidDocument(
                        "unchanged precondition belongs to another theme id".to_owned(),
                    ));
                }
                if current.as_ref() != Some(&expected_document) {
                    return Err(StoreError::Conflict {
                        expected: Some(expected_document.revision()),
                        actual,
                    });
                }
                Some(expected_document.revision())
            },
        };

        if current.is_none() && self.custom_file_count()? >= MAX_CUSTOM_THEMES {
            return Err(StoreError::Limit { limit: MAX_CUSTOM_THEMES });
        }
        let revision = expected.map_or(1, |revision| revision.saturating_add(1));
        let stored = document.with_id_and_revision(id.to_owned(), revision);
        let bytes = stored.to_json_bytes()?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(StoreError::Document(DocumentError::TooLarge { bytes: bytes.len() }));
        }
        fs::create_dir_all(&self.root).map_err(|error| io_error(&self.root, error))?;
        // Re-check after locking so a symlink swap cannot be atomically
        // replaced as a regular theme file.
        reject_symlink(&path)?;
        crate::atomic_file::write(&path, &bytes).map_err(|error| io_error(&path, error))?;
        Ok(stored)
    }

    /// Clone a built-in theme, choose a collision-free display name and save it
    /// immediately.  The returned custom theme therefore starts at revision 1.
    pub fn fork_builtin(
        &self,
        builtin_name_or_id: &str,
        requested_name: Option<&str>,
    ) -> Result<ThemeDocument, StoreError> {
        let builtin = builtin_documents()
            .into_iter()
            .find(|document| {
                document.name() == builtin_name_or_id || document.id() == Some(builtin_name_or_id)
            })
            .ok_or_else(|| StoreError::NotFound(builtin_name_or_id.to_owned()))?;
        let snapshot = self.list()?;
        let name = unique_name(requested_name.unwrap_or(builtin.name()), &snapshot)?;
        let id = self.unique_id(&name, &snapshot)?;
        let candidate = builtin.fork(id, name)?;
        self.save(&candidate, RevisionPrecondition::Absent)
    }

    /// Import any validated document as a new custom theme.  Native IDs are
    /// intentionally ignored so an import cannot overwrite an existing file.
    pub fn import(
        &self,
        document: &ThemeDocument,
        requested_name: Option<&str>,
    ) -> Result<ThemeDocument, StoreError> {
        let snapshot = self.list()?;
        let name = unique_name(requested_name.unwrap_or(document.name()), &snapshot)?;
        let id = self.unique_id(&name, &snapshot)?;
        let candidate = document.with_name(name)?.with_id_and_revision(id, 0);
        self.save(&candidate, RevisionPrecondition::Absent)
    }

    pub fn delete(&self, id: &str, expected_revision: u64) -> Result<(), StoreError> {
        if builtin_documents()
            .into_iter()
            .any(|document| document.id() == Some(id) || document.name() == id)
        {
            return Err(StoreError::BuiltinReadOnly(id.to_owned()));
        }
        let path = self.path_for(id)?;
        let _global_lock = self.acquire_global_lock()?;
        let _target_lock = self.acquire_target_lock(&path)?;
        let current = read_document(&path, id)?;
        let Some(document) = current else {
            return Err(StoreError::Conflict { expected: Some(expected_revision), actual: None });
        };
        if document.revision() != expected_revision {
            return Err(StoreError::Conflict {
                expected: Some(expected_revision),
                actual: Some(document.revision()),
            });
        }
        fs::remove_file(&path).map_err(|error| io_error(&path, error))
    }

    fn acquire_global_lock(&self) -> Result<crate::atomic_file::LifetimeFileLock, StoreError> {
        let path = self.root.join(".pebrel-theme-library");
        crate::atomic_file::try_lifetime_lock(&path)
            .map_err(|error| io_error(&path, error))?
            .ok_or(StoreError::Busy)
    }

    fn acquire_target_lock(
        &self,
        path: &Path,
    ) -> Result<crate::atomic_file::LifetimeFileLock, StoreError> {
        crate::atomic_file::try_lifetime_lock(path)
            .map_err(|error| io_error(path, error))?
            .ok_or(StoreError::Busy)
    }

    fn custom_file_count(&self) -> Result<usize, StoreError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(io_error(&self.root, error)),
        };
        let mut count = 0;
        for entry in entries {
            let entry = entry.map_err(|error| io_error(&self.root, error))?;
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(id) = file_name.strip_suffix(THEME_FILE_SUFFIX) else {
                continue;
            };
            if validate_id(id).is_err()
                || entry.file_type().map_err(|error| io_error(&path, error))?.is_symlink()
            {
                continue;
            }
            count += 1;
        }
        Ok(count)
    }

    fn unique_id(&self, name: &str, snapshot: &ThemeLibrarySnapshot) -> Result<String, StoreError> {
        let base = slug(name);
        for suffix in 1..=MAX_CUSTOM_THEMES + 1 {
            let candidate = if suffix == 1 { base.clone() } else { format!("{base}-{suffix}") };
            if validate_id(&candidate).is_err() {
                continue;
            }
            if snapshot.custom.iter().all(|document| document.id() != Some(candidate.as_str()))
                && !self.path_for(&candidate)?.exists()
            {
                return Ok(candidate);
            }
        }
        Err(StoreError::Limit { limit: MAX_CUSTOM_THEMES })
    }
}

fn read_document(path: &Path, expected_id: &str) -> Result<Option<ThemeDocument>, StoreError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(path, error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(StoreError::InvalidDocument(format!(
            "symbolic links are not accepted: {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(StoreError::InvalidDocument(format!(
            "theme path is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_DOCUMENT_BYTES as u64 {
        return Err(StoreError::Document(DocumentError::TooLarge {
            bytes: metadata.len() as usize,
        }));
    }
    let mut file = fs::File::open(path).map_err(|error| io_error(path, error))?;
    if fs::symlink_metadata(path).map_err(|error| io_error(path, error))?.file_type().is_symlink() {
        return Err(StoreError::InvalidDocument(format!(
            "symbolic links are not accepted: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error(path, error))?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(StoreError::Document(DocumentError::TooLarge { bytes: bytes.len() }));
    }
    let document = ThemeDocument::from_json_bytes(&bytes)?;
    if document.is_builtin() {
        return Err(StoreError::InvalidDocument(
            "custom theme files cannot be marked builtin".to_owned(),
        ));
    }
    if document.id() != Some(expected_id) {
        return Err(StoreError::InvalidDocument(format!(
            "document id {:?} does not match file id {expected_id}",
            document.id()
        )));
    }
    Ok(Some(document))
}

fn reject_symlink(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StoreError::InvalidDocument(
            format!("symbolic links are not accepted: {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
    }
}

fn validate_id(id: &str) -> Result<(), StoreError> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(StoreError::InvalidId(id.to_owned()));
    }
    Ok(())
}

const MAX_THEME_NAME_BYTES: usize = 128;

fn unique_name(requested: &str, snapshot: &ThemeLibrarySnapshot) -> Result<String, StoreError> {
    let base = requested.trim();
    let base = if base.is_empty() { "Imported theme" } else { base };
    let occupied = |name: &str| {
        snapshot
            .builtins
            .iter()
            .chain(snapshot.custom.iter())
            .any(|document| document.name().eq_ignore_ascii_case(name))
    };
    // Check the final bounded candidate, rather than the unbounded source
    // name. Otherwise a long name can truncate into an existing name.
    for suffix_number in 0..=(MAX_CUSTOM_THEMES + 1) {
        let suffix =
            if suffix_number == 0 { String::new() } else { format!(" ({})", suffix_number + 1) };
        let base_budget = MAX_THEME_NAME_BYTES.saturating_sub(suffix.len());
        let candidate = format!("{}{}", truncate_name(base, base_budget), suffix);
        if !candidate.is_empty() && !occupied(&candidate) {
            return Ok(candidate);
        }
    }
    Err(StoreError::Limit { limit: MAX_CUSTOM_THEMES })
}

fn truncate_name(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn slug(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            output.push(character.to_ascii_lowercase());
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    let output = output.trim_matches('-');
    if output.is_empty() { "theme".to_owned() } else { output.chars().take(128).collect() }
}

fn io_error(path: &Path, source: io::Error) -> StoreError {
    StoreError::Io { path: path.to_owned(), source }
}
