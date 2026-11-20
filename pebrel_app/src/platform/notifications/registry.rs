//! Bounded callback ownership for one native notification process lifetime.

use std::collections::VecDeque;

use super::{ToastAction, ToastActivation};

pub(super) const CAPACITY: usize = 64;

pub(super) struct Callbacks {
    pub(super) tag: String,
    activation: Option<ToastActivation>,
    actions: Vec<ToastAction>,
}

impl Callbacks {
    pub(super) fn activate(self, action_id: Option<&str>) {
        if let Some(id) = action_id {
            if let Some(index) =
                id.strip_prefix("choice-").and_then(|value| value.parse::<usize>().ok())
                && let Some(action) = self.actions.get(index)
            {
                (action.activate)();
            }
        } else if let Some(activate) = self.activation {
            activate();
        }
    }
}

pub(super) struct Registry {
    generation: String,
    serial: u64,
    callbacks: VecDeque<Callbacks>,
}

impl Registry {
    pub(super) fn new(generation: String) -> Self {
        Self { generation, serial: 0, callbacks: VecDeque::new() }
    }

    /// Return an owned tag and any evicted tag; native calls happen after the
    /// caller releases its registry borrow.
    pub(super) fn insert(
        &mut self,
        activation: Option<ToastActivation>,
        actions: Vec<ToastAction>,
    ) -> Option<(String, Option<String>)> {
        let serial = self.serial.checked_add(1)?;
        self.serial = serial;
        let tag = format!("pebrel-notification-{}-{serial}", self.generation);
        let evicted = if self.callbacks.len() == CAPACITY {
            self.callbacks.pop_front().map(|callback| callback.tag)
        } else {
            None
        };
        self.callbacks.push_back(Callbacks { tag: tag.clone(), activation, actions });
        Some((tag, evicted))
    }

    pub(super) fn take(&mut self, tag: &str) -> Option<Callbacks> {
        let index = self.callbacks.iter().position(|callback| callback.tag == tag)?;
        self.callbacks.remove(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn responses_select_only_the_default_or_one_valid_action() {
        for (response, expected) in [
            (None, 1),
            (Some("choice-0"), 10),
            (Some("choice-1"), 100),
            (Some("choice-2"), 0),
            (Some("choice--1"), 0),
            (Some("choice-999999999999999999999999999"), 0),
            (Some("unknown"), 0),
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let callback = |weight| {
                let calls = calls.clone();
                Arc::new(move || {
                    calls.fetch_add(weight, Ordering::SeqCst);
                }) as ToastActivation
            };
            let mut registry = Registry::new("session".into());
            let (tag, _) = registry
                .insert(
                    Some(callback(1)),
                    vec![
                        ToastAction { label: "Yes".into(), activate: callback(10) },
                        ToastAction { label: "No".into(), activate: callback(100) },
                    ],
                )
                .unwrap();
            registry.take(&tag).unwrap().activate(response);
            assert!(registry.take(&tag).is_none(), "a response is consumed once");
            assert_eq!(calls.load(Ordering::SeqCst), expected);
        }
    }

    #[test]
    fn a_previous_process_notification_cannot_activate_a_new_callback() {
        let mut previous = Registry::new("previous-process".into());
        let (old_tag, _) = previous.insert(None, Vec::new()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut current = Registry::new("current-process".into());
        let (new_tag, _) = current
            .insert(
                Some(Arc::new(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                })),
                Vec::new(),
            )
            .unwrap();
        assert_ne!(old_tag, new_tag);
        assert!(current.take(&old_tag).is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        current.take(&new_tag).unwrap().activate(None);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn eviction_and_counter_exhaustion_never_reuse_callback_identity() {
        let mut registry = Registry::new("session".into());
        let (first, _) = registry.insert(None, Vec::new()).unwrap();
        for _ in 1..CAPACITY {
            assert!(registry.insert(None, Vec::new()).unwrap().1.is_none());
        }
        let (last, evicted) = registry.insert(None, Vec::new()).unwrap();
        assert_eq!(evicted.as_deref(), Some(first.as_str()));
        assert_eq!(registry.callbacks.len(), CAPACITY);
        assert!(registry.take(&first).is_none());
        assert!(registry.take(&last).is_some());
        registry.serial = u64::MAX;
        let before = registry.callbacks.len();
        assert!(registry.insert(None, Vec::new()).is_none());
        assert_eq!(registry.callbacks.len(), before);
    }
}
