//! 命令边界与显示源范围同序提交；保留真实 VT 网格，避免 ConPTY 光标与宿主分叉。
use super::{CommandView, MAX_CONTENT_BYTES, Marker, Registry, source_hash};
use crate::event::EventListener;
use crate::grid::Dimensions;
use crate::index::{Column, Line, Point};
use crate::term::{Term, TermMode};

#[derive(Default)]
pub(crate) struct CommandCapture {
    shell: Option<String>,
    pending: Option<CommandView>,
    active: Option<(CommandView, usize, u64)>,
}

impl CommandCapture {
    pub fn report<U: EventListener>(&mut self, name: &str, value: &str, host: &U) {
        if name == "pebrel_shell" {
            if self.shell.as_deref() != Some(value) {
                self.cancel();
            }
            self.shell = Some(value.to_owned());
        } else if name == "pebrel_command" {
            self.pending = value.split_once('\n').and_then(|(owner, command)| {
                (self.shell.as_deref() == Some(owner))
                    .then(|| host.command_view(owner, command))
                    .flatten()
            });
        }
    }

    pub fn start<U>(&mut self, term: &Term<U>, committed: bool) {
        self.active = self.pending.take().and_then(|view| {
            (committed && supported(term) && term.grid().cursor.point.column.0 == 0)
                .then(|| (view, term.nebula_cursor_abs_line(), term.reading_epoch()))
        });
    }

    pub fn observe<U>(&mut self, term: &Term<U>) {
        if self
            .active
            .as_ref()
            .is_some_and(|(_, _, epoch)| *epoch != term.reading_epoch() || !supported(term))
        {
            self.active = None;
        }
    }

    pub fn cancel(&mut self) {
        self.pending = None;
        self.active = None;
    }

    pub fn finish<U>(&mut self, term: &mut Term<U>, registry: &mut Registry, code: Option<i32>) {
        let Some((view, start, epoch)) = self.active.take() else { return };
        // 失败、重排、全屏应用和半行结束保留原始输出；不覆盖后续提示符。
        if code != Some(0)
            || epoch != term.reading_epoch()
            || !supported(term)
            || term.grid().cursor.point.column.0 != 0
            || start < term.grid().scrolled_out()
        {
            return;
        }
        let end = term.nebula_cursor_abs_line();
        let rows = end.saturating_sub(start);
        if rows == 0
            || rows > u16::MAX as usize
            || rows.saturating_mul(term.columns()) > 1024 * 1024
        {
            return;
        }
        let floor = term.grid().scrolled_out() + term.history_size();
        let first = Line((start as i64 - floor as i64) as i32);
        let mut text = String::new();
        let mut hashes = Vec::with_capacity(rows);
        for row in 0..rows {
            let line = first + row;
            if term.grid()[line][..].iter().any(|cell| cell.inline_marker().is_some()) {
                return;
            }
            // 每个物理行只消费一次；选区 API 会补入下一行宽字，逐行相加会重复。
            let part = term.line_to_string(line, Column(0)..term.last_column(), false);
            if text.len().saturating_add(part.len()) > MAX_CONTENT_BYTES {
                return;
            }
            text.push_str(&part);
            hashes.push(source_hash(&term.grid()[line][..]));
        }
        if text.trim().is_empty() {
            return;
        }
        let surface = registry.capture(view, text, hashes, 0);
        // 只增加标记，字符、VT 光标、历史和 PTY 尺寸均保留原值。
        let grid = term.grid_mut();
        for row in 0..rows {
            grid[Point::new(first + row, Column(0))]
                .set_inline_marker(Marker { surface: surface.clone(), row: row as u16 });
        }
    }
}

fn supported<U>(term: &Term<U>) -> bool {
    !term.mode().intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE | TermMode::APP_KEYPAD)
}
