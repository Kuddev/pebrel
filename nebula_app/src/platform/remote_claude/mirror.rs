//! 本机项目目录 → 远端镜像目录的固定映射（会话锚点）。
//!
//! Claude Code 按 cwd 给会话分组（`~/.claude/projects/<cwd 编码>/`），远端又没有
//! 项目副本，所以这条映射就是"同一目录反复进入复用同一组会话、不同目录互不相干"
//! 的唯一依据。规则发布后不得改动：改了等于让用户已有的远端历史从 `/resume`
//! 里消失。

use std::fmt;

/// 远端 `$HOME` 下的镜像根目录名。
pub(super) const MIRROR_ROOT: &str = "pebrel-remote";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum MirrorError {
    NotAbsolute,
    Unsupported,
}

impl fmt::Display for MirrorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAbsolute => "not an absolute drive or UNC path",
            Self::Unsupported => "contains a component that cannot be mirrored",
        })
    }
}

/// 把 [`std::fs::canonicalize`] 得到的 Windows 绝对路径映射成镜像根下的相对 POSIX
/// 路径。调用方先规范化，大小写因此取磁盘上的真实名字：同一目录无论用户怎么
/// 拼写，都落到同一个镜像目录。
///
/// - `C:\Users\x\proj` → `c/Users/x/proj`（只有盘符转小写）
/// - `\\server\share\dir` → `unc/server/share/dir`
/// - `D:\` → `d`
pub(super) fn relative_mirror(path: &str) -> Result<String, MirrorError> {
    let path = strip_verbatim(path);
    let (head, rest) = if let Some(unc) = path.strip_prefix(r"\\") {
        ("unc".to_owned(), unc)
    } else {
        let bytes = path.as_bytes();
        if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
            return Err(MirrorError::NotAbsolute);
        }
        let rest = &path[2..];
        if !rest.is_empty() && !rest.starts_with(['\\', '/']) {
            return Err(MirrorError::NotAbsolute);
        }
        ((bytes[0] as char).to_ascii_lowercase().to_string(), rest)
    };
    let mut mirror = head;
    for component in rest.split(['\\', '/']).filter(|component| !component.is_empty()) {
        if matches!(component, "." | "..") || component.chars().any(char::is_control) {
            return Err(MirrorError::Unsupported);
        }
        mirror.push('/');
        mirror.push_str(component);
    }
    if mirror == "unc" {
        return Err(MirrorError::NotAbsolute);
    }
    Ok(mirror)
}

/// 同一项目在远端的稳定状态键（`~/.pebrel-remote/projects/<key>/`）。
///
/// 系统提示词里引用的 SSH 配置路径就挂在这个键下：路径不随每次连接变化，
/// 恢复会话时提示词逐字节一致，模型的提示词缓存才能继续命中。
pub(super) fn project_key(relative: &str) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(relative.as_bytes());
    digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

/// 交给提示词与界面的本机路径：`canonicalize` 产出的 `\\?\` 前缀还原成普通
/// Windows 形式（同一个目录无论怎么拼写，展示给模型的都逐字节一致）。
pub(super) fn display_path(path: &std::path::Path) -> String {
    strip_verbatim(&path.to_string_lossy()).into_owned()
}

fn strip_verbatim(path: &str) -> std::borrow::Cow<'_, str> {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        // 还原成普通 UNC 形式，让上面的 UNC 分支照常识别。
        return format!(r"\\{unc}").into();
    }
    path.strip_prefix(r"\\?\").unwrap_or(path).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_stripped_for_display() {
        assert_eq!(display_path(std::path::Path::new(r"\\?\C:\work\proj")), r"C:\work\proj");
        assert_eq!(
            display_path(std::path::Path::new(r"\\?\UNC\server\share\dir")),
            r"\\server\share\dir"
        );
    }

    #[test]
    fn mirror_paths_are_stable_and_lowercase_the_drive() {
        assert_eq!(relative_mirror(r"C:\work\proj").unwrap(), "c/work/proj");
        assert_eq!(relative_mirror(r"\\?\D:\a\b\").unwrap(), "d/a/b");
        assert_eq!(relative_mirror(r"\\server\share\dir").unwrap(), "unc/server/share/dir");
        assert_eq!(relative_mirror(r"\\?\UNC\server\share\dir").unwrap(), "unc/server/share/dir");
        assert_eq!(relative_mirror(r"D:\").unwrap(), "d");
        // 相对路径、坏的驱动器写法与 `..` 段都不可映射。
        assert_eq!(relative_mirror(r"work\proj"), Err(MirrorError::NotAbsolute));
        assert_eq!(relative_mirror(r"1:\work"), Err(MirrorError::NotAbsolute));
        assert_eq!(relative_mirror(r"C:work"), Err(MirrorError::NotAbsolute));
        assert_eq!(relative_mirror(r"C:\work\..\proj"), Err(MirrorError::Unsupported));
    }

    #[test]
    fn project_keys_are_short_and_project_specific() {
        let key = project_key("c/work/proj");
        assert_eq!(key.len(), 16);
        assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(key, project_key("c/work/other"));
    }
}
