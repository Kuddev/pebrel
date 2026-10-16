//! Snapshot-only hierarchy and scroll anchors; no filesystem reads or row rendering.
use super::{VcsList, VcsRow};
use std::collections::BTreeMap;

pub(super) fn project(flat: &[VcsRow]) -> Vec<VcsRow> {
    let mut output = Vec::new();
    let mut start = 0;
    while start < flat.len() {
        let VcsRow::Heading { section_id, .. } = &flat[start] else {
            output.push(flat[start].clone());
            start += 1;
            continue;
        };
        output.push(flat[start].clone());
        let end = (start + 1..flat.len())
            .find(|ix| !matches!(flat[*ix], VcsRow::Change { .. }))
            .unwrap_or(flat.len());
        let mut folders = BTreeMap::<String, (usize, usize)>::new();
        let mut entries = Vec::new();
        for row in &flat[start + 1..end] {
            let VcsRow::Change { relative_path, .. } = row else { continue };
            for (depth, (position, _)) in relative_path.match_indices('/').enumerate() {
                let folder = &relative_path[..position];
                let entry = folders.entry(folder.to_owned()).or_insert((depth, 0));
                entry.1 += 1;
            }
            let mut file = row.clone();
            if let VcsRow::Change { depth, .. } = &mut file {
                *depth = relative_path.matches('/').count();
            }
            entries.push((sort_key(relative_path, false), file));
        }
        for (relative_path, (depth, count)) in folders {
            entries.push((
                sort_key(&relative_path, true),
                VcsRow::Directory { section_id, relative_path, depth, count, expanded: true },
            ));
        }
        // Components, rather than whole path strings, keep each directory's
        // descendants together and put directories before files at every level.
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        output.extend(entries.into_iter().map(|(_, row)| row));
        start = end;
    }
    output
}

fn sort_key(path: &str, directory: bool) -> Vec<(bool, String)> {
    let mut key: Vec<_> = path.split('/').map(|part| (false, part.to_owned())).collect();
    if !directory && let Some(last) = key.last_mut() {
        last.0 = true;
    }
    key
}

#[derive(Clone, PartialEq)]
enum Identity {
    Heading(&'static str),
    Path(&'static str, String, bool),
    History(String),
}

fn identity(row: &VcsRow) -> Option<Identity> {
    match row {
        VcsRow::Heading { section_id, .. } => Some(Identity::Heading(section_id)),
        VcsRow::Change { section_id, relative_path, .. } => {
            Some(Identity::Path(section_id, relative_path.clone(), false))
        },
        VcsRow::Directory { section_id, relative_path, .. } => {
            Some(Identity::Path(section_id, relative_path.clone(), true))
        },
        VcsRow::History { commit, .. } => Some(Identity::History(commit.full_hash.clone())),
        VcsRow::Empty(_) => None,
    }
}

pub(super) struct ScrollAnchor {
    identity: Identity,
    offset: gpui::ListOffset,
}

impl VcsList {
    pub(super) fn scroll_anchor(&self) -> Option<ScrollAnchor> {
        let offset = self.scroll.logical_scroll_top();
        let identity = identity(self.rows.get(offset.item_ix)?)?;
        Some(ScrollAnchor { identity, offset })
    }

    pub(super) fn rebuild_visible(&mut self, anchor: Option<ScrollAnchor>) {
        self.rows.clear();
        if self.tree_active() {
            let mut hidden: Option<(&str, &str)> = None;
            for row in &self.tree_rows {
                let location = match row {
                    VcsRow::Directory { section_id, relative_path, .. }
                    | VcsRow::Change { section_id, relative_path, .. } => {
                        Some((*section_id, relative_path.as_str()))
                    },
                    _ => None,
                };
                if let (Some((section, folder)), Some((next_section, path))) = (hidden, location)
                    && section == next_section
                    && path.strip_prefix(folder).is_some_and(|suffix| suffix.starts_with('/'))
                {
                    continue;
                }
                hidden = None;
                let mut row = row.clone();
                if let VcsRow::Directory { section_id, relative_path, expanded, .. } = &mut row {
                    *expanded = !self.collapsed.contains(&(*section_id, relative_path.clone()));
                    if !*expanded {
                        hidden = location;
                    }
                }
                self.rows.push(row);
            }
        } else {
            self.rows.extend(self.flat_rows.iter().cloned());
        }
        if let Some(mut anchor) = anchor {
            let mut target =
                self.rows.iter().position(|row| identity(row) == Some(anchor.identity.clone()));
            if target.is_none()
                && let Identity::Path(section, mut path, _) = anchor.identity
            {
                while let Some((parent, _)) = path.rsplit_once('/') {
                    path = parent.to_owned();
                    target = self.rows.iter().position(|row| {
                        matches!(row, VcsRow::Directory { section_id, relative_path, .. }
                            if *section_id == section && relative_path == &path)
                    });
                    if target.is_some() {
                        break;
                    }
                }
            }
            anchor.offset.item_ix =
                target.unwrap_or(anchor.offset.item_ix).min(self.rows.len().saturating_sub(1));
            self.scroll.splice(0..self.scroll.item_count(), self.rows.len());
            self.scroll.scroll_to(anchor.offset);
        } else {
            self.scroll.reset(self.rows.len());
        }
    }
}
