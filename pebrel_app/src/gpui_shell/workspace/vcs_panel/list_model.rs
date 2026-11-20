//! Stable presentation data derived once per repository snapshot or selected view.
use super::*;
use crate::display::side_panel::{GitCommit, GitInfo, GitPanelView, VcsKind};

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
    },
}

pub(in crate::gpui_shell::workspace) struct VcsList {
    snapshot: Option<Arc<GitInfo>>,
    #[cfg(test)]
    pub(super) rendered: Vec<usize>,
    view: Option<GitPanelView>,
    root: Option<PathBuf>,
    rows: Vec<VcsRow>,
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
        self.snapshot = snapshot;
        self.view = Some(view);
        self.root = root;
        self.rows.clear();
        if let Some(info) = self.snapshot.as_ref() {
            if info.vcs == VcsKind::Git && view == GitPanelView::History {
                let graphs = git_graph_rows(&info.history);
                self.lane_layout = git_lane_layout(&graphs);
                self.rows.extend(
                    info.history
                        .iter()
                        .cloned()
                        .zip(graphs)
                        .enumerate()
                        .map(|(index, (commit, graph))| VcsRow::History { index, commit, graph }),
                );
                if self.rows.is_empty() {
                    self.rows.push(VcsRow::Empty(Message::VcsNoHistory));
                }
            } else {
                let conflicts: std::collections::HashSet<_> =
                    info.conflicts.iter().map(|(_, p)| p.as_str()).collect();
                let mut section =
                    |id, message, entries: &[(char, String)], ops, exclude_conflicts: bool| {
                        let changes: Vec<_> = entries
                            .iter()
                            .filter(|(_, p)| !exclude_conflicts || !conflicts.contains(p.as_str()))
                            .enumerate()
                            .map(|(index, (status, path))| VcsRow::Change {
                                section_id: id,
                                index,
                                status: *status,
                                relative_path: path.clone(),
                                ops,
                            })
                            .collect();
                        if !changes.is_empty() {
                            self.rows.push(VcsRow::Heading { message, count: changes.len() });
                            self.rows.extend(changes);
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
                if self.rows.is_empty() && info.vcs != VcsKind::SvnRepository {
                    self.rows.push(VcsRow::Empty(if is_git && view == GitPanelView::Conflicts {
                        Message::VcsNoConflicts
                    } else {
                        Message::VcsNoChanges
                    }));
                }
            }
        }
        if same_location {
            let offset = self.scroll.logical_scroll_top();
            self.scroll.splice(0..self.scroll.item_count(), self.rows.len());
            self.scroll.scroll_to(offset);
        } else {
            self.scroll.reset(self.rows.len());
        }
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
}
