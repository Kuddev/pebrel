//! 提示符目录选择器：单击当前提示符上的目录后，列出上一级、子目录和常用
//! 目录；选中项回到发起的 pane 里 `cd`，而不是像 Quick Jump 那样新开标签。
//!
//! Overlay、筛选和键盘复用命令面板，这里只负责行投影与执行。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::App;
use gpui_component::IconNamed as _;

use crate::i18n::Message;

use super::*;

const CHILD_LIMIT: usize = 300;
const FREQUENT_LIMIT: usize = 40;

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn row(
    pane: gpui::EntityId,
    group_order: usize,
    group: &str,
    label: String,
    path: PathBuf,
    icon: IconName,
) -> WorkspacePaletteRow {
    let full = path.display().to_string();
    WorkspacePaletteRow {
        group_order,
        group: group.to_owned(),
        search: format!("{label} {full}").to_lowercase(),
        label,
        hint: full,
        hint_style: WorkspacePaletteHintStyle::Metadata,
        action: WorkspacePaletteAction::ChangeDirectory { pane, path },
        icon: None,
        icon_glyph: None,
        icon_path: Some(icon.path()),
    }
}

/// 子目录按名字不分大小写排序；读不了的目录（权限、已删除）就是没有子项。
fn child_directories(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut children: Vec<PathBuf> =
        entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    children.sort_by_cached_key(|path| display_name(path).to_lowercase());
    children.truncate(CHILD_LIMIT);
    children
}

pub(super) fn rows(
    pane: gpui::EntityId,
    dir: &Path,
    cwd: Option<&Path>,
    language: crate::display::UiLanguage,
) -> Vec<WorkspacePaletteRow> {
    let here = language.text(Message::DirectoryJumpHere);
    let mut rows = Vec::new();
    let mut seen: HashSet<PathBuf> = cwd.map(Path::to_path_buf).into_iter().collect();
    if seen.insert(dir.to_path_buf()) {
        rows.push(row(pane, 0, here, display_name(dir), dir.to_path_buf(), IconName::FolderOpen));
    }
    if let Some(parent) = dir.parent().filter(|parent| seen.insert(parent.to_path_buf())) {
        let label = format!(".. · {}", display_name(parent));
        rows.push(row(pane, 0, here, label, parent.to_path_buf(), IconName::ArrowUp));
    }
    let children = language.text(Message::DirectoryJumpSubfolders);
    for child in child_directories(dir) {
        seen.insert(child.clone());
        rows.push(row(pane, 1, children, display_name(&child), child, IconName::Folder));
    }
    let frequent = language.text(Message::DirectoryJumpFrequent);
    for path in crate::directory_history::global().search("", FREQUENT_LIMIT) {
        if seen.insert(path.clone()) {
            rows.push(row(pane, 2, frequent, display_name(&path), path, IconName::FolderOpen));
        }
    }
    rows
}

impl NebulaWorkspace {
    pub(super) fn open_directory_jump(
        &mut self,
        view: &Entity<TerminalView>,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = workspace_ui_language();
        let cwd = view.read(cx).local_cwd();
        self.command_manager_open = false;
        self.shell_picker_open = false;
        self.launcher_filter = crate::display::command_palette::LauncherFilter::All;
        self.quick_jump_filter = None;
        self.palette_override = Some(rows(view.entity_id(), &dir, cwd.as_deref(), language));
        self.command_palette_open = true;
        self.command_palette_selected = 0;
        self.reset_palette_query(language.text(Message::DirectoryJumpPlaceholder), window, cx);
        cx.notify();
    }

    fn terminal_view(&self, entity_id: gpui::EntityId, _: &App) -> Option<Entity<TerminalView>> {
        self.tabs.iter().find_map(|tab| match tab {
            WorkspaceTab::Terminal { panes, .. } => panes
                .iter()
                .find(|pane| pane.view.entity_id() == entity_id)
                .map(|pane| pane.view.clone()),
            _ => None,
        })
    }

    pub(super) fn run_directory_jump(
        &mut self,
        pane: gpui::EntityId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette_state();
        if let Some(view) = self.terminal_view(pane, cx) {
            view.update(cx, |view, cx| view.change_directory(&path, cx));
        }
        self.focus_active(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(rows: &[WorkspacePaletteRow]) -> Vec<(usize, String)> {
        rows.iter().map(|row| (row.group_order, row.label.clone())).collect()
    }

    #[test]
    fn picker_lists_parent_then_sorted_children_and_skips_the_current_directory() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("work");
        for child in ["beta", "Alpha", "项目"] {
            std::fs::create_dir_all(dir.join(child)).unwrap();
        }
        std::fs::write(dir.join("file.txt"), "").unwrap();
        let pane = gpui::EntityId::from(1u64);
        let rows = rows(pane, &dir, Some(&dir), crate::display::UiLanguage::ZhCn);
        let mut listed = labels(&rows);
        listed.retain(|(group, _)| *group < 2);
        let parent = format!(".. · {}", display_name(root.path()));
        assert_eq!(
            listed,
            vec![(0, parent), (1, "Alpha".into()), (1, "beta".into()), (1, "项目".into())]
        );
        assert!(rows.iter().all(|row| matches!(
            &row.action,
            WorkspacePaletteAction::ChangeDirectory { pane: target, .. } if *target == pane
        )));
    }

    #[test]
    fn a_clicked_directory_other_than_cwd_is_offered_first() {
        let root = tempfile::tempdir().unwrap();
        let rows = rows(
            gpui::EntityId::from(1u64),
            root.path(),
            Some(Path::new(r"C:\elsewhere")),
            crate::display::UiLanguage::ZhCn,
        );
        assert_eq!(rows[0].label, display_name(root.path()));
        assert_eq!(rows[0].group_order, 0);
    }
}
