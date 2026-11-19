//! Bounded warm cache. A traversal prefix is retained once and reused by queries.

use super::*;

pub(super) struct FileCache {
    pub entries: Vec<IndexedPath>,
    pub memory: FileSearchMemory,
    pub full: bool,
    pub complete: bool,
    pub watches: DirectoryWatches,
    pub finished: Instant,
}

pub(super) struct DirectoryWatches {
    pub dirty: Arc<AtomicBool>,
    watcher: Option<RecommendedWatcher>,
    watched: HashSet<PathBuf>,
    watch_bytes: usize,
    pub unwatched: bool,
}

impl FileCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            memory: FileSearchMemory::cache(),
            full: false,
            complete: false,
            watches: DirectoryWatches::new(),
            finished: Instant::now(),
        }
    }

    pub fn record(&mut self, position: usize, entry: &IndexedPath) {
        if position < self.entries.len() {
            if self.entries[position].path != entry.path
                || self.entries[position].is_dir != entry.is_dir
            {
                // An unobserved edit changed the traversal prefix. Do not
                // append duplicates or label this cache complete.
                self.entries = Vec::new();
                self.memory = FileSearchMemory::cache();
                self.full = true;
            }
            return;
        }
        if self.full || position != self.entries.len() {
            return;
        }
        let growth = if self.entries.len() == self.entries.capacity() { 128 } else { 0 };
        let bytes = entry.heap_bytes() + growth * std::mem::size_of::<IndexedPath>();
        if self.memory.bytes() + bytes > memory::CACHE_LIMIT || !self.memory.grow(bytes) {
            self.full = true;
            return;
        }
        if growth > 0 {
            self.entries.reserve_exact(growth);
        }
        self.entries.push(entry.clone());
    }

    pub fn finish_prefix(&mut self, visited: usize) {
        if visited < self.entries.len() {
            let bytes = self.entries[visited..].iter().map(IndexedPath::heap_bytes).sum();
            self.entries.truncate(visited);
            self.memory.release(bytes);
        }
    }
}

impl DirectoryWatches {
    pub fn new() -> Self {
        Self {
            dirty: Arc::new(AtomicBool::new(false)),
            watcher: None,
            watched: HashSet::new(),
            watch_bytes: 0,
            unwatched: false,
        }
    }

    pub fn watch(&mut self, directory: &Path) {
        if self.watched.contains(directory) {
            return;
        }
        if self.watched.len() >= 128 || self.watch_bytes + directory.as_os_str().len() > 64 * 1024 {
            self.unwatched = true;
            return;
        }
        if self.watcher.is_none() {
            let dirty = self.dirty.clone();
            self.watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event.as_ref().map_or(true, |event| {
                        event.need_rescan() || event_changes_names(&event.kind)
                    }) {
                        dirty.store(true, Ordering::Release);
                    }
                })
                .ok();
        }
        let Some(watcher) = self.watcher.as_mut() else {
            self.unwatched = true;
            return;
        };
        if watcher.watch(directory, RecursiveMode::NonRecursive).is_ok() {
            self.watch_bytes += directory.as_os_str().len();
            self.watched.insert(directory.to_owned());
        } else {
            self.unwatched = true;
        }
    }
}
