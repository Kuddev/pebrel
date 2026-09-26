//! 同步重绘后的保守阅读定位；只查找新网格，不恢复被应用清掉的内容。
use super::{Cell, EventListener, Flags, Term, TermMode};
use crate::grid::{Dimensions, Grid, Scroll};
use crate::index::{Column, Line};

const ANCHOR_ROWS: usize = 3;
const MAX_COLUMNS: usize = 1024;
const MAX_SCAN_CELLS: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub(super) struct RedrawAnchor {
    enabled: bool,
    active: bool,
    rows: Option<Vec<String>>,
    cleared_viewport: bool,
    cleared_history: bool,
}

impl<T> Term<T> {
    /// 由宿主根据已确认的应用生命周期选择；默认不改变终端滚动语义。
    pub fn set_redraw_anchor_enabled(&mut self, enabled: bool) {
        if self.redraw_anchor.enabled != enabled {
            self.cancel_redraw_anchor();
            self.redraw_anchor.enabled = enabled;
        }
    }

    /// 用户操作或不完整同步更新优先，迟到的输出不得恢复旧阅读位置。
    pub fn cancel_redraw_anchor(&mut self) {
        self.redraw_anchor.rows = None;
    }

    pub(super) fn begin_redraw_anchor(&mut self) {
        let previous_active = self.redraw_anchor.active;
        self.redraw_anchor = RedrawAnchor {
            enabled: self.redraw_anchor.enabled,
            active: true,
            ..Default::default()
        };
        if previous_active
            || !self.redraw_anchor.enabled
            || self.grid.display_offset() == 0
            || self.mode.intersects(TermMode::ALT_SCREEN | TermMode::VI)
            || self.selection.is_some()
            || self.columns() > MAX_COLUMNS
            || self.screen_lines() < ANCHOR_ROWS
        {
            return;
        }
        let start = -(self.grid.display_offset() as i32);
        let rows: Option<Vec<_>> = (0..ANCHOR_ROWS)
            .map(|offset| row_key(&self.grid, Line(start + offset as i32)))
            .collect();
        self.redraw_anchor.rows = rows.filter(|rows| rows.iter().any(|row| !row.is_empty()));
    }

    pub(super) fn observe_redraw_clear(&mut self, mode: &super::ansi::ClearMode) {
        if !self.redraw_anchor.active {
            return;
        }
        match mode {
            super::ansi::ClearMode::All => self.redraw_anchor.cleared_viewport = true,
            super::ansi::ClearMode::Saved => self.redraw_anchor.cleared_history = true,
            _ => (),
        }
    }
}

impl<T: EventListener> Term<T> {
    pub(super) fn finish_redraw_anchor(&mut self) {
        let enabled = self.redraw_anchor.enabled;
        let state = std::mem::replace(
            &mut self.redraw_anchor,
            RedrawAnchor { enabled, ..Default::default() },
        );
        let Some(rows) = state.rows else { return };
        if !state.cleared_viewport
            || !state.cleared_history
            || self.mode.intersects(TermMode::ALT_SCREEN | TermMode::VI)
            || self.selection.is_some()
            || self.grid.total_lines().saturating_mul(self.columns()) > MAX_SCAN_CELLS
        {
            return;
        }
        let start = -(self.history_size() as i32);
        let end = self.screen_lines() as i32 - ANCHOR_ROWS as i32;
        let mut matched = None;
        for line in start..=end {
            if rows.iter().enumerate().all(|(offset, expected)| {
                row_key(&self.grid, Line(line + offset as i32)).as_ref() == Some(expected)
            }) {
                if matched.is_some() {
                    return;
                }
                matched = Some(line);
            }
        }
        if let Some(line) = matched.filter(|line| *line <= 0) {
            self.scroll_display(Scroll::Bottom);
            self.scroll_display(Scroll::Delta(-line));
        }
    }
}

fn row_key(grid: &Grid<Cell>, line: Line) -> Option<String> {
    let mut text = String::new();
    let mut space = false;
    for column in 0..grid.columns() {
        let cell = &grid[line][Column(column)];
        // 换行重排没有可靠的行身份，不以不完整片段猜测原位置。
        if cell.flags.contains(Flags::WRAPLINE) {
            return None;
        }
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        if cell.c == ' ' && cell.zerowidth().is_none() {
            space = !text.is_empty();
            continue;
        }
        if space {
            text.push(' ');
            space = false;
        }
        text.push(cell.c);
        if let Some(chars) = cell.zerowidth() {
            text.extend(chars);
        }
    }
    // Pi 表格的纯边框随列宽伸长；只归一纯边框，不折叠正文中的重复字符。
    if text.contains('─')
        && text
            .chars()
            .all(|c| matches!(c, '─' | '┌' | '┬' | '┐' | '├' | '┼' | '┤' | '└' | '┴' | '┘'))
    {
        let mut previous = None;
        text.retain(|c| {
            let keep = c != '─' || previous != Some(c);
            previous = Some(c);
            keep
        });
    }
    Some(text)
}
