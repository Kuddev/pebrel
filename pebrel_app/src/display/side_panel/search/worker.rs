//! Search ownership, latest-request mailbox, cancellation and warm-cache reuse.

use super::cache::FileCache;
use super::*;

// Only one CPU/I/O search runs at a time; views never wait for this lock.
// Queued windows retain one latest request, not one task per keystroke.
static SEARCH_WORK: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct DesiredSearch {
    root: Option<FileIndexRoot>,
    query: Option<SearchRequest>,
    epoch: u64,
    refresh: u64,
}

struct SearchState {
    desired: Mutex<DesiredSearch>,
    wake: Condvar,
    revision: AtomicU64,
    processed: AtomicU64,
    stopped: AtomicBool,
    result: Mutex<Option<FileSearchResult>>,
    status: AtomicU8,
    indexed_count: AtomicUsize,
    truncated: AtomicBool,
    #[cfg(test)]
    scans: AtomicUsize,
    #[cfg(test)]
    work_deferrals: AtomicUsize,
    #[cfg(test)]
    retained_bytes: AtomicUsize,
    #[cfg(test)]
    allocations: Mutex<CacheAllocations>,
}

#[cfg(test)]
#[derive(Clone, Debug, Default, serde::Serialize)]
pub(crate) struct CacheAllocations {
    pub records: usize,
    pub array_capacity: usize,
    pub array_address: usize,
    pub array_bytes: usize,
    pub string_bytes: usize,
}

#[cfg(test)]
fn record_allocations(state: &SearchState, cache: &FileCache) {
    state.retained_bytes.store(cache.memory.bytes(), Ordering::Release);
    *state.allocations.lock().unwrap() = CacheAllocations {
        records: cache.entries.len(),
        array_capacity: cache.entries.capacity(),
        array_address: cache.entries.as_ptr() as usize,
        array_bytes: cache.entries.capacity() * std::mem::size_of::<IndexedPath>(),
        string_bytes: cache.entries.iter().map(IndexedPath::heap_bytes).sum(),
    };
}

pub(crate) struct EmbeddedFileIndex {
    state: Arc<SearchState>,
}

impl EmbeddedFileIndex {
    pub(crate) fn new() -> Self {
        let state = Arc::new(SearchState {
            desired: Mutex::new(DesiredSearch::default()),
            wake: Condvar::new(),
            revision: AtomicU64::new(1),
            processed: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            result: Mutex::new(None),
            status: AtomicU8::new(INDEX_IDLE),
            indexed_count: AtomicUsize::new(0),
            truncated: AtomicBool::new(false),
            #[cfg(test)]
            scans: AtomicUsize::new(0),
            #[cfg(test)]
            work_deferrals: AtomicUsize::new(0),
            #[cfg(test)]
            retained_bytes: AtomicUsize::new(0),
            #[cfg(test)]
            allocations: Mutex::new(CacheAllocations::default()),
        });
        let worker = state.clone();
        let _ = std::thread::Builder::new()
            .name("pebrel file search".to_owned())
            .spawn(move || run_search_worker(worker));
        Self { state }
    }

    fn update(&self, edit: impl FnOnce(&mut DesiredSearch)) -> u64 {
        let mut desired = self.state.desired.lock().unwrap_or_else(|e| e.into_inner());
        edit(&mut desired);
        *self.state.result.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.state.status.store(
            if desired.query.is_some() { INDEX_BUILDING } else { INDEX_IDLE },
            Ordering::Release,
        );
        self.state.truncated.store(false, Ordering::Release);
        let revision = self.state.revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.state.wake.notify_all();
        revision
    }

    pub(crate) fn rebuild(
        &self,
        root: Option<FileIndexRoot>,
        epoch: u64,
        query: Option<(u64, String, FileSearchOptions)>,
    ) {
        self.update(|desired| {
            desired.root = root;
            desired.epoch = epoch;
            desired.query = query.map(|(generation, query, options)| SearchRequest {
                epoch,
                generation,
                query,
                options,
            });
        });
    }

    pub(crate) fn query(
        &self,
        epoch: u64,
        generation: u64,
        query: String,
        options: FileSearchOptions,
    ) {
        self.update(|desired| {
            desired.query = Some(SearchRequest { epoch, generation, query, options });
        });
    }

    pub(crate) fn clear_query(&self) {
        self.update(|desired| desired.query = None);
    }

    pub(crate) fn refresh(&self, epoch: u64) {
        self.update(|desired| {
            if desired.epoch == epoch {
                desired.refresh = desired.refresh.wrapping_add(1);
            }
        });
    }

    pub(crate) fn take_result(&self) -> Option<FileSearchResult> {
        self.state.result.lock().ok()?.take()
    }

    pub(crate) fn status(&self) -> FileIndexStatus {
        match self.state.status.load(Ordering::Acquire) {
            INDEX_BUILDING => FileIndexStatus::Building,
            INDEX_READY => FileIndexStatus::Ready,
            _ => FileIndexStatus::Idle,
        }
    }

    pub(crate) fn indexed_count(&self) -> usize {
        self.state.indexed_count.load(Ordering::Acquire)
    }

    pub(crate) fn truncated(&self) -> bool {
        self.state.truncated.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn scans(&self) -> usize {
        self.state.scans.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.state.retained_bytes.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn allocation_snapshot(&self) -> CacheAllocations {
        self.state.allocations.lock().unwrap().clone()
    }

    #[cfg(test)]
    pub(crate) fn release_for_test(&self) {
        let revision = self.update(|desired| *desired = DesiredSearch::default());
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.state.processed.load(Ordering::Acquire) < revision {
            assert!(Instant::now() < deadline, "search worker did not release its resources");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for EmbeddedFileIndex {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        self.state.revision.fetch_add(1, Ordering::AcqRel);
        self.state.wake.notify_all();
    }
}

fn wait_for_work(state: &SearchState, duration: Duration) {
    let guard = state.desired.lock().unwrap_or_else(|e| e.into_inner());
    let _ = state.wake.wait_timeout(guard, duration);
}

fn run_search_worker(state: Arc<SearchState>) {
    let mut cache = FileCache::new();
    let mut root = None;
    let mut epoch = 0;
    let mut refresh = 0;
    loop {
        if state.stopped.load(Ordering::Acquire) {
            return;
        }
        let (desired, revision) = {
            let desired = state.desired.lock().unwrap_or_else(|e| e.into_inner());
            (desired.clone(), state.revision.load(Ordering::Acquire))
        };
        let changed = root != desired.root || epoch != desired.epoch || refresh != desired.refresh;
        let dirty = cache.watches.dirty.swap(false, Ordering::AcqRel);
        if changed || dirty {
            // A watch event can invalidate an already-processed query. Keep
            // it pending if debounce or the shared work lock defers this scan.
            state.processed.store(0, Ordering::Release);
            cache = FileCache::new();
            root = desired.root.clone();
            epoch = desired.epoch;
            refresh = desired.refresh;
        } else if revision == state.processed.load(Ordering::Acquire) {
            wait_for_work(&state, WATCH_DEBOUNCE);
            continue;
        }
        let cancelled = || {
            state.stopped.load(Ordering::Acquire)
                || state.revision.load(Ordering::Acquire) != revision
        };
        let Some(request) = desired.query.as_ref().filter(|query| query.epoch == epoch) else {
            state.indexed_count.store(cache.entries.len(), Ordering::Release);
            state.status.store(INDEX_IDLE, Ordering::Release);
            #[cfg(test)]
            record_allocations(&state, &cache);
            state.processed.store(revision, Ordering::Release);
            state.wake.notify_all();
            continue;
        };
        wait_for_work(&state, SEARCH_DEBOUNCE);
        if cancelled() {
            continue;
        }
        let _work = match SEARCH_WORK.try_lock() {
            Ok(work) => work,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                #[cfg(test)]
                state.work_deferrals.fetch_add(1, Ordering::Release);
                wait_for_work(&state, WATCH_DEBOUNCE);
                continue;
            },
        };
        let matcher = match QueryMatcher::compile(&request.query, request.options) {
            Ok(matcher) => matcher,
            Err(error) => {
                publish(
                    &state,
                    revision,
                    request,
                    &BestMatches::new(request),
                    Some(error),
                    false,
                    true,
                );
                continue;
            },
        };
        let mut best = BestMatches::new(request);
        for entry in &cache.entries {
            if cancelled() {
                break;
            }
            if matcher.matches(entry) {
                best.add(entry.clone());
            }
        }
        let reusable = cache.complete
            && (!cache.watches.unwatched || cache.finished.elapsed() < Duration::from_secs(2));
        if reusable {
            publish(&state, revision, request, &best, None, false, true);
            continue;
        }
        if !cache.entries.is_empty() {
            publish(&state, revision, request, &best, None, true, false);
        }
        best = BestMatches::new(request);
        let mut position = 0;
        let mut last_publish = Instant::now();
        // Install each non-recursive watch before enumerating that directory.
        // Keep it independent from entry storage so both traversal callbacks
        // can mutate their owner without delaying watches until after the scan.
        let mut watches = std::mem::replace(&mut cache.watches, cache::DirectoryWatches::new());
        #[cfg(test)]
        state.scans.fetch_add(1, Ordering::AcqRel);
        let mut visit = |entry: IndexedPath| {
            cache.record(position, &entry);
            position += 1;
            if matcher.matches(&entry) {
                best.add(entry);
            }
            if last_publish.elapsed() >= Duration::from_millis(100) {
                state.indexed_count.store(position, Ordering::Release);
                publish(&state, revision, request, &best, None, true, false);
                last_publish = Instant::now();
            }
        };
        let outcome = match root.as_ref() {
            Some(FileIndexRoot::Local(root)) => walk::local(
                root,
                cancelled,
                |directory| {
                    watches.watch(directory);
                },
                &mut visit,
            ),
            Some(FileIndexRoot::Wsl(located)) => walk::wsl(located, cancelled, &mut visit),
            None => walk::WalkOutcome::default(),
        };
        if matches!(root, Some(FileIndexRoot::Wsl(_))) {
            watches.unwatched = true;
        }
        cache.watches = watches;
        if !cancelled() && !outcome.limited && outcome.error.is_none() {
            cache.finish_prefix(position);
        }
        cache.complete = !cancelled() && !outcome.limited && outcome.error.is_none() && !cache.full;
        cache.finished = Instant::now();
        state.indexed_count.store(outcome.visited, Ordering::Release);
        #[cfg(test)]
        record_allocations(&state, &cache);
        publish(&state, revision, request, &best, outcome.error, outcome.limited, true);
    }
}

fn publish(
    state: &SearchState,
    revision: u64,
    request: &SearchRequest,
    best: &BestMatches,
    error: Option<String>,
    limited: bool,
    complete: bool,
) {
    let (mut result, result_limited) = best.result(request);
    result.error = error;
    result.complete = complete;
    let desired = state.desired.lock().unwrap_or_else(|e| e.into_inner());
    if state.revision.load(Ordering::Acquire) != revision || state.stopped.load(Ordering::Acquire) {
        return;
    }
    *state.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    state.truncated.store(limited || result_limited || best.limited, Ordering::Release);
    state.status.store(if complete { INDEX_READY } else { INDEX_BUILDING }, Ordering::Release);
    if complete {
        state.processed.store(revision, Ordering::Release);
    }
    drop(desired);
    state.wake.notify_all();
}

#[cfg(test)]
mod tests {
    use super::super::tests::wait_for_result;
    use super::*;

    #[test]
    fn watched_change_stays_pending_while_another_search_owns_the_work_lock() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let index = EmbeddedFileIndex::new();
        index.rebuild(
            Some(FileIndexRoot::Local(directory.path().to_owned())),
            1,
            Some((1, "needle".to_owned(), FileSearchOptions::default())),
        );
        wait_for_result(&index, |result| result.total == 0);

        let work = SEARCH_WORK.lock().unwrap_or_else(|error| error.into_inner());
        let scans = index.scans();
        let revision = index.state.revision.load(Ordering::Acquire);
        let deferrals = index.state.work_deferrals.load(Ordering::Acquire);
        std::fs::write(nested.join("needle.txt"), b"").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while index.state.work_deferrals.load(Ordering::Acquire) == deferrals {
            assert!(
                Instant::now() < deadline,
                "watch invalidation did not reach the held work lock"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(index.scans(), scans, "the contending worker cannot scan under the held lock");
        assert_ne!(index.state.processed.load(Ordering::Acquire), revision);
        drop(work);

        let result = wait_for_result(&index, |result| result.total == 1);
        assert_eq!(result.rows[0].path, nested.join("needle.txt"));
        assert!(index.scans() > scans);
        index.release_for_test();
    }
}
