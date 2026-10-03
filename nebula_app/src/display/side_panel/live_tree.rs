//! Local tree invalidation, independent of deep filename search and VCS reads.
//!
//! The UI only exchanges bounded desired scopes/atomic flags. A lazily owned
//! worker installs and releases native watches; it never scans directories.

use super::FileRow;
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const MAX_WATCHES: usize = 1024;
const MAX_PATH_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WatchScope {
    root: PathBuf,
    directories: Vec<PathBuf>,
}

impl WatchScope {
    pub(super) fn new(root: &Path, rows: &[FileRow]) -> Self {
        let mut directories = vec![root.to_owned()];
        let mut bytes = root.as_os_str().len();
        for row in rows {
            if !row.is_dir || !row.expanded || row.is_parent || !row.path.starts_with(root) {
                continue;
            }
            let cost = row.path.as_os_str().len();
            // Leave one handle for the parent, which observes root replacement.
            if directories.len() >= MAX_WATCHES - 1 || bytes + cost > MAX_PATH_BYTES {
                break;
            }
            directories.push(row.path.clone());
            bytes += cost;
        }
        directories.sort_unstable();
        directories.dedup();
        Self { root: root.to_owned(), directories }
    }

    fn observes(&self, path: &Path) -> bool {
        path == self.root
            || self.directories.binary_search_by(|dir| dir.as_path().cmp(path)).is_ok()
            || (path.file_name().is_some_and(|name| name != ".git")
                && path.parent().is_some_and(|parent| {
                    self.directories.binary_search_by(|dir| dir.as_path().cmp(parent)).is_ok()
                }))
    }

    // FSEvents reports canonical spellings. Resolve on the worker, retaining
    // the root's directory entry so replacing a symlink root also rearms it.
    fn canonicalized(&self) -> Self {
        let root = self
            .root
            .parent()
            .zip(self.root.file_name())
            .and_then(|(parent, name)| parent.canonicalize().ok().map(|path| path.join(name)))
            .unwrap_or_else(|| self.root.canonicalize().unwrap_or_else(|_| self.root.clone()));
        let mut directories = Vec::new();
        let mut bytes = 0;
        for path in &self.directories {
            let path = path.canonicalize().unwrap_or_else(|_| path.clone());
            let cost = path.as_os_str().len();
            if bytes + cost > MAX_PATH_BYTES {
                break;
            }
            bytes += cost;
            directories.push(path);
        }
        directories.sort_unstable();
        directories.dedup();
        Self { root, directories }
    }

    fn invalidates(&self, event: &Event) -> bool {
        if event.need_rescan() {
            return true;
        }
        let changes_names = matches!(
            event.kind,
            EventKind::Any
                | EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(ModifyKind::Name(_))
        );
        if changes_names && event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|path| {
            self.observes(path)
                && (changes_names
                    || (matches!(event.kind, EventKind::Modify(_))
                        && (self.directories.contains(path)
                            || path.file_name().is_some_and(|name| name == ".gitignore"))))
        })
    }
}

#[derive(Default)]
struct Signal {
    dirty: AtomicBool,
    rearm: AtomicBool,
}

impl Signal {
    fn record(&self, scope: &WatchScope, event: notify::Result<Event>) {
        let rearm = match &event {
            Ok(event) => {
                if !scope.invalidates(event) {
                    return;
                }
                event.need_rescan()
                    || (matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Remove(_)
                            | EventKind::Modify(ModifyKind::Name(_))
                    ) && event
                        .paths
                        .iter()
                        .any(|path| path == &scope.root || scope.directories.contains(path)))
            },
            Err(_) => true,
        };
        if rearm {
            self.rearm.store(true, Ordering::Release);
        }
        self.dirty.store(true, Ordering::Release);
    }
}

struct WatchRequest {
    scope: WatchScope,
    signal: Arc<Signal>,
}

#[derive(Default)]
struct Mailbox {
    pending: Mutex<Option<WatchRequest>>,
    wake: Condvar,
    stopped: AtomicBool,
}

struct WatchWorker(Arc<Mailbox>);

impl WatchWorker {
    fn new() -> Option<Self> {
        let mailbox = Arc::new(Mailbox::default());
        let worker = mailbox.clone();
        match std::thread::Builder::new()
            .name("pebrel tree watch".into())
            .spawn(move || run(worker))
        {
            Ok(_) => Some(Self(mailbox)),
            Err(error) => {
                log::warn!("Could not start file tree watcher: {error}");
                None
            },
        }
    }

    fn configure(&self, request: WatchRequest) {
        let mut pending = self.0.pending.lock().unwrap_or_else(|e| e.into_inner());
        *pending = Some(request);
        self.0.wake.notify_one();
    }
}

impl Drop for WatchWorker {
    fn drop(&mut self) {
        // Serialize with wait to avoid a shutdown notification being lost.
        let _pending = self.0.pending.lock().unwrap_or_else(|e| e.into_inner());
        self.0.stopped.store(true, Ordering::Release);
        self.0.wake.notify_one();
    }
}

#[derive(Default)]
pub(super) struct TreeWatch {
    scope: Option<WatchScope>,
    signal: Arc<Signal>,
    worker: Option<WatchWorker>,
    last_refresh: Option<Instant>,
}

impl TreeWatch {
    pub(super) fn active(&self) -> bool {
        self.scope.is_some()
    }

    pub(super) fn configure(&mut self, scope: Option<WatchScope>) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.signal = Arc::new(Signal::default());
        self.last_refresh = None;
        if let Some(scope) = &self.scope {
            if self.worker.is_none() {
                self.worker = WatchWorker::new();
            }
            if let Some(worker) = &self.worker {
                worker
                    .configure(WatchRequest { scope: scope.clone(), signal: self.signal.clone() });
            }
        } else {
            // Handles are dropped by their owner, not by the rendering thread.
            self.worker = None;
        }
    }

    pub(super) fn restart(&mut self) {
        let scope = self.scope.clone();
        self.scope = None;
        self.configure(scope);
    }

    pub(super) fn take_changed(&mut self, now: Instant) -> bool {
        if self.last_refresh.is_some_and(|last| now.duration_since(last) < REFRESH_INTERVAL)
            || !self.signal.dirty.swap(false, Ordering::AcqRel)
        {
            return false;
        }
        if self.signal.rearm.swap(false, Ordering::AcqRel) {
            self.restart();
        }
        // This is a rate limit, not a trailing-only debounce: a long clone
        // remains live, and an event during a scan remains pending afterwards.
        self.last_refresh = Some(now);
        true
    }
}

fn install(request: WatchRequest, mailbox: &Mailbox) -> Option<RecommendedWatcher> {
    let scope = request.scope.canonicalized();
    let signal = request.signal;
    let callback_scope = scope.clone();
    let callback_signal = signal.clone();
    let mut watcher = match notify::recommended_watcher(move |event: notify::Result<Event>| {
        callback_signal.record(&callback_scope, event);
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            log::warn!("File tree notifications unavailable: {error}");
            return None;
        },
    };
    // Watch the parent first so deletion/recreation of the root can rearm it.
    for path in
        scope.root.parent().into_iter().chain(scope.directories.iter().map(PathBuf::as_path))
    {
        if mailbox.stopped.load(Ordering::Acquire)
            || mailbox.pending.lock().unwrap_or_else(|e| e.into_inner()).is_some()
        {
            return None;
        }
        if let Err(error) = watcher.watch(path, RecursiveMode::NonRecursive) {
            log::debug!("File tree watch unavailable for {}: {error}", path.display());
        }
    }
    // Watches may be installed after the caller's first snapshot. One catch-up
    // scan closes that gap, including when a newly expanded directory is empty.
    signal.dirty.store(true, Ordering::Release);
    Some(watcher)
}

fn run(mailbox: Arc<Mailbox>) {
    let mut watcher = None;
    loop {
        let request = {
            let mut pending = mailbox.pending.lock().unwrap_or_else(|e| e.into_inner());
            while pending.is_none() && !mailbox.stopped.load(Ordering::Acquire) {
                pending = mailbox.wake.wait(pending).unwrap_or_else(|e| e.into_inner());
            }
            if mailbox.stopped.load(Ordering::Acquire) {
                return;
            }
            pending.take().unwrap()
        };
        drop(watcher.take());
        watcher = install(request, &mailbox);
    }
}

#[cfg(test)]
mod tests;
