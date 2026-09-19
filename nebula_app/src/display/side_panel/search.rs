//! On-demand filename search with bounded warm caches and streamed results.

use super::*;
use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

mod cache;
mod matching;
mod memory;
#[cfg(test)]
mod stress;
#[cfg(test)]
mod tests;
mod walk;
mod worker;

use matching::*;
pub(crate) use memory::FileSearchMemory;
pub(crate) use worker::EmbeddedFileIndex;

const MAX_SEARCH_RESULTS: usize = 1_000;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(100);
const INDEX_IDLE: u8 = 0;
const INDEX_BUILDING: u8 = 1;
const INDEX_READY: u8 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileSearchOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIndexStatus {
    Idle,
    Building,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileIndexRoot {
    Local(PathBuf),
    Wsl(crate::shell_detect::WslCwd),
}

#[derive(Clone)]
struct SearchRequest {
    epoch: u64,
    generation: u64,
    query: String,
    options: FileSearchOptions,
}

#[derive(Debug)]
pub(crate) struct FileSearchResult {
    pub(crate) epoch: u64,
    pub(crate) generation: u64,
    pub(crate) query: String,
    pub(crate) options: FileSearchOptions,
    pub(crate) rows: Vec<FileRow>,
    pub(crate) total: usize,
    pub(crate) error: Option<String>,
    pub(crate) memory: FileSearchMemory,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone)]
struct IndexedPath {
    path: PathBuf,
    guest_path: Option<String>,
    name: String,
    name_folded: String,
    key: String,
    key_folded: String,
    is_dir: bool,
}

impl IndexedPath {
    fn heap_bytes(&self) -> usize {
        self.path.capacity()
            + self.guest_path.as_ref().map_or(0, String::capacity)
            + self.name.capacity()
            + self.name_folded.capacity()
            + self.key.capacity()
            + self.key_folded.capacity()
    }
}

fn event_changes_names(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Any
            | EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_))
    )
}

fn indexed_local_path(root: &Path, path: &Path, is_dir: bool) -> Option<IndexedPath> {
    let relative = path.strip_prefix(root).ok()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    let key = relative.to_string_lossy().replace('\\', "/");
    Some(IndexedPath {
        path: path.to_path_buf(),
        guest_path: None,
        name_folded: name.to_lowercase(),
        key_folded: key.to_lowercase(),
        name,
        key,
        is_dir,
    })
}
