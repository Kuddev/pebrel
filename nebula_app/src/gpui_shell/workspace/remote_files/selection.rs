//! Path-based selection, including the deferred click that preserves a group while dragging.

use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub(super) struct Selection {
    selected: HashSet<String>,
    anchor: Option<String>,
    focus: Option<String>,
    pending_collapse: Option<String>,
}

impl Selection {
    pub(super) fn contains(&self, path: &str) -> bool {
        self.selected.contains(path)
    }

    pub(super) fn focus(&self) -> Option<&str> {
        self.focus.as_deref()
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn select_only(&mut self, path: &str) {
        self.selected.clear();
        self.selected.insert(path.to_owned());
        self.anchor = Some(path.to_owned());
        self.focus = Some(path.to_owned());
        self.pending_collapse = None;
    }

    pub(super) fn select_all(&mut self, paths: &[&str]) {
        self.selected = paths.iter().map(|path| (*path).to_owned()).collect();
        if self.focus.as_deref().is_none_or(|path| !paths.contains(&path)) {
            self.focus = paths.first().map(|path| (*path).to_owned());
        }
        if self.anchor.as_deref().is_none_or(|path| !paths.contains(&path)) {
            self.anchor = self.focus.clone();
        }
        self.pending_collapse = None;
    }

    pub(super) fn press(&mut self, paths: &[&str], index: usize, toggle: bool, range: bool) {
        let Some(path) = paths.get(index).copied() else { return };
        self.pending_collapse = None;
        if range {
            self.extend_to(paths, index, toggle);
        } else if toggle {
            self.focus = Some(path.to_owned());
            self.toggle_focus();
        } else if self.contains(path) {
            self.focus = Some(path.to_owned());
            self.pending_collapse = Some(path.to_owned());
        } else {
            self.select_only(path);
        }
    }

    pub(super) fn click(&mut self, path: &str) {
        if self.pending_collapse.as_deref() == Some(path) {
            self.select_only(path);
        }
    }

    pub(super) fn drag_started(&mut self) {
        self.pending_collapse = None;
    }

    pub(super) fn step(&mut self, paths: &[&str], delta: isize, toggle: bool, range: bool) {
        if paths.is_empty() {
            return;
        }
        self.pending_collapse = None;
        let current = self.focus.as_deref().and_then(|path| paths.iter().position(|p| *p == path));
        let next = match current {
            Some(index) => index.saturating_add_signed(delta).min(paths.len() - 1),
            None if delta <= -(paths.len() as isize) => 0,
            None if delta >= paths.len() as isize => paths.len() - 1,
            None if delta < 0 => paths.len() - 1,
            None => 0,
        };
        if range {
            if self.anchor.is_none() {
                self.anchor = Some(paths[current.unwrap_or(0)].to_owned());
            }
            self.extend_to(paths, next, toggle);
        } else if toggle {
            self.focus = Some(paths[next].to_owned());
        } else {
            self.select_only(paths[next]);
        }
    }

    pub(super) fn toggle_focus(&mut self) {
        let Some(path) = self.focus.clone() else { return };
        if !self.selected.remove(&path) {
            self.selected.insert(path.clone());
        }
        self.anchor = Some(path);
        self.pending_collapse = None;
    }

    pub(super) fn retain(&mut self, paths: &[&str]) {
        let present: HashSet<&str> = paths.iter().copied().collect();
        self.selected.retain(|path| present.contains(path.as_str()));
        for slot in [&mut self.anchor, &mut self.focus, &mut self.pending_collapse] {
            if slot.as_deref().is_some_and(|path| !present.contains(path)) {
                *slot = None;
            }
        }
    }

    fn extend_to(&mut self, paths: &[&str], index: usize, keep: bool) {
        let anchor = self.anchor.as_deref().and_then(|path| paths.iter().position(|p| *p == path));
        let anchor = anchor.unwrap_or(index);
        if !keep {
            self.selected.clear();
        }
        self.selected.extend(
            paths[anchor.min(index)..=anchor.max(index)].iter().map(|path| (*path).to_owned()),
        );
        self.anchor = Some(paths[anchor].to_owned());
        self.focus = Some(paths[index].to_owned());
    }
}
