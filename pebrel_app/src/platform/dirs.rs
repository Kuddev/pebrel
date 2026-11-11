//! 用户目录解析（005 §5 L1）。
//!
//! Pebrel 的应用路径适配器。共享设置 crate 负责路径优先级与启动迁移，
//! 这里缓存应用侧解析结果，所有配置、历史、会话和字体继续使用同一目录。
//!
//! ## 各平台落点
//!
//! | 平台 | 目录 |
//! | :-- | :-- |
//! | Windows | `%APPDATA%\Pebrel` |
//! | macOS | `~/Library/Application Support/Pebrel` |
//! | Linux | `$XDG_CONFIG_HOME/pebrel`，未设则 `~/.config/pebrel` |
//!
//! `PEBREL_CONFIG_DIR` 优先，`NEBULA_CONFIG_DIR` 是保留的便携/测试兼容入口。
//! 主程序在路径被缓存、数据被打开之前执行非覆盖迁移，旧来源保留供恢复。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 覆盖数据目录。便携模式与自动化测试用——`ui_probe` 之类的工具跑起来
/// 会写设置文件，指到临时目录就不会污染真实配置。
#[cfg(test)]
fn has_override() -> bool {
    ["PEBREL_CONFIG_DIR", "NEBULA_CONFIG_DIR"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// 用户主目录。Windows `USERPROFILE`，Unix `HOME`。
///
/// 存在的理由是仓库里有 6 处只读 `USERPROFILE` 而没有 `HOME` 回落
/// （`ai_hook.rs` 找 `.claude` / `.codex` / `.pi` 的四处最典型），那些路径
/// 在 Linux/macOS 上恒为 `None`，对应功能静默消失。
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let raw = std::env::var_os("USERPROFILE");
    #[cfg(unix)]
    let raw = std::env::var_os("HOME");

    raw.map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
}

/// Pebrel 的配置与数据目录，必要时创建。
///
/// 结果缓存在 `OnceLock`：改造前 26 个调用点每次都要读环境变量 +
/// `create_dir_all` 一次。缓存同时保证进程内路径恒定——半途改环境变量
/// 不会让一部分文件写到旧目录、另一部分写到新目录。
pub fn data_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = resolve_data_dir();
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
}

fn resolve_data_dir() -> PathBuf {
    // `pebrel_settings.txt`, runtime discovery, sessions, history, and imported
    // fonts must share one directory. The zero-dependency settings crate owns
    // the environment/OS resolution so consumers outside `pebrel_app` cannot
    // accidentally revive the old Linux/macOS paths.
    pebrel_settings::settings_dir()
}

/// 数据目录下的一项（文件或只含文件的目录）。AI 会话记录和 Agent 识别规则覆盖
/// 以前各自计算路径，改名后仍写在旧的 `Nebula` 目录，启动迁移的文件清单也没有包含
/// 它们。首次解析路径时补齐缺失文件；失败后下次启动可重试，已有文件保持不变。
pub(crate) fn adopted_data_entry(name: &str) -> PathBuf {
    let current = data_dir().join(name);
    if let Some(legacy) = pre_rename_entry_dir() {
        adopt_entry(&legacy.join(name), &current);
    }
    current
}

/// 旧计算方式的落点。设了覆盖目录时新旧都指向覆盖目录，无需复制。
fn pre_rename_entry_dir() -> Option<PathBuf> {
    let overridden = ["PEBREL_CONFIG_DIR", "NEBULA_CONFIG_DIR"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    if overridden {
        return None;
    }
    std::env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
        .map(|root| root.join("Nebula"))
}

fn adopt_entry(legacy: &Path, current: &Path) {
    if let Err(error) = try_adopt_entry(legacy, current) {
        log::warn!("could not copy {} to {}: {error}", legacy.display(), current.display());
    }
}

fn try_adopt_entry(legacy: &Path, current: &Path) -> std::io::Result<()> {
    if legacy == current {
        return Ok(());
    }
    let source = match std::fs::symlink_metadata(legacy) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if source.is_file() {
        return pebrel_settings::adopt_legacy_data_file(legacy, current);
    }
    if source.is_dir() {
        match std::fs::symlink_metadata(current) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Ok(());
            },
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(current)?;
            },
            Err(error) => return Err(error),
        }
        for entry in std::fs::read_dir(legacy)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                pebrel_settings::adopt_legacy_data_file(
                    &entry.path(),
                    &current.join(entry.file_name()),
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopting_copies_missing_entries_without_overwriting_or_removing() {
        let temp = tempfile::tempdir().unwrap();
        let (legacy, current) = (temp.path().join("Nebula"), temp.path().join("Pebrel"));
        std::fs::create_dir_all(legacy.join("agent-detection")).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(legacy.join("ai_sessions.json"), b"old").unwrap();
        std::fs::write(legacy.join("agent-detection/codex.toml"), b"rules").unwrap();

        adopt_entry(&legacy.join("ai_sessions.json"), &current.join("ai_sessions.json"));
        adopt_entry(&legacy.join("agent-detection"), &current.join("agent-detection"));
        assert_eq!(std::fs::read(current.join("ai_sessions.json")).unwrap(), b"old");
        assert_eq!(std::fs::read(current.join("agent-detection/codex.toml")).unwrap(), b"rules");
        assert!(legacy.join("ai_sessions.json").exists());

        std::fs::write(current.join("ai_sessions.json"), b"new").unwrap();
        std::fs::write(legacy.join("ai_sessions.json"), b"older").unwrap();
        adopt_entry(&legacy.join("ai_sessions.json"), &current.join("ai_sessions.json"));
        assert_eq!(std::fs::read(current.join("ai_sessions.json")).unwrap(), b"new");

        adopt_entry(&legacy.join("missing.json"), &current.join("missing.json"));
        assert!(!current.join("missing.json").exists());
    }

    #[test]
    fn adopting_completes_a_partial_directory_without_replacing_new_rules() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("legacy");
        let current = temp.path().join("current");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::create_dir(&current).unwrap();
        std::fs::write(legacy.join("codex.toml"), b"old codex").unwrap();
        std::fs::write(legacy.join("claude.toml"), b"old claude").unwrap();
        std::fs::write(current.join("codex.toml"), b"new codex").unwrap();

        adopt_entry(&legacy, &current);
        assert_eq!(std::fs::read(current.join("codex.toml")).unwrap(), b"new codex");
        assert_eq!(std::fs::read(current.join("claude.toml")).unwrap(), b"old claude");
    }

    #[cfg(unix)]
    #[test]
    fn adopting_preserves_a_dangling_destination_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("legacy.json");
        let current = temp.path().join("current.json");
        let missing = temp.path().join("missing.json");
        std::fs::write(&legacy, b"old").unwrap();
        std::os::unix::fs::symlink(&missing, &current).unwrap();

        adopt_entry(&legacy, &current);
        assert_eq!(std::fs::read_link(&current).unwrap(), missing);
        assert!(!missing.exists());
    }

    #[test]
    fn home_dir_rejects_empty_env() {
        // 空字符串环境变量在实践中出现过（某些登录管理器），当作未设置。
        // 这里只验证过滤器本身——直接改进程环境会污染其他并发测试。
        let empty = PathBuf::from("");
        assert!(Some(empty).filter(|p: &PathBuf| !p.as_os_str().is_empty()).is_none());
    }

    #[test]
    fn data_dir_is_absolute_and_named() {
        let dir = resolve_data_dir();
        assert!(dir.is_absolute(), "数据目录必须是绝对路径：{dir:?}");
        // 覆盖环境变量下叶子名可以是任意的，只在默认路径上校验命名。
        if !has_override() {
            let leaf = dir.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            assert!(
                leaf.eq_ignore_ascii_case("pebrel"),
                "默认数据目录应以 pebrel 结尾，实际 {leaf:?}"
            );
        }
    }

    /// Migration runs before the application writes to the renamed directory.
    #[cfg(windows)]
    #[test]
    fn windows_path_uses_pebrel_layout() {
        if has_override() {
            return;
        }
        let Some(appdata) = std::env::var_os("APPDATA") else { return };
        assert_eq!(resolve_data_dir(), PathBuf::from(appdata).join("Pebrel"));
    }

    /// Linux 侧遵守 XDG：相对路径的 `XDG_CONFIG_HOME` 按规范视为未设置。
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_path_follows_xdg() {
        if has_override() {
            return;
        }
        let dir = resolve_data_dir();
        assert_eq!(dir.file_name().and_then(|name| name.to_str()), Some("pebrel"));
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            if xdg.is_absolute() {
                assert_eq!(dir, xdg.join("pebrel"));
                return;
            }
        }
        if let Some(home) = home_dir() {
            assert_eq!(dir, home.join(".config").join("pebrel"));
        }
    }

    /// macOS 有自己的规矩：Application Support，不是 `~/.config`。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_path_uses_application_support() {
        if has_override() {
            return;
        }
        let Some(home) = home_dir() else { return };
        assert_eq!(
            resolve_data_dir(),
            home.join("Library").join("Application Support").join("Pebrel")
        );
    }
}
