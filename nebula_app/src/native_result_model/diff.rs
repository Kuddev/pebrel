//! 有界 unified patch 视图模型；只解释现有输出，不查询仓库或执行 diff 命令。
use std::ops::Range;
use std::sync::Arc;

const MAX_FILES: usize = 256;
const MAX_LINES: usize = 20_000;
const MAX_DETAILS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Context,
    Added,
    Deleted,
    Hunk,
    Note,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub kind: Kind,
}

pub(crate) struct File {
    pub path: String,
    pub details: Vec<String>,
    pub source: Range<usize>,
    pub body: Arc<str>,
    pub rows: Arc<[Row]>,
    pub added: usize,
    pub deleted: usize,
    pub max_line: u32,
}

pub(crate) struct Patch {
    pub preamble: Arc<str>,
    pub files: Vec<File>,
}

struct Hunk {
    old: u32,
    new: u32,
    old_left: u32,
    new_left: u32,
}

struct Pending {
    path: String,
    old_path: Option<String>,
    details: Vec<String>,
    start: usize,
    body_start: Option<usize>,
    rows: Vec<Row>,
    added: usize,
    deleted: usize,
    hunk: Option<Hunk>,
}

impl Pending {
    fn finish_hunk(&self) -> Result<(), &'static str> {
        if self.hunk.as_ref().is_some_and(|h| h.old_left != 0 || h.new_left != 0) {
            return Err("format");
        }
        Ok(())
    }

    fn finish(self, text: &str, end: usize) -> Result<File, &'static str> {
        self.finish_hunk()?;
        let body = self.body_start.map_or("", |start| &text[start..end]);
        let body = body.strip_suffix('\n').unwrap_or(body);
        let body = body.strip_suffix('\r').unwrap_or(body);
        Ok(File {
            path: self.path,
            details: self.details,
            source: self.start..end,
            body: Arc::from(body),
            max_line: self
                .rows
                .iter()
                .flat_map(|row| [row.old, row.new])
                .flatten()
                .max()
                .unwrap_or(1),
            rows: self.rows.into(),
            added: self.added,
            deleted: self.deleted,
        })
    }
}

impl Patch {
    pub fn parse(text: &str, from_grid: bool) -> Result<Self, &'static str> {
        if text.len() > nebula_terminal::inline_content::MAX_CONTENT_BYTES {
            return Err("limit");
        }
        let mut files = Vec::new();
        let mut pending: Option<Pending> = None;
        let mut offset = 0;
        let mut preamble_end = None;
        for (index, raw) in text.split_inclusive('\n').enumerate() {
            if index == MAX_LINES {
                return Err("limit");
            }
            let start = offset;
            offset += raw.len();
            let line = raw.trim_end_matches(['\r', '\n']);
            if line.starts_with("diff --cc ") || line.starts_with("diff --combined ") {
                // 合并差异没有单一旧行号；保留原文，不把多父版本伪装成双列 patch。
                return Err("format");
            }
            if let Some(header) = line.strip_prefix("diff --git ") {
                if let Some(file) = pending.take() {
                    files.push(file.finish(text, start)?);
                }
                if files.len() == MAX_FILES {
                    return Err("limit");
                }
                preamble_end.get_or_insert(start);
                pending = Some(Pending {
                    path: header_path(header),
                    old_path: None,
                    details: Vec::new(),
                    start,
                    body_start: None,
                    rows: Vec::new(),
                    added: 0,
                    deleted: 0,
                    hunk: None,
                });
                continue;
            }
            let Some(file) = pending.as_mut() else { continue };
            if let Some(header) = line.strip_prefix("@@ ") {
                file.finish_hunk()?;
                file.hunk = Some(hunk(header)?);
                file.body_start.get_or_insert(start);
                file.rows.push(Row { old: None, new: None, kind: Kind::Hunk });
                continue;
            }
            if line.starts_with("@@") {
                return Err("format");
            }
            if let Some(hunk) = file.hunk.as_mut() {
                let kind = match line.as_bytes().first() {
                    Some(b'+') => Kind::Added,
                    Some(b'-') => Kind::Deleted,
                    Some(b' ') => Kind::Context,
                    Some(b'\\')
                        if line == "\\ No newline at end of file"
                            && file.rows.last().is_some_and(|row| {
                                matches!(row.kind, Kind::Added | Kind::Deleted | Kind::Context)
                            }) =>
                    {
                        Kind::Note
                    },
                    // VT 的显示文本会去掉空白 context 行末尾的空格；只在两个范围仍有余量时接纳。
                    None if from_grid && hunk.old_left > 0 && hunk.new_left > 0 => Kind::Context,
                    _ => return Err("format"),
                };
                let old = if matches!(kind, Kind::Deleted | Kind::Context) {
                    Some(advance(&mut hunk.old, &mut hunk.old_left)?)
                } else {
                    None
                };
                let new = if matches!(kind, Kind::Added | Kind::Context) {
                    Some(advance(&mut hunk.new, &mut hunk.new_left)?)
                } else {
                    None
                };
                file.added += usize::from(kind == Kind::Added);
                file.deleted += usize::from(kind == Kind::Deleted);
                file.rows.push(Row { old, new, kind });
            } else if let Some(path) = line.strip_prefix("--- ") {
                let path = path_value(path)?;
                file.old_path = Some(path.strip_prefix("a/").unwrap_or(&path).to_owned());
            } else if let Some(path) = line.strip_prefix("+++ ") {
                let path = path_value(path)?;
                file.path = if path == "/dev/null" {
                    file.old_path.clone().ok_or("format")?
                } else {
                    path.strip_prefix("b/").unwrap_or(&path).to_owned()
                };
            } else {
                if line.starts_with("GIT binary patch") || file.details.len() == MAX_DETAILS {
                    return Err(if file.details.len() == MAX_DETAILS { "limit" } else { "format" });
                }
                if let Some(path) =
                    line.strip_prefix("rename to ").or_else(|| line.strip_prefix("copy to "))
                {
                    file.path = path_value(path)?;
                }
                file.details.push(line.to_owned());
            }
        }
        if let Some(file) = pending {
            files.push(file.finish(text, text.len())?);
        }
        let preamble_end = preamble_end.ok_or("format")?;
        Ok(Self { preamble: Arc::from(text[..preamble_end].trim_end()), files })
    }
}

fn advance(line: &mut u32, remaining: &mut u32) -> Result<u32, &'static str> {
    *remaining = remaining.checked_sub(1).ok_or("format")?;
    let current = *line;
    *line = line.checked_add(1).ok_or("format")?;
    Ok(current)
}

fn hunk(header: &str) -> Result<Hunk, &'static str> {
    let (ranges, _) = header.split_once(" @@").ok_or("format")?;
    let mut fields = ranges.split_whitespace();
    let (old, old_left) = range(fields.next().and_then(|s| s.strip_prefix('-')).ok_or("format")?)?;
    let (new, new_left) = range(fields.next().and_then(|s| s.strip_prefix('+')).ok_or("format")?)?;
    if fields.next().is_some() {
        return Err("format");
    }
    Ok(Hunk { old, new, old_left, new_left })
}

fn range(source: &str) -> Result<(u32, u32), &'static str> {
    let (start, count) = source.split_once(',').unwrap_or((source, "1"));
    if !start.bytes().all(|c| c.is_ascii_digit()) || !count.bytes().all(|c| c.is_ascii_digit()) {
        return Err("format");
    }
    let start = start.parse().map_err(|_| "format")?;
    let count = count.parse().map_err(|_| "format")?;
    if start == 0 && count != 0 {
        return Err("format");
    }
    Ok((start, count))
}

fn path_value(source: &str) -> Result<String, &'static str> {
    if !source.starts_with('"') {
        return Ok(source.split('\t').next().unwrap_or("").to_owned());
    }
    let (path, rest) = quoted_path(source)?;
    if !rest.is_empty() && !rest.starts_with('\t') {
        return Err("format");
    }
    Ok(path)
}

fn quoted_path(source: &str) -> Result<(String, &str), &'static str> {
    let mut bytes = source.as_bytes()[1..].iter().copied();
    let mut output = Vec::new();
    while let Some(byte) = bytes.next() {
        match byte {
            b'"' => {
                let path = String::from_utf8(output).map_err(|_| "format")?;
                return Ok((path, &source[source.len() - bytes.len()..]));
            },
            b'\\' => {
                let escaped = bytes.next().ok_or("format")?;
                output.push(match escaped {
                    b'"' | b'\\' => escaped,
                    b'a' => 7,
                    b'b' => 8,
                    b't' => 9,
                    b'n' => 10,
                    b'v' => 11,
                    b'f' => 12,
                    b'r' => 13,
                    b'0'..=b'3' => {
                        let second =
                            bytes.next().filter(|c| (b'0'..=b'7').contains(c)).ok_or("format")?;
                        let third =
                            bytes.next().filter(|c| (b'0'..=b'7').contains(c)).ok_or("format")?;
                        (escaped - b'0') * 64 + (second - b'0') * 8 + (third - b'0')
                    },
                    _ => return Err("format"),
                });
            },
            byte => output.push(byte),
        }
    }
    Err("format")
}

fn header_path(header: &str) -> String {
    if header.starts_with('"')
        && let Ok((_, rest)) = quoted_path(header)
        && let Some(new) = rest.strip_prefix(' ')
        && let Ok(new) = path_value(new)
    {
        return new.strip_prefix("b/").unwrap_or(&new).to_owned();
    }
    if let Some((old, new)) = header.rsplit_once(" b/")
        && old.strip_prefix("a/") == Some(new)
    {
        return new.to_owned();
    }
    // +++/rename/copy 元数据随后给出权威名称；未知前缀保留 header，不猜文件系统路径。
    header.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_patch_tracks_both_line_numbers_and_original_source() {
        let text = "commit fixture\n\ndiff --git a/config.ts b/config.ts\nindex 123..456 100644\n--- a/config.ts\n+++ b/config.ts\n@@ -12,3 +12,4 @@\n export const output = {\n-  format: 'text',\n+  format: 'auto',\n+  preserveSource: true,\n };\n";
        let patch = Patch::parse(text, false).unwrap();
        assert_eq!(patch.preamble.as_ref(), "commit fixture");
        let file = &patch.files[0];
        assert_eq!(file.path, "config.ts");
        assert_eq!((file.added, file.deleted), (2, 1));
        assert_eq!(
            file.rows.iter().map(|r| (r.old, r.new)).collect::<Vec<_>>(),
            [
                (None, None),
                (Some(12), Some(12)),
                (Some(13), None),
                (None, Some(13)),
                (None, Some(14)),
                (Some(14), Some(15)),
            ]
        );
        assert_eq!(&text[file.source.clone()], text.split_once("\n\n").unwrap().1);
        assert!(file.body.starts_with("@@ -12,3 +12,4 @@\n"));
    }

    #[test]
    fn paths_metadata_empty_sides_and_no_newline_markers_remain_distinct() {
        let text = "diff --git a/old b/new\nsimilarity index 100%\nrename from old\nrename to new name\ndiff --git a/new b/new\nnew file mode 100644\n--- /dev/null\n+++ \"b/\\344\\270\\255\\346\\226\\207.txt\"\n@@ -0,0 +1 @@\n+hello\n\\ No newline at end of file\n";
        let patch = Patch::parse(text, false).unwrap();
        assert_eq!(patch.files.len(), 2);
        assert_eq!(patch.files[0].path, "new name");
        assert!(patch.files[0].rows.is_empty());
        assert_eq!(patch.files[1].path, "中文.txt");
        assert_eq!(patch.files[1].rows[1].new, Some(1));
        assert_eq!(patch.files[1].rows[2].kind, Kind::Note);
        assert!(path_value("\"unterminated").is_err());
        assert!(path_value("\"path\"junk").is_err());
        let mode = "diff --git \"a/\\344\\270\\255\\346\\226\\207.txt\" \"b/\\344\\270\\255\\346\\226\\207.txt\"\nold mode 100644\nnew mode 100755\n";
        assert_eq!(Patch::parse(mode, false).unwrap().files[0].path, "中文.txt");
    }

    #[test]
    fn incomplete_or_unsupported_patches_fall_back_without_inventing_counts() {
        for text in [
            "not a patch",
            "diff --cc file\n@@@ -1 +1 +1 @@@\n line\n",
            "diff --git a/x b/x\n@@@ -1 +1 +1 @@@\n line\n",
            "diff --git a/x b/x\nGIT binary patch\nliteral 3\nabc\n",
            "diff --git a/x b/x\n@@ -1,2 +1 @@\n line\n",
            "diff --git a/x b/x\n@@ -0 +1 @@\n+line\n",
            "diff --git a/x b/x\n@@ -1 +1 @@\n line\n+extra\n",
        ] {
            assert!(Patch::parse(text, false).is_err(), "{text}");
        }
        let blank = "diff --git a/x b/x\n@@ -1 +1 @@\n\n";
        assert!(Patch::parse(blank, false).is_err());
        assert_eq!(Patch::parse(blank, true).unwrap().files[0].rows[1].kind, Kind::Context);
        assert!(Patch::parse(&"x\n".repeat(MAX_LINES + 1), false).is_err());
    }
}
