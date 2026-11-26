//! 拖到标签栏的外部路径：每个文件夹一个标签，其余项忽略并提示。
//!
//! 终端区域有自己的落点（粘贴路径、SSH 上传），实现在
//! `terminal::view::path_drop`；这里只负责标签栏这一侧，两者互不影响。

use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use crate::gpui_shell::toast::{self, ToastKind};
use crate::i18n::Message;

use super::NebulaWorkspace;

/// 把一次拖入的路径分成「可打开的目录」与「忽略的数量」。
///
/// 只认磁盘上真实存在的目录：普通文件、失效路径和同一批里的重复项都算忽略。
/// 这样混进一个文件也不会让整批拖入失败——用户拖的是文件夹时不该被旁边的
/// 一个文件取消。
pub(super) fn dropped_directories(paths: &[PathBuf]) -> (Vec<PathBuf>, usize) {
    let mut directories: Vec<PathBuf> = Vec::new();
    let mut ignored = 0usize;
    for path in paths {
        if is_directory(path) && !directories.contains(path) {
            directories.push(path.clone());
        } else {
            ignored += 1;
        }
    }
    (directories, ignored)
}

fn is_directory(path: &Path) -> bool {
    path.is_dir()
}

impl NebulaWorkspace {
    /// 标签栏落点：每个文件夹开一个标签，`add_terminal_at` 会把新标签设为活动
    /// 标签，所以最后一个文件夹自然拿到焦点。
    pub(super) fn open_dropped_directories(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (directories, ignored) = dropped_directories(paths);
        let language = crate::gpui_shell::config::ui_language(cx);
        if directories.is_empty() {
            toast::toast(
                window,
                cx,
                ToastKind::Warning,
                language.text(Message::CommonDropDirectoriesEmpty),
            );
            return;
        }
        // 打开是有落点的：先开标签，再用一条提示交代被忽略的部分，用户的
        // 显式动作不会被静默改写。
        for directory in directories {
            self.add_terminal_at(Some(directory), None, window, cx);
        }
        if ignored > 0 {
            let count = ignored.to_string();
            let text =
                language.format(Message::CommonDropDirectoriesIgnored, &[("ignored", &count)]);
            toast::toast(window, cx, ToastKind::Warning, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_directories_become_tabs_and_repeats_collapse() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let file = root.path().join("notes.txt");
        std::fs::write(&file, b"x").unwrap();
        let missing = root.path().join("gone");

        let (directories, ignored) =
            dropped_directories(&[file.clone(), first.clone(), second.clone(), first.clone(), missing]);
        assert_eq!(directories, vec![first, second]);
        assert_eq!(ignored, 3, "one file, one missing path, one repeat");
    }

    #[test]
    fn a_drop_without_any_directory_opens_nothing() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("notes.txt");
        std::fs::write(&file, b"x").unwrap();
        let (directories, ignored) = dropped_directories(&[file]);
        assert!(directories.is_empty());
        assert_eq!(ignored, 1);
    }
}
