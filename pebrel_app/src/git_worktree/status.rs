//! 桌面与手机共用 Git 的 NUL 分隔状态，文件名不是展示文本，不能按行或空格拆分。

use serde::Serialize;

pub(crate) const STATUS_ARGS: &[&str] = &["status", "--porcelain=v2", "--branch", "-z"];

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct GitStatus {
    pub branch: String,
    pub head: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<GitEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GitEntry {
    pub path: String,
    pub original: Option<String>,
    pub index: char,
    pub worktree: char,
    pub conflict: bool,
}

impl GitEntry {
    pub(crate) fn staged(&self) -> bool {
        !matches!(self.index, '.' | '?')
    }

    pub(crate) fn unstaged(&self) -> bool {
        self.worktree != '.'
    }
}

pub(crate) fn parse_status(output: &str) -> Result<GitStatus, &'static str> {
    if !output.ends_with('\0') {
        return Err("git_invalid_status");
    }
    let mut result = GitStatus::default();
    let mut records = output.split_terminator('\0');
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            if let Some(value) = header.strip_prefix("branch.head ") {
                result.branch = value.into();
            } else if let Some(value) = header.strip_prefix("branch.oid ") {
                result.head = value.into();
            } else if let Some(value) = header.strip_prefix("branch.upstream ") {
                result.upstream = Some(value.into());
            } else if let Some(value) = header.strip_prefix("branch.ab ") {
                let (ahead, behind) = value.split_once(' ').ok_or("git_invalid_status")?;
                result.ahead = ahead
                    .strip_prefix('+')
                    .ok_or("git_invalid_status")?
                    .parse()
                    .map_err(|_| "git_invalid_status")?;
                result.behind = behind
                    .strip_prefix('-')
                    .ok_or("git_invalid_status")?
                    .parse()
                    .map_err(|_| "git_invalid_status")?;
            }
            continue;
        }
        if record.starts_with("! ") {
            continue;
        }
        let kind = record.as_bytes().first().copied().ok_or("git_invalid_status")?;
        let entry = if kind == b'?' {
            GitEntry {
                path: record.strip_prefix("? ").ok_or("git_invalid_status")?.into(),
                original: None,
                index: '?',
                worktree: '?',
                conflict: false,
            }
        } else {
            let count = match kind {
                b'1' => 9,
                b'2' => 10,
                b'u' => 11,
                _ => return Err("git_invalid_status"),
            };
            let fields: Vec<_> = record.splitn(count, ' ').collect();
            if fields.len() != count || fields[1].len() != 2 || !fields[1].is_ascii() {
                return Err("git_invalid_status");
            }
            let xy = fields[1].as_bytes();
            GitEntry {
                path: fields[count - 1].into(),
                original: if kind == b'2' {
                    Some(records.next().ok_or("git_invalid_status")?.into())
                } else {
                    None
                },
                index: xy[0] as char,
                worktree: xy[1] as char,
                conflict: kind == b'u',
            }
        };
        if entry.path.is_empty() || entry.original.as_ref().is_some_and(|p| p.is_empty()) {
            return Err("git_invalid_status");
        }
        result.entries.push(entry);
    }
    if result.branch.is_empty() || result.head.is_empty() {
        return Err("git_invalid_status");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_rename_identity_and_partial_staging() {
        let status = parse_status(
            "# branch.oid abc\0# branch.head main\0# branch.ab +2 -1\0\
            2 RM N... 100644 100644 100644 abc def R100 新 name\n.txt\0old name.txt\0? -new.txt\0",
        )
        .unwrap();
        assert_eq!((status.ahead, status.behind), (2, 1));
        assert_eq!(status.entries[0].path, "新 name\n.txt");
        assert_eq!(status.entries[0].original.as_deref(), Some("old name.txt"));
        assert!(status.entries[0].staged() && status.entries[0].unstaged());
        assert!(!status.entries[1].staged());
    }

    #[test]
    fn rejects_partial_records_and_keeps_conflicts() {
        assert!(parse_status("# branch.head main").is_err());
        let result = parse_status("# branch.oid abc\0# branch.head main\0u UU N... 100644 100644 100644 100644 a b c conflict.txt\0").unwrap();
        assert!(result.entries[0].conflict);
    }
}
