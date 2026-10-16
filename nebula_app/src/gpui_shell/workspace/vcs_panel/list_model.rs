//! Stable presentation data derived once per repository snapshot or selected view.
use super::*;
use crate::display::side_panel::{GitCommit, GitInfo, GitPanelView, VcsKind};
use std::collections::HashSet;

mod tree;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ChangeLayout {
    #[default]
    List,
    Tree,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum RowOps {
    Unstaged,
    Staged,
    Conflict,
    Svn,
}

#[derive(Clone)]
pub(super) enum VcsRow {
    Heading {
        section_id: &'static str,
        message: Option<Message>,
        count: usize,
    },
    Empty(Message),
    History {
        index: usize,
        commit: GitCommit,
        graph: GitGraphRow,
    },
    Change {
        section_id: &'static str,
        index: usize,
        status: char,
        relative_path: String,
        ops: RowOps,
        depth: usize,
    },
    Directory {
        section_id: &'static str,
        relative_path: String,
        depth: usize,
        count: usize,
        expanded: bool,
    },
}

pub(in crate::gpui_shell::workspace) struct VcsList {
    snapshot: Option<Arc<GitInfo>>,
    #[cfg(test)]
    pub(super) rendered: Vec<usize>,
    view: Option<GitPanelView>,
    root: Option<PathBuf>,
    rows: Vec<VcsRow>,
    flat_rows: Vec<VcsRow>,
    tree_rows: Vec<VcsRow>,
    layout: ChangeLayout,
    collapsed: HashSet<(&'static str, String)>,
    lane_layout: (f32, f32),
    pub(super) scroll: gpui::ListState,
}

impl Default for VcsList {
    fn default() -> Self {
        Self {
            snapshot: None,
            #[cfg(test)]
            rendered: Vec::new(),
            view: None,
            root: None,
            rows: Vec::new(),
            flat_rows: Vec::new(),
            tree_rows: Vec::new(),
            layout: ChangeLayout::List,
            collapsed: HashSet::new(),
            lane_layout: (0.0, 0.0),
            scroll: gpui::ListState::new(0, gpui::ListAlignment::Top, px(34.0)),
        }
    }
}

impl VcsList {
    pub(super) fn sync(
        &mut self,
        snapshot: Option<Arc<GitInfo>>,
        view: GitPanelView,
        root: Option<PathBuf>,
    ) {
        let same_snapshot = match (&self.snapshot, &snapshot) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        let same_location = self.view == Some(view) && self.root == root;
        if same_snapshot && same_location {
            return;
        }
        let anchor = self.scroll_anchor();
        if self.root != root {
            self.collapsed.clear();
        }
        self.snapshot = snapshot;
        self.view = Some(view);
        self.root = root;
        self.flat_rows.clear();
        if let Some(info) = self.snapshot.as_ref() {
            if info.vcs == VcsKind::Git && view == GitPanelView::History {
                let graphs = git_graph_rows(&info.history);
                self.lane_layout = git_lane_layout(&graphs);
                self.flat_rows.extend(
                    info.history
                        .iter()
                        .cloned()
                        .zip(graphs)
                        .enumerate()
                        .map(|(index, (commit, graph))| VcsRow::History { index, commit, graph }),
                );
                if self.flat_rows.is_empty() {
                    self.flat_rows.push(VcsRow::Empty(Message::VcsNoHistory));
                }
            } else {
                let conflicts: std::collections::HashSet<_> =
                    info.conflicts.iter().map(|(_, p)| p.as_str()).collect();
                let mut section =
                    |id, message, entries: &[(char, String)], ops, exclude_conflicts: bool| {
                        let changes: Vec<_> = entries
                            .iter()
                            .enumerate()
                            .filter(|(_, (_, p))| {
                                !exclude_conflicts || !conflicts.contains(p.as_str())
                            })
                            .map(|(index, (status, path))| VcsRow::Change {
                                section_id: id,
                                index,
                                status: *status,
                                relative_path: path.clone(),
                                ops,
                                depth: 0,
                            })
                            .collect();
                        if !changes.is_empty() {
                            self.flat_rows.push(VcsRow::Heading {
                                section_id: id,
                                message,
                                count: changes.len(),
                            });
                            self.flat_rows.extend(changes);
                        }
                    };
                let is_git = info.vcs == VcsKind::Git;
                section(
                    "conflicts",
                    Some(Message::VcsMergeConflicts),
                    &info.conflicts,
                    if is_git { RowOps::Conflict } else { RowOps::Svn },
                    false,
                );
                match info.vcs {
                    VcsKind::Git if view == GitPanelView::Changes => {
                        section(
                            "staged",
                            Some(Message::VcsStaged),
                            &info.staged,
                            RowOps::Staged,
                            true,
                        );
                        section(
                            "changes",
                            Some(Message::VcsChanges),
                            &info.unstaged,
                            RowOps::Unstaged,
                            true,
                        );
                    },
                    VcsKind::Svn => section("svn", None, &info.unstaged, RowOps::Svn, true),
                    _ => {},
                }
                if self.flat_rows.is_empty() && info.vcs != VcsKind::SvnRepository {
                    self.flat_rows.push(VcsRow::Empty(
                        if is_git && view == GitPanelView::Conflicts {
                            Message::VcsNoConflicts
                        } else {
                            Message::VcsNoChanges
                        },
                    ));
                }
            }
        }
        self.tree_rows = tree::project(&self.flat_rows);
        self.prune_collapsed();
        self.rebuild_visible(if same_location { anchor } else { None });
    }

    pub(super) fn row(&self, index: usize) -> Option<&VcsRow> {
        self.rows.get(index)
    }
    pub(super) fn root(&self) -> Option<&PathBuf> {
        self.root.as_ref()
    }
    pub(super) fn lane_layout(&self) -> (f32, f32) {
        self.lane_layout
    }

    pub(super) fn layout(&self) -> ChangeLayout {
        self.layout
    }

    pub(super) fn tree_active(&self) -> bool {
        self.layout == ChangeLayout::Tree && self.can_choose_layout()
    }

    pub(super) fn can_choose_layout(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|info| info.vcs == VcsKind::Git)
            && self.view != Some(GitPanelView::History)
    }

    pub(super) fn set_layout(&mut self, layout: ChangeLayout) {
        if self.layout != layout {
            let anchor = self.scroll_anchor();
            self.layout = layout;
            self.rebuild_visible(anchor);
        }
    }

    pub(super) fn toggle_directory(&mut self, section_id: &'static str, path: &str) {
        if !self.tree_active()
            || !self.tree_rows.iter().any(|row| {
                matches!(row, VcsRow::Directory { section_id: id, relative_path, .. }
                if *id == section_id && relative_path == path)
            })
        {
            return;
        }
        let anchor = self.scroll_anchor();
        let key = (section_id, path.to_owned());
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        self.rebuild_visible(anchor);
    }

    fn prune_collapsed(&mut self) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            self.collapsed.clear();
            return;
        };
        self.collapsed.retain(|(section, folder)| {
            let entries = match *section {
                "conflicts" => &snapshot.conflicts,
                "staged" => &snapshot.staged,
                "changes" => &snapshot.unstaged,
                _ => return false,
            };
            entries.iter().any(|(_, path)| {
                path.strip_prefix(folder.as_str()).is_some_and(|suffix| suffix.starts_with('/'))
            })
        });
    }
}
