//! Dirty notification and byte/frame credit. Slow receivers retain one dirty
//! bit, never a growing queue of stale terminal grids.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, mpsc};

const WINDOW_FRAMES: usize = 4;
const WINDOW_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct Entries {
    next: u64,
    live: HashMap<u64, Arc<Watch>>,
}

#[derive(Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<Entries>>);

pub(super) struct Watch {
    target: (u64, u64),
    wake: mpsc::SyncSender<()>,
    state: Mutex<Credit>,
}

#[derive(Default)]
struct Credit {
    dirty: bool,
    closed: bool,
    sent: u64,
    acked: u64,
    outstanding: VecDeque<(u64, usize)>,
    bytes: usize,
    waiting_bytes: usize,
}

pub(super) struct Registration {
    pub id: u64,
    pub watch: Arc<Watch>,
    pub receiver: mpsc::Receiver<()>,
    registry: Registry,
}

impl Registry {
    pub(super) fn register(&self, target: (u64, u64)) -> Registration {
        let (wake, receiver) = mpsc::sync_channel(1);
        let watch = Arc::new(Watch {
            target,
            wake,
            state: Mutex::new(Credit { dirty: true, ..Default::default() }),
        });
        let mut entries = self.0.lock().unwrap();
        entries.next += 1;
        let id = entries.next;
        entries.live.insert(id, watch.clone());
        Registration { id, watch, receiver, registry: self.clone() }
    }

    pub(crate) fn changed(&self, window: u64, pane: u64) {
        for watch in self.0.lock().unwrap().live.values() {
            if watch.target == (window, pane) {
                watch.state.lock().unwrap().dirty = true;
                let _ = watch.wake.try_send(());
            }
        }
    }

    pub(super) fn control(&self, id: u64, target: (u64, u64), ack: Option<u64>) -> bool {
        let entries = self.0.lock().unwrap();
        let Some(watch) = entries.live.get(&id).filter(|watch| watch.target == target) else {
            return false;
        };
        let mut state = watch.state.lock().unwrap();
        if let Some(ack) = ack {
            if ack < state.acked || ack > state.sent {
                return false;
            }
            state.acked = ack;
            while state.outstanding.front().is_some_and(|(seq, _)| *seq <= ack) {
                let (_, bytes) = state.outstanding.pop_front().unwrap();
                state.bytes -= bytes;
            }
        } else {
            state.closed = true;
        }
        let _ = watch.wake.try_send(());
        true
    }
}

impl Watch {
    pub fn closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }
    pub fn blocked(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.blocked()
    }

    pub fn take_dirty(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closed || state.blocked() {
            return false;
        }
        std::mem::take(&mut state.dirty)
    }

    // Reserve before writing: a fast peer may acknowledge the last socket byte
    // immediately. Even an oversized full recovery grid has one bounded slot.
    pub fn reserve(&self, bytes: usize) -> Option<u64> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return None;
        }
        if state.outstanding.len() >= WINDOW_FRAMES
            || (!state.outstanding.is_empty() && state.bytes.saturating_add(bytes) > WINDOW_BYTES)
        {
            state.dirty = true;
            state.waiting_bytes = bytes;
            return None;
        }
        state.waiting_bytes = 0;
        state.sent += 1;
        let sequence = state.sent;
        state.outstanding.push_back((sequence, bytes));
        state.bytes += bytes;
        Some(sequence)
    }
}

impl Credit {
    fn blocked(&self) -> bool {
        self.outstanding.len() >= WINDOW_FRAMES
            || (!self.outstanding.is_empty()
                && (self.bytes >= WINDOW_BYTES
                    || self.bytes.saturating_add(self.waiting_bytes) > WINDOW_BYTES))
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.registry.0.lock().unwrap().live.remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_latency_stream_bounds_credit_and_coalesces_latest_state() {
        let registry = Registry::default();
        let sub = registry.register((1, 2));
        assert!(sub.watch.take_dirty());
        for _ in 0..WINDOW_FRAMES {
            assert!(sub.watch.reserve(10).is_some());
        }
        for _ in 0..10_000 {
            registry.changed(1, 2);
        }
        assert!(!sub.watch.take_dirty());
        assert!(sub.receiver.try_recv().is_ok());
        assert!(sub.receiver.try_recv().is_err());
        assert!(!registry.control(sub.id, (1, 3), Some(4)));
        assert!(!registry.control(sub.id, (1, 2), Some(5)));
        assert!(registry.control(sub.id, (1, 2), Some(4)));
        assert!(sub.watch.take_dirty());
        assert!(!sub.watch.take_dirty());
        registry.changed(2, 2);
        assert!(!sub.watch.take_dirty());
    }

    #[test]
    fn mobile_latency_stream_large_snapshot_waits_for_ack_and_releases_owner() {
        let registry = Registry::default();
        let sub = registry.register((1, 2));
        sub.watch.take_dirty();
        assert_eq!(sub.watch.reserve(10), Some(1));
        assert_eq!(sub.watch.reserve(2 * 1024 * 1024), None);
        assert!(!sub.watch.take_dirty());
        assert!(registry.control(sub.id, (1, 2), Some(1)));
        assert!(sub.watch.take_dirty());
        assert_eq!(sub.watch.reserve(2 * 1024 * 1024), Some(2));
        registry.changed(1, 2);
        assert!(!sub.watch.take_dirty());
        assert!(registry.control(sub.id, (1, 2), Some(2)));
        assert!(sub.watch.take_dirty());
        assert!(!registry.control(sub.id, (1, 2), Some(0)));
        assert!(registry.control(sub.id, (1, 2), None));
        assert!(sub.watch.closed());
        assert!(!sub.watch.take_dirty());
        let id = sub.id;
        drop(sub);
        assert!(!registry.control(id, (1, 2), None));
        assert!(registry.0.lock().unwrap().live.is_empty());
    }
}
