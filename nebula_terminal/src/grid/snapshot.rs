//! 安全显示数据快照：不保存终端协议、链接动作或运行状态。

use super::{Dimensions, Grid};
use crate::index::{Column, Line};
use crate::term::cell::Cell;

#[cfg(feature = "serde")]
mod serialization;

// 解码后的防灾上限；持久化层还须独立限制每条记录的编码字节数。
const MAX_SNAPSHOT_CELLS: usize = 262_144;

#[derive(Clone, Debug, Default)]
pub struct DisplaySnapshot {
    columns: usize,
    rows: Vec<Vec<Cell>>,
}

impl DisplaySnapshot {
    pub fn capture(grid: &Grid<Cell>, range: std::ops::Range<Line>, max_cells: usize) -> Self {
        let max_cells = max_cells.min(MAX_SNAPSHOT_CELLS);
        let columns = grid.columns();
        let end = range.end.0.min(grid.screen_lines() as i32);
        let start = range.start.0.max(-(grid.history_size() as i32));
        let start =
            start.max(end.saturating_sub((max_cells / columns).min(i32::MAX as usize) as i32));
        let rows = (start..end)
            .map(|line| {
                (0..columns)
                    .map(|column| {
                        let mut cell = grid[Line(line)][Column(column)].clone();
                        cell.set_hyperlink(None);
                        cell
                    })
                    .collect()
            })
            .collect();
        Self { columns, rows }
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.columns == 0
            || self.rows.len() > MAX_SNAPSHOT_CELLS / self.columns
            || self.rows.iter().any(|row| {
                row.len() != self.columns
                    || row.iter().any(|cell| {
                        (cell.c.is_control() && cell.c != '\t')
                            || cell.hyperlink().is_some()
                            || cell
                                .zerowidth()
                                .is_some_and(|chars| chars.iter().any(|c| c.is_control()))
                    })
            })
        {
            return Err("invalid display snapshot");
        }
        Ok(())
    }

    pub fn restore_scrollback(&self, grid: &mut Grid<Cell>) -> Result<(), &'static str> {
        self.validate()?;
        if self.rows.is_empty() {
            return Ok(());
        }
        if self.columns != grid.columns() {
            // 只重排快照，不 resize 活终端；复用已有宽字符/软换行规则。
            let reflow_capacity = self.rows.len().saturating_mul(self.columns);
            let mut staging = Grid::<Cell>::new(1, self.columns, reflow_capacity);
            self.restore_scrollback(&mut staging)?;
            staging.resize(true, 1, grid.columns());
            let reflowed = Self::capture(
                &staging,
                Line(-(staging.history_size() as i32))..Line(0),
                usize::MAX,
            );
            return reflowed.restore_scrollback(grid);
        }
        let old_history = grid.history_size();
        let count = self.rows.len().min(grid.max_scroll_limit.saturating_sub(old_history));
        grid.raw.initialize(count, grid.columns());
        let top = -((old_history + count) as i32);
        for (index, row) in self.rows.iter().rev().take(count).rev().enumerate() {
            for (column, cell) in row.iter().enumerate() {
                grid[Line(top + index as i32)][Column(column)] = cell.clone();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::VoidListener;
    use crate::term::{Config, Term, test::TermSize};
    use crate::vte::ansi;

    #[test]
    fn captures_primary_screen_while_alternate_screen_is_active() {
        let mut term = Term::new(Config::default(), &TermSize::new(8, 2), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, b"main");
        parser.advance(&mut term, b"\x1b[?1049halt");
        assert_eq!(term.grid()[Line(0)][Column(4)].c, 'a');

        let snapshot = DisplaySnapshot::capture(term.primary_grid(), Line(0)..Line(1), 8);
        let mut target = Grid::<Cell>::new(2, 8, 10);
        snapshot.restore_scrollback(&mut target).unwrap();
        assert_eq!(target[Line(-1)][Column(0)].c, 'm');
        assert_eq!(target[Line(-1)][Column(1)].c, 'a');
    }

    #[test]
    fn restores_reflowed_unicode_before_existing_history_without_touching_prompt() {
        let mut source = Term::new(Config::default(), &TermSize::new(8, 3), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut source, "ab中文e\u{301}XYZ\r\n".as_bytes());
        let snapshot = DisplaySnapshot::capture(source.grid(), Line(0)..Line(2), 100);
        let mut target = Term::new(Config::default(), &TermSize::new(5, 2), VoidListener);
        parser.advance(&mut target, b"keep\r\nlast\r\nnew>");
        snapshot.restore_scrollback(target.grid_mut()).unwrap();
        assert_eq!(target.grid().history_size(), 4);
        let text: String = (-4..-1)
            .flat_map(|line| (0..5).map(move |col| (line, col)))
            .filter_map(|(line, col)| {
                let cell = &target.grid()[Line(line)][Column(col)];
                (!cell.flags.intersects(
                    crate::term::cell::Flags::WIDE_CHAR_SPACER
                        | crate::term::cell::Flags::LEADING_WIDE_CHAR_SPACER,
                ) && cell.c != ' ')
                    .then_some(cell.c)
            })
            .collect();
        assert_eq!(text, "ab中文eXYZ");
        assert_eq!(target.grid()[Line(-3)][Column(2)].zerowidth(), Some(&['\u{301}'][..]));
        assert_eq!(target.grid()[Line(-1)][Column(0)].c, 'k');
        assert_eq!(target.grid()[Line(1)][Column(0)].c, 'n');
        assert_eq!(target.grid().cursor.point, crate::index::Point::new(Line(1), Column(4)));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serialized_snapshot_round_trips_display_only_and_rejects_actions() {
        let mut source = Term::new(Config::default(), &TermSize::new(4, 2), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut source, "\x1b[38;2;12;34;56me\u{301}中".as_bytes());
        let snapshot = DisplaySnapshot::capture(source.grid(), Line(0)..Line(1), 4);
        let value = serde_json::to_value(&snapshot).unwrap();
        let decoded: DisplaySnapshot = serde_json::from_value(value.clone()).unwrap();
        let mut target = Grid::<Cell>::new(2, 4, 10);
        decoded.restore_scrollback(&mut target).unwrap();
        assert_eq!(target[Line(-1)][Column(0)].zerowidth(), Some(&['\u{301}'][..]));
        assert_eq!(target[Line(-1)][Column(1)].c, '中');
        assert_eq!(
            target[Line(-1)][Column(0)].fg,
            ansi::Color::Spec(crate::vte::ansi::Rgb { r: 12, g: 34, b: 56 })
        );
        assert!(value["rows"][0][0].get("extra").is_none());
        let mut invalid = value;
        invalid["rows"][0][0]["hyperlink"] = serde_json::json!("https://example.invalid");
        assert!(serde_json::from_value::<DisplaySnapshot>(invalid).is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn deserialization_rejects_invalid_dimensions_and_control_data() {
        for value in [
            serde_json::json!({ "columns": 0, "rows": [[]] }),
            serde_json::json!({ "columns": 4, "rows": [[]] }),
            serde_json::json!({ "columns": 262145, "rows": [[]] }),
        ] {
            assert!(serde_json::from_value::<DisplaySnapshot>(value).is_err());
        }
        let snapshot = DisplaySnapshot {
            columns: 1,
            rows: vec![vec![Cell { c: '\u{1b}', ..Cell::default() }]],
        };
        let value = serde_json::to_value(&snapshot).unwrap();
        assert!(serde_json::from_value::<DisplaySnapshot>(value).is_err());
    }

    #[test]
    fn rejects_malformed_rows_before_changing_live_grid() {
        let mut target = Grid::<Cell>::new(2, 4, 10);
        target[Line(0)][Column(0)].c = 'x';
        let snapshot = DisplaySnapshot { columns: 4, rows: vec![vec![Cell::default(); 5]] };
        assert!(snapshot.restore_scrollback(&mut target).is_err());
        assert_eq!(target.history_size(), 0);
        assert_eq!(target[Line(0)][Column(0)].c, 'x');
    }

    #[test]
    fn rejects_actions_and_control_characters_before_restoring() {
        let mut target = Grid::<Cell>::new(2, 4, 10);
        for cell in [Cell { c: '\u{1b}', ..Cell::default() }, {
            let mut cell = Cell::default();
            cell.set_hyperlink(Some(crate::term::cell::Hyperlink::new(
                None::<String>,
                "https://example.invalid".into(),
            )));
            cell
        }] {
            let snapshot = DisplaySnapshot { columns: 4, rows: vec![vec![cell; 4]] };
            assert!(snapshot.restore_scrollback(&mut target).is_err());
            assert_eq!(target.history_size(), 0);
        }
    }

    #[test]
    fn restores_parser_tab_cells_as_inert_display_data() {
        let mut source = Term::new(Config::default(), &TermSize::new(12, 2), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut source, b"a\tb");
        let snapshot = DisplaySnapshot::capture(source.grid(), Line(0)..Line(1), 12);
        let mut target = Grid::<Cell>::new(2, 12, 10);
        snapshot.restore_scrollback(&mut target).unwrap();
        assert_eq!(target[Line(-1)][Column(0)].c, 'a');
        assert_eq!(target[Line(-1)][Column(8)].c, 'b');
    }

    #[test]
    fn rejects_oversized_snapshot_before_allocating_history() {
        let snapshot = DisplaySnapshot { columns: 1, rows: vec![vec![Cell::default()]; 262_145] };
        let mut target = Grid::<Cell>::new(2, 4, 10);
        assert!(snapshot.restore_scrollback(&mut target).is_err());
        assert_eq!(target.history_size(), 0);
    }

    #[test]
    fn capture_keeps_only_complete_tail_rows_within_budget() {
        let mut source = Term::new(Config::default(), &TermSize::new(4, 3), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut source, b"one\r\ntwo\r\nend");
        let snapshot = DisplaySnapshot::capture(source.grid(), Line(-100)..Line(100), 9);
        let mut target = Grid::<Cell>::new(2, 4, 10);
        snapshot.restore_scrollback(&mut target).unwrap();
        assert_eq!(target.history_size(), 2);
        assert_eq!(target[Line(-2)][Column(0)].c, 't');
        assert_eq!(target[Line(-1)][Column(0)].c, 'e');
        let empty = DisplaySnapshot::capture(source.grid(), Line(2)..Line(1), 9);
        empty.restore_scrollback(&mut target).unwrap();
        assert_eq!(target.history_size(), 2);
    }

    #[test]
    fn restores_display_without_replaying_hyperlink_or_changing_live_screen() {
        let mut source = Term::new(Config::default(), &TermSize::new(12, 3), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(
            &mut source,
            b"\x1b]8;;https://example.invalid\x1b\\\x1b[31mold\x1b]8;;\x1b\\\r\n",
        );
        let snapshot = DisplaySnapshot::capture(source.grid(), Line(0)..Line(1), 100);
        let mut target = Term::new(Config::default(), &TermSize::new(12, 3), VoidListener);
        parser.advance(&mut target, b"new>");
        let cursor = target.grid().cursor.clone();
        snapshot.restore_scrollback(target.grid_mut()).unwrap();
        assert_eq!(target.grid().history_size(), 1);
        assert_eq!(target.grid()[Line(-1)][Column(0)].c, 'o');
        assert_eq!(
            target.grid()[Line(-1)][Column(0)].fg,
            ansi::Color::Named(ansi::NamedColor::Red)
        );
        assert!(target.grid()[Line(-1)][Column(0)].hyperlink().is_none());
        assert_eq!(target.grid()[Line(0)][Column(0)].c, 'n');
        assert_eq!(target.grid().cursor.point, cursor.point);
        assert_eq!(target.grid().cursor.template, cursor.template);
    }
}
