//! 逐终端的有界命令显示记录；调用方只提交已确认的 shell 命令。

use nebula_terminal::grid::snapshot::DisplaySnapshot;
use nebula_terminal::grid::{Dimensions, Grid};
use nebula_terminal::index::Line;
use nebula_terminal::term::cell::{Cell, Flags};
use serde::{Deserialize, Serialize};

pub(crate) mod storage;

const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_CAPTURE_CELLS: usize = 4096;

// 仅在保存边界编码，按整行缩小，避免切断 UTF-8 或宽字符。
fn capture_bounded(
    grid: &Grid<Cell>,
    range: std::ops::Range<Line>,
    byte_budget: usize,
) -> DisplaySnapshot {
    let mut low = 0;
    let mut high = MAX_CAPTURE_CELLS / grid.columns();
    let mut best = DisplaySnapshot::capture(grid, range.clone(), 0);
    while low <= high {
        let rows = low + (high - low) / 2;
        let candidate = DisplaySnapshot::capture(grid, range.clone(), rows * grid.columns());
        if serde_json::to_vec(&candidate).is_ok_and(|bytes| bytes.len() <= byte_budget) {
            best = candidate;
            low = rows + 1;
        } else if rows == 0 {
            break;
        } else {
            high = rows - 1;
        }
    }
    best
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandRecord {
    #[serde(default)]
    pub(crate) captured_at: u64,
    prompt: DisplaySnapshot,
    output: DisplaySnapshot,
}

#[derive(Default)]
pub(crate) struct RecentOutput {
    records: Vec<CommandRecord>,
    active_output_start: Option<usize>,
}

impl RecentOutput {
    pub(crate) fn begin(&mut self, grid: &Grid<Cell>) {
        let mut start = grid.cursor.point.line;
        while start.0 > -(grid.history_size() as i32)
            && grid[Line(start.0 - 1)][nebula_terminal::index::Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        {
            start.0 -= 1;
        }
        let end = Line(grid.cursor.point.line.0 + 1);
        if self.records.len() == 5 {
            self.records.remove(0);
        }
        self.records.push(CommandRecord {
            captured_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_micros().min(u64::MAX as u128) as u64),
            prompt: capture_bounded(grid, start..end, MAX_RECORD_BYTES / 2),
            output: DisplaySnapshot::capture(grid, end..end, 0),
        });
        self.active_output_start = Some(grid.scrolled_out() + grid.history_size() + end.0 as usize);
    }

    pub(crate) fn finish(&mut self, grid: &Grid<Cell>) {
        if let Some(output) = self.capture_active(grid, false) {
            if let Some(record) = self.records.last_mut() {
                record.output = output;
            }
        }
        self.active_output_start = None;
    }

    pub(crate) fn snapshot(&self, grid: &Grid<Cell>) -> Vec<CommandRecord> {
        let mut records = self.records.clone();
        if let Some(output) = self.capture_active(grid, true) {
            if let Some(record) = records.last_mut() {
                record.output = output;
            }
        }
        records
    }

    fn capture_active(&self, grid: &Grid<Cell>, include_cursor: bool) -> Option<DisplaySnapshot> {
        let absolute = self.active_output_start?;
        let origin = grid.scrolled_out() + grid.history_size();
        let start = (absolute as i128 - origin as i128).clamp(i32::MIN as i128, i32::MAX as i128);
        let end = grid.cursor.point.line.0 + i32::from(include_cursor);
        let prompt_bytes = serde_json::to_vec(&self.records.last()?.prompt).ok()?.len();
        // 为字段名、标点及采集时间预留空间，确保整条记录仍在预算内。
        let budget = MAX_RECORD_BYTES.saturating_sub(prompt_bytes + 64);
        Some(capture_bounded(grid, Line(start as i32)..Line(end), budget))
    }
    pub(crate) fn try_from_records(records: Vec<CommandRecord>) -> Result<Self, &'static str> {
        Self::validate_records(&records)?;
        Ok(Self { records, active_output_start: None })
    }

    pub(crate) fn validate_records(records: &[CommandRecord]) -> Result<(), &'static str> {
        if records.len() > 5 {
            return Err("too many command records");
        }
        for record in records {
            let bytes = serde_json::to_vec(record).map_err(|_| "invalid command record")?;
            if bytes.len() > MAX_RECORD_BYTES {
                return Err("command record exceeds byte budget");
            }
        }
        Ok(())
    }
    pub(crate) fn restore(&self, grid: &mut Grid<Cell>) -> Result<(), &'static str> {
        // 每次静态导入都插在现有历史之前，因此从最新记录的输出开始。
        for record in self.records.iter().rev() {
            record.output.restore_scrollback(grid)?;
            record.prompt.restore_scrollback(grid)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_terminal::event::VoidListener;
    use nebula_terminal::grid::Dimensions;
    use nebula_terminal::index::{Column, Line};
    use nebula_terminal::term::{Config, Term};
    use nebula_terminal::vte::ansi;

    pub(super) struct Size;
    impl Dimensions for Size {
        fn total_lines(&self) -> usize {
            4
        }
        fn screen_lines(&self) -> usize {
            4
        }
        fn columns(&self) -> usize {
            32
        }
    }

    #[test]
    fn retains_five_commands_with_output_in_order_without_reexecuting() {
        let mut term = Term::new(Config::default(), &Size, VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut recent = RecentOutput::default();
        for index in 0..7 {
            parser.advance(&mut term, format!("C:>echo {index}").as_bytes());
            recent.begin(term.grid());
            parser.advance(&mut term, format!("\r\nresult{index}\r\n").as_bytes());
            recent.finish(term.grid());
        }
        let records = recent.snapshot(term.grid());
        assert_eq!(records.len(), 5);
        let mut target = Term::new(Config::default(), &Size, VoidListener);
        RecentOutput::try_from_records(records).unwrap().restore(target.grid_mut()).unwrap();
        let text = target.bounds_to_string(
            nebula_terminal::index::Point::new(
                Line(-(target.grid().history_size() as i32)),
                Column(0),
            ),
            nebula_terminal::index::Point::new(Line(-1), Column(31)),
        );
        assert!(!text.contains("echo 1"));
        assert!(text.contains("echo 2"));
        assert!(text.contains("result6"));
        assert!(text.find("result2").unwrap() < text.find("result6").unwrap());
        assert_eq!(target.grid().cursor.point.line, Line(0));
        assert_eq!(target.grid()[Line(0)][Column(0)].c, ' ');
    }

    #[test]
    fn loading_rejects_excess_records_instead_of_growing_on_next_command() {
        let mut term = Term::new(Config::default(), &Size, VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, b"C:>echo test");
        let mut recent = RecentOutput::default();
        recent.begin(term.grid());
        let record = recent.snapshot(term.grid()).remove(0);
        assert!(RecentOutput::try_from_records(vec![record; 6]).is_err());
    }

    #[test]
    fn loading_rejects_oversize_records_before_restore() {
        let mut term = Term::new(Config::default(), &Size, VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        for _ in 0..200 {
            parser.advance(&mut term, b"long unbounded output\r\n");
        }
        let snapshot = DisplaySnapshot::capture(term.grid(), Line(-200)..Line(0), 6400);
        let record = CommandRecord { captured_at: 0, prompt: snapshot.clone(), output: snapshot };
        assert!(serde_json::to_vec(&record).unwrap().len() > MAX_RECORD_BYTES);
        assert!(RecentOutput::try_from_records(vec![record]).is_err());
    }

    #[test]
    fn encoded_record_keeps_command_and_tail_within_256_kib() {
        let mut term = Term::new(Config::default(), &Size, VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, b"C:>large-tool");
        let mut recent = RecentOutput::default();
        recent.begin(term.grid());
        for index in 0..300 {
            parser.advance(
                &mut term,
                format!("\r\n\x1b[38;2;123;234;245mrow{index:03} output\x1b[0m").as_bytes(),
            );
        }
        let records = recent.snapshot(term.grid());
        let encoded = serde_json::to_vec(&records[0]).unwrap();
        assert!(encoded.len() <= 256 * 1024, "{} encoded bytes", encoded.len());
        let mut target = Term::new(Config::default(), &Size, VoidListener);
        RecentOutput::try_from_records(records).unwrap().restore(target.grid_mut()).unwrap();
        let text = target.bounds_to_string(
            nebula_terminal::index::Point::new(
                Line(-(target.grid().history_size() as i32)),
                Column(0),
            ),
            nebula_terminal::index::Point::new(Line(-1), Column(31)),
        );
        assert!(text.contains("C:>large-tool"));
        assert!(text.contains("row299 output"));
        assert!(!text.contains("row000 output"));
    }

    #[test]
    fn active_output_is_captured_and_two_restores_do_not_duplicate_records() {
        let mut term = Term::new(Config::default(), &Size, VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, b"C:>tool");
        let mut recent = RecentOutput::default();
        recent.begin(term.grid());
        parser.advance(&mut term, b"\r\npartial");
        let records = recent.snapshot(term.grid());
        assert_eq!(records.len(), 1);
        let encoded = serde_json::to_vec(&records).unwrap();
        let restored =
            RecentOutput::try_from_records(serde_json::from_slice(&encoded).unwrap()).unwrap();
        let empty = Term::new(Config::default(), &Size, VoidListener);
        let encoded_again = serde_json::to_vec(&restored.snapshot(empty.grid())).unwrap();
        assert_eq!(encoded_again, encoded);
    }
}
