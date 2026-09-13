use std::path::PathBuf;

use crate::i18n::t;

#[cfg(feature = "legacy-shell")]
use super::window::Window;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "file_dialog/desktop.rs"]
mod platform;
#[cfg(windows)]
#[path = "file_dialog/windows.rs"]
mod platform;
#[cfg(windows)]
#[path = "file_dialog/folder.rs"]
mod windows_folder;

#[derive(Debug, Clone, Copy)]
struct FileFilter {
    name: &'static str,
    #[cfg_attr(windows, allow(dead_code))]
    extensions: &'static [&'static str],
    #[cfg_attr(not(windows), allow(dead_code))]
    patterns: &'static [&'static str],
}

const ALL_FILES_FILTER: FileFilter =
    FileFilter { name: "file_dialog.filter.all_files", extensions: &["*"], patterns: &["*.*"] };
const IMAGE_FILTERS: &[FileFilter] = &[
    FileFilter {
        name: "file_dialog.filter.images",
        extensions: &["png", "jpg", "jpeg", "webp", "bmp"],
        patterns: &["*.png", "*.jpg", "*.jpeg", "*.webp", "*.bmp"],
    },
    ALL_FILES_FILTER,
];
const FONT_FILTERS: &[FileFilter] = &[
    FileFilter {
        name: "file_dialog.filter.fonts",
        extensions: &["ttf", "otf", "ttc", "otc"],
        patterns: &["*.ttf", "*.otf", "*.ttc", "*.otc"],
    },
    ALL_FILES_FILTER,
];
const PRIVATE_KEY_FILTERS: &[FileFilter] = &[
    FileFilter {
        name: "file_dialog.filter.private_keys",
        extensions: &["pem", "key", "ppk"],
        patterns: &["id_*", "*.pem", "*.key", "*.ppk"],
    },
    ALL_FILES_FILTER,
];
const WORKSPACE_FILTERS: &[FileFilter] = &[
    FileFilter {
        name: "file_dialog.filter.workspaces",
        extensions: &["json"],
        patterns: &["*.nebula-workspace.json", "*.json"],
    },
    ALL_FILES_FILTER,
];
const BACKUP_FILTERS: &[FileFilter] = &[
    FileFilter {
        name: "file_dialog.filter.backups",
        extensions: &["nebula-backup"],
        patterns: &["*.nebula-backup"],
    },
    ALL_FILES_FILTER,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrivateKeyFileKind {
    PrivateKey,
    PublicKey,
    Unsupported,
}

pub(super) fn classify_private_key_contents(contents: &[u8]) -> PrivateKeyFileKind {
    let Ok(text) = std::str::from_utf8(contents) else {
        return PrivateKeyFileKind::Unsupported;
    };
    let trimmed = text.trim_start();
    if trimmed.starts_with("ssh-")
        || trimmed.starts_with("ecdsa-sha2-")
        || trimmed.starts_with("sk-ssh-")
        || trimmed.starts_with("sk-ecdsa-")
        || russh::keys::ssh_key::PublicKey::from_openssh(trimmed).is_ok()
    {
        return PrivateKeyFileKind::PublicKey;
    }
    if [
        concat!("-----BEGIN OPENSSH ", "PRIVATE KEY-----"),
        concat!("-----BEGIN RSA ", "PRIVATE KEY-----"),
        concat!("-----BEGIN EC ", "PRIVATE KEY-----"),
        concat!("-----BEGIN ENCRYPTED ", "PRIVATE KEY-----"),
        concat!("-----BEGIN ", "PRIVATE KEY-----"),
        concat!("PuTTY-User-Key-", "File-"),
    ]
    .iter()
    .any(|header| trimmed.starts_with(header))
    {
        PrivateKeyFileKind::PrivateKey
    } else {
        PrivateKeyFileKind::Unsupported
    }
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_image_file(owner: &Window) -> Option<String> {
    platform::pick_file(owner, &t!("file_dialog.choose_background_image"), IMAGE_FILTERS)
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_font_file(owner: &Window) -> Option<PathBuf> {
    platform::pick_file(owner, &t!("file_dialog.import_terminal_font"), FONT_FILTERS)
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_private_key_file(owner: &Window) -> Option<Result<PathBuf, String>> {
    let path = platform::pick_file(owner, &t!("file_dialog.choose_private_key"), PRIVATE_KEY_FILTERS)?;
    Some(validate_private_key_path(&path))
}

/// GPUI 添加主机面板用：同一套 `pem` / `key` / `ppk` / `id_*` 过滤器，
/// 不依赖 winit `Window`。Windows 上应优先走 [`pick_private_key_file_with_hwnd`]。
#[cfg(not(windows))]
pub(crate) fn pick_private_key_file_unowned() -> Option<Result<PathBuf, String>> {
    let path = platform::pick_file_unowned(&t!("file_dialog.choose_private_key"), PRIVATE_KEY_FILTERS)?;
    Some(validate_private_key_path(&path))
}

/// 把系统文件对话框挂到 GPUI 窗口上，过滤规则与旧壳 `pick_private_key_file` 相同。
#[cfg(windows)]
pub(crate) fn pick_private_key_file_with_hwnd(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> Option<Result<PathBuf, String>> {
    let path = platform::pick_file_with_hwnd(hwnd, &t!("file_dialog.choose_private_key"), PRIVATE_KEY_FILTERS)?;
    Some(validate_private_key_path(&path))
}

/// GPUI 目录操作复用旧壳的 Windows IFileOpenDialog。该实现会把已注册的
/// WSL 发行版钉入侧栏，并完整返回 `\\wsl.localhost\...` 路径。
#[cfg(windows)]
pub(crate) fn pick_folder_with_hwnd(
    hwnd: windows_sys::Win32::Foundation::HWND,
    title: &str,
) -> Option<PathBuf> {
    platform::pick_folder_with_hwnd(hwnd, title)
}

/// GPUI SFTP 上传用：允许多选，并把 Win32 模态框挂到当前 GPUI 窗口。
/// 调用方必须放到专用线程，避免对话框消息泵重入 GPUI 的 update 借用。
#[cfg(windows)]
pub(crate) fn pick_upload_files_with_hwnd(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> Vec<PathBuf> {
    platform::pick_files_with_hwnd(hwnd, &t!("file_dialog.choose_upload_files"), &[ALL_FILES_FILTER])
}

/// Validate a path selected by a non-winit UI shell. The legacy picker and GPUI
/// both use this exact classifier so a `.pub` file can never silently enter a
/// profile just because the shell used a different native file-dialog API.
pub(crate) fn validate_private_key_path(path: &std::path::Path) -> Result<PathBuf, String> {
    let contents = std::fs::read(path)
        .map_err(|err| t!("file_dialog.private_key.read_failed", path = path.display(), error = err).to_string())?;
    match classify_private_key_contents(&contents) {
        PrivateKeyFileKind::PrivateKey => Ok(path.to_path_buf()),
        PrivateKeyFileKind::PublicKey => Err(t!("file_dialog.private_key.public_key").to_string()),
        PrivateKeyFileKind::Unsupported => {
            Err(t!("file_dialog.private_key.unsupported").to_string())
        },
    }
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_upload_files(owner: &Window) -> Vec<PathBuf> {
    platform::pick_files(owner, &t!("file_dialog.choose_upload_files"), &[ALL_FILES_FILTER])
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_upload_directory(owner: &Window) -> Option<PathBuf> {
    platform::pick_folder(owner, &t!("file_dialog.choose_upload_directory"))
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_download_directory(owner: &Window) -> Option<PathBuf> {
    platform::pick_folder(owner, &t!("file_dialog.choose_download_directory"))
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_side_panel_directory(owner: &Window) -> Option<PathBuf> {
    platform::pick_folder(owner, &t!("file_dialog.choose_tree_root"))
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_startup_directory(owner: &Window) -> Option<PathBuf> {
    platform::pick_folder(owner, &t!("file_dialog.choose_startup_directory"))
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_terminal_directory(owner: &Window) -> Option<PathBuf> {
    platform::pick_folder(owner, &t!("file_dialog.import_terminal_directory"))
}

#[cfg(feature = "legacy-shell")]
pub(super) fn save_workspace_file(owner: &Window, default_name: &str) -> Option<PathBuf> {
    platform::save_file(owner, &t!("file_dialog.export_workspace"), WORKSPACE_FILTERS, default_name)
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_workspace_file(owner: &Window) -> Option<PathBuf> {
    platform::pick_file(owner, &t!("file_dialog.open_workspace"), WORKSPACE_FILTERS)
}

#[cfg(feature = "legacy-shell")]
pub(super) fn save_backup_file(owner: &Window) -> Option<PathBuf> {
    platform::save_file(owner, &t!("file_dialog.export_backup"), BACKUP_FILTERS, "nebula-backup.nebula-backup")
}

#[cfg(feature = "legacy-shell")]
pub(super) fn pick_backup_file(owner: &Window) -> Option<PathBuf> {
    platform::pick_file(owner, &t!("file_dialog.restore_backup"), BACKUP_FILTERS)
}

#[cfg(test)]
mod tests {
    use super::{PrivateKeyFileKind, classify_private_key_contents};

    #[test]
    fn public_key_text_is_rejected_as_private_key() {
        assert_eq!(
            classify_private_key_contents(b"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest user@host"),
            PrivateKeyFileKind::PublicKey
        );
    }

    #[test]
    fn encrypted_and_putty_private_key_headers_are_accepted_for_later_unlock() {
        assert_eq!(
            classify_private_key_contents(
                concat!("-----BEGIN OPENSSH ", "PRIVATE KEY-----\nnot-decoded-until-passphrase\n")
                    .as_bytes()
            ),
            PrivateKeyFileKind::PrivateKey
        );
        assert_eq!(
            classify_private_key_contents(
                concat!("PuTTY-User-Key-", "File-3: ssh-ed25519\nEncryption: aes256-cbc")
                    .as_bytes()
            ),
            PrivateKeyFileKind::PrivateKey
        );
    }

    #[test]
    fn unrelated_file_is_not_accepted_as_private_key() {
        assert_eq!(
            classify_private_key_contents(b"this is not an SSH key"),
            PrivateKeyFileKind::Unsupported
        );
    }
}
