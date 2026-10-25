use serde_json::Value;
use std::cmp::Ordering;
use std::ops::Range;

pub(crate) mod diff;
mod directory;
pub(crate) use directory::{FileKind, FileMetadata};

pub(crate) const MAX_TABLE_ROWS: usize = 20_000;
const MAX_TABLE_COLUMNS: usize = 64;
const MAX_TABLE_CELLS: usize = 256 * 1024;

#[derive(Default)]
pub(crate) struct RawText {
    pub text: String,
    pub lines: Vec<Range<usize>>,
}
impl RawText {
    pub fn append(&mut self, chunk: &str) {
        let start = self.text.len();
        self.text.push_str(chunk);
        let mut line_start = self.lines.pop().map_or(start, |line| line.start);
        for (offset, c) in chunk.char_indices() {
            if matches!(c, '\r' | '\n') {
                let absolute = start + offset;
                if c != '\n' || absolute == 0 || self.text.as_bytes()[absolute - 1] != b'\r' {
                    self.lines.push(line_start..absolute);
                }
                line_start = absolute + c.len_utf8();
            }
        }
        self.lines.push(line_start..self.text.len());
    }
}

pub(crate) struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub files: Option<FileMetadata>,
}

impl Table {
    pub fn parse(text: &str) -> Result<Self, &'static str> {
        if text.len() > nebula_terminal::inline_content::MAX_CONTENT_BYTES {
            return Err("limit");
        }
        let values = bounded_values(text)?;
        let mut columns = Vec::new();
        for value in &values {
            let object = value.as_object().ok_or("table")?;
            for key in object.keys() {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
                if columns.len() > MAX_TABLE_COLUMNS
                    || columns.len().saturating_mul(values.len()) > MAX_TABLE_CELLS
                {
                    return Err("limit");
                }
            }
        }
        let rows = values
            .iter()
            .map(|value| {
                columns
                    .iter()
                    .map(|key| match value.get(key) {
                        Some(Value::String(text)) => text.clone(),
                        Some(Value::Null) | None => String::new(),
                        Some(value) => value.to_string(),
                    })
                    .collect()
            })
            .collect();
        Ok(Self { columns, rows, files: None })
    }

    pub fn order(&self, query: &str, sort: Option<(usize, bool)>) -> Vec<usize> {
        let query = query.to_lowercase();
        let mut order: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                query.is_empty() || row.iter().any(|cell| cell.to_lowercase().contains(&query))
            })
            .map(|(index, _)| index)
            .collect();
        if let Some((column, descending)) = sort {
            order.sort_by(|&a, &b| {
                let left = &self.rows[a][column];
                let right = &self.rows[b][column];
                // 先分类型再比较，避免 2 < 10 < 1a < 2 这样的循环关系。
                let cmp = match (numeric_cell(left), numeric_cell(right)) {
                    (Some(left), Some(right)) => left.total_cmp(&right),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => left.cmp(right),
                };
                if descending { cmp.reverse() } else { cmp }
            });
        }
        order
    }
}

pub(crate) fn numeric_cell(text: &str) -> Option<f64> {
    text.trim().trim_end_matches('%').parse::<f64>().ok().filter(|number| number.is_finite())
}

fn bounded_values(text: &str) -> Result<Vec<Value>, &'static str> {
    // 在反序列化过程中止住行数，而不是先把整个 JSON 数组膨胀成 Value 后才检查。
    struct Rows;
    impl<'de> serde::de::Visitor<'de> for Rows {
        type Value = Result<Vec<Value>, &'static str>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an array of table records")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut rows = Vec::new();
            while let Some(value) = seq.next_element::<Value>()? {
                if rows.len() == MAX_TABLE_ROWS {
                    return Ok(Err("limit"));
                }
                rows.push(value);
            }
            Ok(Ok(rows))
        }
    }
    if text.trim_start().starts_with('[') {
        let mut parser = serde_json::Deserializer::from_str(text);
        let rows =
            serde::Deserializer::deserialize_seq(&mut parser, Rows).map_err(|_| "table")??;
        parser.end().map_err(|_| "table")?;
        Ok(rows)
    } else {
        // 单个 JSON 对象可以跨行；失败后只接受逐行完整对象的 NDJSON。
        if let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(text) {
            return Ok(vec![value]);
        }
        let mut rows = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            if rows.len() == MAX_TABLE_ROWS {
                return Err("limit");
            }
            rows.push(serde_json::from_str(line).map_err(|_| "table")?);
        }
        Ok(rows)
    }
}

#[derive(Default)]
pub(crate) struct GitProgress {
    pub phases: Vec<Phase>,
    pub tail: Vec<String>,
    pending: [String; 2],
}

pub(crate) struct Phase {
    pub name: String,
    pub percent: Option<u8>,
    pub detail: String,
}

impl GitProgress {
    pub fn rows(&self) -> usize {
        self.phases.len() + self.tail.len() + self.pending_lines().count() + 1
    }
    pub fn pending_lines(&self) -> impl Iterator<Item = &str> {
        self.pending.iter().map(String::as_str).filter(|line| {
            let value = line.trim().strip_prefix("remote: ").unwrap_or(line.trim());
            !value.is_empty()
                && !GIT_PHASES
                    .iter()
                    .any(|phase| value.starts_with(phase) || phase.starts_with(value))
        })
    }
    pub fn feed(&mut self, text: &str, stderr: bool) {
        let stream = usize::from(stderr);
        for c in text.chars() {
            if matches!(c, '\r' | '\n') {
                let line = std::mem::take(&mut self.pending[stream]);
                self.line(line.trim());
            } else if self.pending[stream].len() < 8192 {
                self.pending[stream].push(c);
            }
        }
    }
    pub fn finish(&mut self) {
        for stream in 0..2 {
            let line = std::mem::take(&mut self.pending[stream]);
            self.line(line.trim());
        }
    }
    fn line(&mut self, line: &str) {
        let line = line.strip_prefix("remote: ").unwrap_or(line);
        if line.is_empty() {
            return;
        }
        if let Some(name) = GIT_PHASES
            .iter()
            .find(|name| line.strip_prefix(**name).is_some_and(|rest| rest.starts_with(':')))
        {
            let detail = line[name.len() + 1..].trim().to_owned();
            let percent = detail
                .split_once('%')
                .and_then(|(value, _)| value.trim().parse::<u8>().ok())
                .filter(|value| *value <= 100);
            if let Some(phase) = self.phases.iter_mut().find(|phase| phase.name == *name) {
                phase.percent = percent;
                phase.detail = detail;
            } else {
                self.phases.push(Phase { name: name.to_string(), percent, detail });
            }
        } else {
            self.tail.push(line.to_owned());
            if self.tail.len() > 6 {
                self.tail.remove(0);
            }
        }
    }
}

const GIT_PHASES: &[&str] = &[
    "Enumerating objects",
    "Counting objects",
    "Compressing objects",
    "Receiving objects",
    "Resolving deltas",
    "Updating files",
    "Checking out files",
    "Writing objects",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_git_short_failure_and_unterminated_phase_height() {
        let mut git = GitProgress::default();
        git.feed("fatal: missing repository\n", true);
        git.finish();
        assert_eq!(git.rows(), 2);
        let mut git = GitProgress::default();
        git.feed(
            "Counting objects: 100%\rCompressing objects: 100%\rReceiving objects: 100%",
            true,
        );
        assert_eq!(git.rows(), 3);
        git.finish();
        assert_eq!(git.rows(), 4);
        assert_eq!(git.phases.len(), 3);
    }
    #[test]
    fn ndjson_union_numeric_sort_and_unicode_filter() {
        let table = Table::parse(
            "{\"Name\":\"中文\",\"CPU\":\"2%\"}\n{\"Name\":\"api\",\"CPU\":\"10%\",\"PIDs\":8}",
        )
        .unwrap();
        assert_eq!(table.columns, ["Name", "CPU", "PIDs"]);
        assert_eq!(table.order("", Some((1, true))), [1, 0]);
        assert_eq!(table.order("中文", None), [0]);
        assert_eq!(Table::parse(r#"[{"Name":"only"}]"#).unwrap().columns, ["Name"]);
        assert!(Table::parse("[1,2]").is_err());
        assert!(Table::parse("{broken}").is_err());
        assert!(Table::parse(r#"{"a":1}{"a":2}"#).is_err());
    }
    #[test]
    fn git_chunk_boundaries_and_unknown_progress() {
        let mut git = GitProgress::default();
        git.feed("remote: Counting objects: 2", true);
        git.feed("partial stdout", false);
        git.feed("0% (2/10)\rremote: Counting objects: 100% (10/10), done.\nReceiving objects: 5% (1/20)\r",true);
        git.feed("fatal: example failure 80%\n", true);
        assert_eq!(git.phases.len(), 2);
        assert_eq!(git.phases[0].percent, Some(100));
        assert_eq!(git.phases[1].percent, Some(5));
        assert_eq!(git.tail, ["fatal: example failure 80%"]);
    }
    #[test]
    fn mixed_column_has_a_consistent_total_order() {
        let table = Table::parse(r#"[{"v":"2"},{"v":"10"},{"v":"1a"},{"v":"NaN"}]"#).unwrap();
        assert_eq!(table.order("", Some((0, false))), [0, 1, 2, 3]);
        assert_eq!(table.order("", Some((0, true))), [3, 2, 1, 0]);
    }

    #[test]
    fn large_tables_stop_before_materializing_unbounded_rows_or_cells() {
        let records = format!("[{}]", "{},".repeat(MAX_TABLE_ROWS) + "{}");
        assert!(matches!(Table::parse(&records), Err("limit")));
        let record =
            format!("{{{}}}", (0..64).map(|i| format!("\"k{i}\":0")).collect::<Vec<_>>().join(","));
        let many = format!("[{}]", vec![record; 4097].join(","));
        assert!(matches!(Table::parse(&many), Err("limit")));
        assert_eq!(Table::parse("{\n  \"a\": 2\n}").unwrap().rows, [["2"]]);
    }

    #[test]
    #[ignore = "manual native-result parse and ordering cost sample"]
    fn measure_native_result_large_inputs() {
        let listing = (0..10_000)
            .map(|i| format!("-rw-r--r-- 1 user staff {i} Oct 20 09:20 file-{i}.txt\n"))
            .collect::<String>();
        let json = format!(
            "[{}]",
            (0..10_000)
                .map(|i| format!("{{\"name\":\"file-{i}\",\"size\":{i},\"kind\":\"file\"}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        for (label, source, directory) in [("ls-long", &listing, true), ("json", &json, false)] {
            let started = std::time::Instant::now();
            let mut raw = RawText::default();
            raw.append(source);
            let raw_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = std::time::Instant::now();
            let table = if directory {
                Table::parse_directory(source, false)
            } else {
                Table::parse(source)
            }
            .unwrap();
            let parse_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = std::time::Instant::now();
            let order = table.order("file-9", Some((table.columns.len() - 1, true)));
            crate::gpui_shell::try_write_stderr(format_args!(
                "native-result sample {label}: bytes={} rows={} cells={} raw_ms={raw_ms:.3} parse_ms={parse_ms:.3} filter_sort_ms={:.3} matches={}\n",
                source.len(),
                table.rows.len(),
                table.rows.len() * table.columns.len(),
                started.elapsed().as_secs_f64() * 1000.0,
                order.len()
            ));
        }
    }
}
