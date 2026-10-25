//! 从已经显示的目录输出取字段；不重新枚举文件，也不补造权限、大小或类型。
use super::{MAX_TABLE_ROWS, Table};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    Unknown,
    File,
    Directory,
    Link,
}

pub(crate) struct FileMetadata {
    pub name_column: usize,
    pub kinds: Vec<FileKind>,
    pub headers: bool,
    pub preamble: Vec<String>,
}

impl Table {
    pub fn parse_directory(text: &str, powershell: bool) -> Result<Self, &'static str> {
        if text.len() > nebula_terminal::inline_content::MAX_CONTENT_BYTES {
            return Err("limit");
        }
        let lines = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(MAX_TABLE_ROWS + 1)
            .collect::<Vec<_>>();
        if lines.len() > MAX_TABLE_ROWS {
            return Err("limit");
        }
        if powershell {
            if let Some(table) = powershell_table(&lines) {
                return Ok(table);
            }
        }
        if let Some(table) = long_listing(&lines, powershell)
            .or_else(|| powershell.then(|| long_listing(&lines, false)).flatten())
        {
            return Ok(table);
        }
        // ls 的多列空白也可能属于文件名。没有结构证据时保留整行和顺序，
        // 不以 split_whitespace 把一个名字猜成多个文件，更不查询当前磁盘补信息。
        Ok(Self {
            columns: vec!["Name".into()],
            rows: lines.iter().map(|line| vec![(*line).to_owned()]).collect(),
            files: Some(FileMetadata {
                name_column: 0,
                kinds: vec![FileKind::Unknown; lines.len()],
                headers: false,
                preamble: Vec::new(),
            }),
        })
    }
}

fn mode_kind(mode: &str) -> Option<FileKind> {
    let bytes = mode.as_bytes();
    if bytes.len() < 10
        || !b"-dlcbps".contains(&bytes[0])
        || !bytes[1..10].iter().all(|c| b"rwxstST-".contains(c))
        || !bytes[10..].iter().all(|c| b"+@.".contains(c))
    {
        return None;
    }
    Some(match bytes[0] {
        b'd' => FileKind::Directory,
        b'l' => FileKind::Link,
        _ => FileKind::File,
    })
}

fn take_word<'a>(rest: &mut &'a str) -> Option<&'a str> {
    *rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let word = &rest[..end];
    *rest = &rest[end..];
    Some(word)
}

fn long_listing(lines: &[&str], powershell: bool) -> Option<Table> {
    let mut rows = Vec::new();
    let mut kinds = Vec::new();
    let mut preamble = Vec::new();
    let mut compact = None;
    for line in lines {
        if rows.is_empty()
            && line.trim().strip_prefix("total ").is_some_and(|n| n.parse::<u64>().is_ok())
        {
            preamble.push((*line).to_owned());
            continue;
        }
        let mut rest = *line;
        let mode = take_word(&mut rest)?;
        let kind = mode_kind(mode)?;
        let first = take_word(&mut rest)?;
        // Nebula-List 的旧输出只有 mode/size/date/name；没有 owner/link 字段。
        let short = powershell;
        if compact.is_some_and(|previous| previous != short) {
            return None;
        }
        compact = Some(short);
        let mut row = vec![mode.to_owned()];
        if short {
            row.push(first.to_owned());
        } else {
            first.parse::<u64>().ok()?;
            row.extend([first, take_word(&mut rest)?, take_word(&mut rest)?].map(str::to_owned));
            let size = take_word(&mut rest)?;
            if !size.chars().next()?.is_ascii_digit() {
                return None;
            }
            row.push(size.to_owned());
        }
        let date = [take_word(&mut rest)?, take_word(&mut rest)?, take_word(&mut rest)?];
        if !date[1].bytes().all(|b| b.is_ascii_digit())
            || !date[2].bytes().all(|b| b.is_ascii_digit() || b == b':')
        {
            return None;
        }
        row.push(date.join(" "));
        // 时间后的第一个空格（旧 PowerShell wrapper 为两个）是列分隔；
        // 其余空格属于打印出的文件名，不能用 trim_start 丢弃。
        let name = rest.strip_prefix(if short { "  " } else { " " })?;
        if name.is_empty() {
            return None;
        }
        row.push(name.to_owned());
        rows.push(row);
        kinds.push(kind);
    }
    let columns: &[&str] = if compact? {
        &["Mode", "Size", "Modified", "Name"]
    } else {
        &["Mode", "Links", "Owner", "Group", "Size", "Modified", "Name"]
    };
    Some(Table {
        columns: columns.iter().map(|name| (*name).to_owned()).collect(),
        rows,
        files: Some(FileMetadata {
            name_column: columns.len() - 1,
            kinds,
            headers: true,
            preamble,
        }),
    })
}

fn powershell_table(lines: &[&str]) -> Option<Table> {
    let separator = lines.iter().position(|line| {
        line.contains("----") && line.chars().all(|c| c == '-' || c.is_whitespace())
    })?;
    let header = lines.get(separator.checked_sub(1)?)?;
    let mut starts = Vec::new();
    let mut in_field = false;
    for (index, c) in lines[separator].chars().enumerate() {
        if c == '-' && !in_field {
            starts.push(index);
        }
        in_field = c == '-';
    }
    if starts.len() < 2 || starts.len() > 8 {
        return None;
    }
    let fields = |line: &str| {
        let chars: Vec<_> = line.chars().collect();
        starts
            .iter()
            .enumerate()
            .map(|(i, start)| {
                let end = starts.get(i + 1).copied().unwrap_or(chars.len()).min(chars.len());
                chars[(*start).min(chars.len())..end].iter().collect::<String>().trim().to_owned()
            })
            .collect::<Vec<_>>()
    };
    let columns = fields(header);
    if columns != ["Mode", "LastWriteTime", "Length", "Name"] {
        return None;
    }
    let name_column = columns.iter().position(|name| name == "Name")?;
    let mode_column = columns.iter().position(|name| name == "Mode");
    let mut rows = Vec::new();
    let mut kinds = Vec::new();
    for line in &lines[separator + 1..] {
        // 日期/大小右对齐，数据可能伸到表头起点左边；从 Name 列反向取大小，
        // 首词是 Mode，两者之间的完整片段才是时间，不按横线左端硬切。
        let chars: Vec<_> = line.chars().collect();
        let name_start = starts[name_column];
        if chars.len() < name_start {
            return None;
        }
        let name = chars[name_start..].iter().collect::<String>();
        let prefix = chars[..name_start].iter().collect::<String>();
        let mut rest = prefix.as_str();
        let mode = take_word(&mut rest)?;
        if !mode.chars().all(|c| "darlsh-".contains(c)) {
            return None;
        }
        let rest = rest.trim();
        let (modified, length) = match rest.rsplit_once(char::is_whitespace) {
            Some((modified, length))
                if !length.is_empty() && length.bytes().all(|c| c.is_ascii_digit()) =>
            {
                (modified.trim(), length)
            },
            _ if mode.starts_with('d') => (rest, ""),
            _ => return None,
        };
        let row = vec![mode.to_owned(), modified.to_owned(), length.to_owned(), name];
        if row[name_column].is_empty() || line.trim_start().starts_with("Directory:") {
            return None;
        }
        let mode = mode_column.map(|column| row[column].as_str()).unwrap_or("");
        let kind = if mode.starts_with('d') {
            FileKind::Directory
        } else if mode.contains('l') {
            FileKind::Link
        } else if !mode.is_empty() {
            FileKind::File
        } else {
            FileKind::Unknown
        };
        rows.push(row);
        kinds.push(kind);
    }
    Some(Table {
        columns,
        rows,
        files: Some(FileMetadata {
            name_column,
            kinds,
            headers: true,
            preamble: lines[..separator - 1].iter().map(|line| (*line).to_owned()).collect(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listings_only_contain_reported_fields_and_preserve_names() {
        let table = Table::parse_directory("alpha  two words\n中文 name\n", false).unwrap();
        assert_eq!(table.columns, ["Name"]);
        assert_eq!(table.rows[0], ["alpha  two words"]);
        assert!(!table.files.unwrap().headers);
        let table = Table::parse_directory(
            "total 8\ndrwxr-xr-x 2 user staff 4096 Oct 20 09:20 中文  folder\n-rw-r--r-- 1 user staff 12 Oct 19 2026 a b.txt\nlrwxrwxrwx 1 user staff 8 Oct 19 08:00 link -> a b.txt\n",
            false,
        ).unwrap();
        assert_eq!(table.rows[0].last().unwrap(), "中文  folder");
        assert_eq!(
            table.files.as_ref().unwrap().kinds,
            [FileKind::Directory, FileKind::File, FileKind::Link]
        );
        assert_eq!(table.files.unwrap().preamble, ["total 8"]);
        let table =
            Table::parse_directory("drwxr-xr-x       -  Oct 20 09:20  中文目录\n", true).unwrap();
        assert_eq!(table.columns, ["Mode", "Size", "Modified", "Name"]);
        assert_eq!(table.rows[0][1], "-");
        let table = Table::parse_directory("-rw-r--r-- 1 u g 1 Oct 20 09:20   a\n", false).unwrap();
        assert_eq!(table.rows[0].last().unwrap(), "  a");
    }

    #[test]
    fn powershell_table_keeps_actual_columns_and_bounded_input() {
        let row = |mode: &str, date: &str, size: &str, name: &str| {
            format!("{mode:<7}{date:>26} {size:>14} {name}\n")
        };
        let text = format!(
            "    Directory: C:\\work\n\n{}{}{}{}",
            row("Mode", "LastWriteTime", "Length", "Name"),
            row("----", "-------------", "------", "----"),
            row("d-----", "2026/10/20     16:35", "", "中文目录"),
            row("-a----", "2026/10/20     16:35", "12", "  file name.txt")
        );
        let table = Table::parse_directory(&text, true).unwrap();
        assert_eq!(table.columns, ["Mode", "LastWriteTime", "Length", "Name"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], ["d-----", "2026/10/20     16:35", "", "中文目录"]);
        assert_eq!(table.rows[1], ["-a----", "2026/10/20     16:35", "12", "  file name.txt"]);
        assert_eq!(table.files.unwrap().preamble, ["    Directory: C:\\work"]);
        assert!(matches!(
            Table::parse_directory(&"x\n".repeat(MAX_TABLE_ROWS + 1), false),
            Err("limit")
        ));
        let real = "Mode                 LastWriteTime         Length Name\n----                 -------------         ------ ----\n-a----        2026/10/20     16:35           8733 directory.rs\n";
        assert_eq!(
            Table::parse_directory(real, true).unwrap().rows[0],
            ["-a----", "2026/10/20     16:35", "8733", "directory.rs"]
        );
    }
}
