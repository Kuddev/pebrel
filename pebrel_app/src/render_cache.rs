//! Byte and entry bounded LRU used by application-owned render resources.

use std::collections::HashMap;
use std::hash::Hash;

struct Entry<V> {
    value: V,
    bytes: usize,
    touched: u64,
}

pub(crate) struct RenderCache<K, V> {
    entries: HashMap<K, Entry<V>>,
    max_bytes: usize,
    max_entries: usize,
    used: usize,
    clock: u64,
}

impl<K: Eq + Hash + Clone, V> RenderCache<K, V> {
    pub(crate) fn new(max_bytes: usize, max_entries: usize) -> Self {
        Self { entries: HashMap::new(), max_bytes, max_entries, used: 0, clock: 0 }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        let entry = self.entries.get_mut(key)?;
        self.clock = self.clock.saturating_add(1);
        entry.touched = self.clock;
        Some(&entry.value)
    }

    /// Reject an individually oversized value before disturbing useful entries.
    pub(crate) fn insert(&mut self, key: K, value: V, bytes: usize) -> bool {
        self.insert_with_eviction(key, value, bytes, |_| {})
    }

    /// Let resource owners release external storage when an entry is replaced
    /// or evicted. Accounting and recency still follow the same cache policy.
    pub(crate) fn insert_with_eviction(
        &mut self,
        key: K,
        value: V,
        bytes: usize,
        mut evict: impl FnMut(V),
    ) -> bool {
        let bytes = bytes.max(1);
        if bytes > self.max_bytes || self.max_entries == 0 {
            return false;
        }
        if let Some(value) = self.take(&key) {
            evict(value);
        }
        while self.entries.len() >= self.max_entries
            || self.used.saturating_add(bytes) > self.max_bytes
        {
            let oldest = self.entries.iter().min_by_key(|(_, entry)| entry.touched);
            let Some(key) = oldest.map(|(key, _)| key.clone()) else { break };
            if let Some(value) = self.take(&key) {
                evict(value);
            }
        }
        self.clock = self.clock.saturating_add(1);
        self.used += bytes;
        self.entries.insert(key, Entry { value, bytes, touched: self.clock });
        true
    }

    pub(crate) fn remove(&mut self, key: &K) {
        self.take(key);
    }

    fn take(&mut self, key: &K) -> Option<V> {
        let entry = self.entries.remove(key)?;
        self.used -= entry.bytes;
        Some(entry.value)
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = V> + '_ {
        self.used = 0;
        self.entries.drain().map(|(_, entry)| entry.value)
    }

    /// Trim cold entries to a lower working budget without evicting resources
    /// that the caller is using in the current frame.
    pub(crate) fn trim_with_eviction(
        &mut self,
        bytes: usize,
        keep: impl Fn(&K) -> bool,
        mut evict: impl FnMut(V),
    ) {
        while self.used > bytes {
            let oldest = self
                .entries
                .iter()
                .filter(|(key, _)| !keep(key))
                .min_by_key(|(_, entry)| entry.touched);
            let Some(key) = oldest.map(|(key, _)| key.clone()) else { break };
            if let Some(value) = self.take(&key) {
                evict(value);
            }
        }
    }

    /// Reserve part of the owner's budget before background allocation begins.
    /// Growing the allowance does not eagerly allocate or resurrect evicted data.
    pub(crate) fn set_byte_budget(&mut self, bytes: usize) {
        self.set_byte_budget_with_eviction(bytes, |_| {});
    }

    pub(crate) fn set_byte_budget_with_eviction(&mut self, bytes: usize, evict: impl FnMut(V)) {
        self.max_bytes = bytes;
        self.trim_with_eviction(bytes, |_| false, evict);
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn used_bytes(&self) -> usize {
        self.used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_resources_are_released_on_eviction_replacement_and_drain() {
        let mut cache = RenderCache::new(30, 3);
        let mut released = Vec::new();
        cache.insert(1, "first", 10);
        cache.insert(2, "second", 10);
        assert_eq!(cache.get(&1), Some(&"first"));
        assert!(cache.insert_with_eviction(3, "third", 20, |value| released.push(value)));
        assert_eq!(released, ["second"]);
        assert!(cache.insert_with_eviction(1, "replacement", 5, |value| released.push(value)));
        assert_eq!(released, ["second", "first"]);
        assert!(!cache.insert_with_eviction(4, "oversized", 31, |value| released.push(value)));
        assert_eq!(released.len(), 2);
        let remaining: Vec<_> = cache.drain().collect();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.contains(&"third") && remaining.contains(&"replacement"));
        assert_eq!((cache.len(), cache.used_bytes()), (0, 0));
    }

    #[test]
    fn trimming_preserves_visible_resources_and_then_releases_them_when_cold() {
        let mut cache = RenderCache::new(100, 8);
        cache.insert("visible", 1, 60);
        cache.insert("cold", 2, 20);
        let mut released = Vec::new();
        cache.trim_with_eviction(10, |key| *key == "visible", |value| released.push(value));
        assert_eq!(released, [2]);
        assert_eq!(cache.used_bytes(), 60);
        cache.trim_with_eviction(10, |_| false, |value| released.push(value));
        assert_eq!(released, [2, 1]);
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn reservations_release_cold_resources_before_allocation() {
        let mut cache = RenderCache::new(100, 8);
        cache.insert("cold", 1, 40);
        cache.insert("hot", 2, 40);
        cache.set_byte_budget(50);
        assert_eq!(cache.get(&"cold"), None);
        assert_eq!(cache.get(&"hot"), Some(&2));
        assert!(!cache.insert("large", 3, 51));
        cache.set_byte_budget(100);
        assert_eq!(cache.used_bytes(), 40);
        cache.set_byte_budget(0);
        assert_eq!(cache.len(), 0);
        assert!(!cache.insert("failure", 4, 1));
    }

    #[test]
    fn reducing_the_budget_returns_external_resources_to_the_owner() {
        let mut cache = RenderCache::new(100, 8);
        cache.insert("cold", 1, 40);
        cache.insert("hot", 2, 40);
        let mut released = Vec::new();
        cache.set_byte_budget_with_eviction(50, |value| released.push(value));
        assert_eq!(released, [1]);
        assert_eq!(cache.get(&"hot"), Some(&2));
        cache.set_byte_budget_with_eviction(100, |value| released.push(value));
        assert_eq!(released, [1]);
        cache.set_byte_budget_with_eviction(0, |value| released.push(value));
        assert_eq!(released, [1, 2]);
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn scrolling_evicts_one_cold_resource_and_keeps_hot_resources() {
        let mut cache = RenderCache::new(64, 4);
        for key in 0..4 {
            cache.insert(key, key, 16);
        }
        assert_eq!(cache.get(&0), Some(&0));
        cache.insert(4, 4, 16);
        assert_eq!(cache.len(), 4);
        assert_eq!(cache.get(&0), Some(&0));
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&2));
        assert_eq!(cache.used_bytes(), 64);
    }

    #[test]
    fn resize_replacements_and_oversized_images_preserve_accounting() {
        let mut cache = RenderCache::new(100, 8);
        cache.insert("formula", 1, 40);
        cache.insert("molecule", 2, 40);
        assert!(!cache.insert("too large", 3, 101));
        assert_eq!(cache.len(), 2);
        cache.insert("formula", 4, 10);
        assert_eq!(cache.used_bytes(), 50);
        cache.remove(&"molecule");
        assert_eq!(cache.used_bytes(), 10);
    }

    #[test]
    fn eighty_logical_sessions_share_one_budget_during_repeated_resize() {
        let mut cache = RenderCache::new(32 * 1024, 128);
        for resize in 0..40 {
            for session in 0..80 {
                cache.insert((session, resize), session, 1024);
                assert!(cache.used_bytes() <= 32 * 1024);
                assert!(cache.len() <= 32);
            }
        }
        assert_eq!(cache.get(&(79, 39)), Some(&79));
        assert_eq!(cache.get(&(0, 0)), None);
    }
}
