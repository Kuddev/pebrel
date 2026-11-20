//! 有界、只读的显示文本查询。地址只在当前会话 epoch 内有效，不代表原始 stdout。
//!
//! `text` 保留源空格，软折行不插入换行。调用方在锁外合并同一物理行的碎片，
//! 再按现有选区复制语义去掉硬行尾 ASCII 空格；不可逐块 trim，否则会丢掉内部空格。
use std::collections::VecDeque;
use std::fmt;

use super::cell::Flags;
use super::{Term, TermMode};
use crate::grid::Dimensions;
use crate::index::{Column, Line};

mod guard;
use guard::TailGuard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReadingPoint {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadingSpan {
    pub epoch: u64,
    pub start: ReadingPoint,
    pub end: ReadingPoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingCell {
    pub byte_start: usize,
    pub byte_end: usize,
    pub column: usize,
    pub width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingRow {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub text: String,
    pub cells: Vec<ReadingCell>,
    /// 仅跨过完整物理行且它不是软折行时为真；text 本身不含此换行。
    pub hard_break: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingChunk {
    pub rows: Vec<ReadingRow>,
    pub next: Option<ReadingPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedCommand {
    pub id: u64,
    /// B→C 的屏幕显示文本，不是 shell AST，也不能直接用于重新执行。
    pub command: String,
    pub exit_code: Option<i32>,
    pub output: ReadingSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingError {
    StaleEpoch,
    Evicted,
    InvalidRange,
    InvalidCursor,
    BudgetTooSmall,
    CommandNotFound,
    ContentChanged,
    UnsupportedScreen,
}

impl fmt::Display for ReadingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReadingError {}

// 对恶意宽网格/组合字符也有硬上限；调用方预算只能进一步缩小它。
const CHUNK_CELLS: usize = 4096;
const CHUNK_BYTES: usize = 32768;
const COMMANDS: usize = 32;
const METADATA_BYTES: usize = 1024 * 1024;

#[derive(Default)]
enum Execution {
    #[default]
    Idle,
    Prompt,
    Input(ReadingPoint),
    Output {
        start: ReadingPoint,
        command: String,
    },
}

struct CommandRecord {
    command: CompletedCommand,
    guard: TailGuard,
    cost: usize,
}

/// prompt 与执行标记共享同一网格生命周期，避免 Term 持有零散关联字段。
#[derive(Default)]
pub(super) struct ReadingState {
    /// OSC 133;A 的主屏绝对行号，严格递增；prompt 模块按保留历史惰性清理。
    pub(super) prompt_marks: VecDeque<usize>,
    /// 原有补全/输入边界状态；查询接口不改变这些状态或终端模式。
    pub(super) prompt_active: bool,
    pub(super) prompt_input: Option<(usize, Column)>,
    epoch: u64,
    next_id: u64,
    execution: Execution,
    commands: VecDeque<CommandRecord>,
    metadata: usize,
}

impl ReadingState {
    pub(super) fn invalidate(&mut self) {
        self.epoch = self.epoch.checked_add(1).expect("reading epoch exhausted");
        self.execution = Execution::Idle;
        self.commands.clear();
        self.metadata = 0;
    }
}

impl<T> Term<T> {
    pub fn reading_epoch(&self) -> u64 {
        self.reading.epoch
    }

    /// 当前保留网格的半开范围；alt-screen 只包含自己的网格，不混入主屏历史。
    pub fn reading_span(&self) -> ReadingSpan {
        let floor = self.grid.scrolled_out();
        ReadingSpan {
            epoch: self.reading_epoch(),
            start: ReadingPoint { line: floor, column: 0 },
            end: ReadingPoint { line: floor + self.grid.total_lines(), column: 0 },
        }
    }

    fn reading_cursor(&self) -> ReadingPoint {
        ReadingPoint {
            line: self.nebula_cursor_abs_line(),
            // 待自动换行不是已产生的换行；否则空输出会多复制一个换行。
            column: if self.grid.cursor.input_needs_wrap {
                self.columns()
            } else {
                self.grid.cursor.point.column.0
            },
        }
    }

    fn validate_reading_span(&self, span: ReadingSpan) -> Result<(), ReadingError> {
        if span.epoch != self.reading_epoch() {
            return Err(ReadingError::StaleEpoch);
        }
        let retained = self.reading_span();
        if span.start.line < retained.start.line {
            return Err(ReadingError::Evicted);
        }
        if span.start > span.end
            || span.end > retained.end
            || span.start.column > self.columns()
            || span.end.column > self.columns()
        {
            return Err(ReadingError::InvalidRange);
        }
        Ok(())
    }

    /// 每次只检查预算内的物理 cell（含 spacer），不扫描整行或整个历史。
    /// 零宽序列不能拆开：预算容不下一个完整 cell 时返回 BudgetTooSmall。
    /// 命令范围只校验本块相交的尾部；复制开始和最终提交前仍须 command_output_span 全检。
    pub fn reading_chunk(
        &self,
        span: ReadingSpan,
        cursor: Option<ReadingPoint>,
        max_cells: usize,
        max_bytes: usize,
    ) -> Result<ReadingChunk, ReadingError> {
        let chunk = self.reading_chunk_raw(span, cursor, max_cells, max_bytes)?;
        if let Some(record) = self.reading.commands.iter().find(|r| r.command.output == span) {
            let read = ReadingSpan {
                start: cursor.unwrap_or(span.start),
                end: chunk.next.unwrap_or(span.end),
                ..span
            };
            record.guard.validate(self, read)?;
        }
        Ok(chunk)
    }

    fn reading_chunk_raw(
        &self,
        span: ReadingSpan,
        cursor: Option<ReadingPoint>,
        max_cells: usize,
        max_bytes: usize,
    ) -> Result<ReadingChunk, ReadingError> {
        self.validate_reading_span(span)?;
        let mut point = cursor.unwrap_or(span.start);
        if point < span.start || point > span.end || point.column > self.columns() {
            return Err(ReadingError::InvalidCursor);
        }
        if point == span.end {
            return Ok(ReadingChunk { rows: Vec::new(), next: None });
        }
        let max_cells = max_cells.min(CHUNK_CELLS);
        let max_bytes = max_bytes.min(CHUNK_BYTES);
        if max_cells == 0 || max_bytes == 0 {
            return Err(ReadingError::BudgetTooSmall);
        }
        let mut rows = Vec::new();
        let mut consumed = 0;
        let mut bytes = 0;
        while point < span.end && consumed < max_cells {
            let relative = point.line as i64
                - self.grid.scrolled_out() as i64
                - self.grid.history_size() as i64;
            let source = &self.grid[Line(relative as i32)];
            let limit = if point.line == span.end.line { span.end.column } else { self.columns() };
            let mut row = ReadingRow {
                line: point.line,
                start_column: point.column,
                end_column: point.column,
                text: String::new(),
                cells: Vec::new(),
                hard_break: false,
            };
            while point.column < limit && consumed < max_cells {
                let cell = &source[Column(point.column)];
                let spacer = cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
                let zero = cell.zerowidth().unwrap_or_default();
                // 先 O(1) 拒绝过长的组合序列，再只在剩余字节预算内求长度。
                let available = max_bytes - bytes;
                let needed = if spacer {
                    0
                } else if zero.len() > available {
                    available + 1
                } else {
                    cell.c.len_utf8() + zero.iter().map(|c| c.len_utf8()).sum::<usize>()
                };
                if needed > available {
                    break;
                }
                if !spacer {
                    let start = row.text.len();
                    row.text.push(cell.c);
                    row.text.extend(zero.iter().copied());
                    row.cells.push(ReadingCell {
                        byte_start: start,
                        byte_end: row.text.len(),
                        column: point.column,
                        width: if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 },
                    });
                }
                bytes += needed;
                consumed += 1;
                point.column += 1;
                row.end_column = point.column;
            }
            // col==width 是合法续读点：硬换行占一字节预算，不能随最后一个 cell 超支。
            if point.column == self.columns() && point < span.end {
                let hard = !source[Column(self.columns() - 1)].flags.contains(Flags::WRAPLINE);
                if !hard || bytes < max_bytes {
                    row.hard_break = hard;
                    bytes += usize::from(hard);
                    point = ReadingPoint { line: point.line + 1, column: 0 };
                    // 即使从 col==width 开始，也不允许零成本遍历无限行。
                    if row.start_column == row.end_column {
                        consumed += 1;
                    }
                }
            }
            let progressed = row.start_column != row.end_column || point.line != row.line;
            if progressed {
                rows.push(row);
            } else {
                if rows.is_empty() {
                    return Err(ReadingError::BudgetTooSmall);
                }
                break;
            }
            if point.column != 0 || bytes == max_bytes {
                break;
            }
        }
        Ok(ReadingChunk { rows, next: (point < span.end).then_some(point) })
    }

    /// 只借用有界菜单元数据；可用性以 command_output_span 的实时校验为准。
    pub fn completed_commands(&self) -> impl DoubleEndedIterator<Item = &CompletedCommand> {
        self.reading.commands.iter().map(|record| &record.command)
    }

    pub fn command_output_span(&self, id: u64) -> Result<ReadingSpan, ReadingError> {
        if self.mode.contains(TermMode::ALT_SCREEN) {
            return Err(ReadingError::UnsupportedScreen);
        }
        let record = self
            .reading
            .commands
            .iter()
            .find(|r| r.command.id == id)
            .ok_or(ReadingError::CommandNotFound)?;
        self.validate_reading_span(record.command.output)?;
        record.guard.validate(self, record.guard.span)?;
        Ok(record.command.output)
    }

    /// StreamProcessor 已按 wire order 提交 parser slice 后才调用；sync 未提交时拒绝猜坐标。
    pub(crate) fn reading_shell_marker(
        &mut self,
        marker: u8,
        exit_code: Option<i32>,
        committed: bool,
    ) {
        if !committed || self.mode.contains(TermMode::ALT_SCREEN) {
            self.reading.execution = Execution::Idle;
            return;
        }
        let previous = std::mem::take(&mut self.reading.execution);
        self.reading.execution = match (marker, previous) {
            (b'A', Execution::Idle) => Execution::Prompt,
            (b'B', Execution::Prompt) => Execution::Input(self.reading_cursor()),
            (b'C', Execution::Input(input)) => {
                let start = self.reading_cursor();
                let label = ReadingSpan { epoch: self.reading_epoch(), start: input, end: start };
                let command = self
                    .reading_chunk_raw(label, None, 2048, 8192)
                    .ok()
                    .filter(|chunk| chunk.next.is_none())
                    .map(|chunk| display_text(&chunk))
                    .unwrap_or_default();
                Execution::Output { start, command }
            },
            (b'D', Execution::Output { start, command }) => {
                self.finish_reading_command(start, command, exit_code);
                Execution::Idle
            },
            // 重复、缺失和乱序标记不能沿用旧 C；下一条完整 A/B/C/D 才恢复。
            _ => Execution::Idle,
        };
    }

    fn finish_reading_command(
        &mut self,
        start: ReadingPoint,
        command: String,
        exit_code: Option<i32>,
    ) {
        let output = ReadingSpan { epoch: self.reading_epoch(), start, end: self.reading_cursor() };
        if self.validate_reading_span(output).is_err() {
            return;
        }
        let live_start =
            ReadingPoint { line: self.grid.scrolled_out() + self.grid.history_size(), column: 0 };
        let tail = ReadingSpan { start: output.start.max(live_start).min(output.end), ..output };
        let Ok(guard) = TailGuard::capture(self, tail) else {
            // 只拒绝真正超出守卫预算的 live 尾；与公开读取块的大小独立。
            return;
        };
        let cost = guard.heap_bytes() + command.capacity() + std::mem::size_of::<CommandRecord>();
        // VecDeque 弹出记录后仍保留槽位；另预留最大槽位容量，不把空槽漏出总预算。
        let reserve = COMMANDS * std::mem::size_of::<CommandRecord>();
        if cost + reserve > METADATA_BYTES {
            return;
        }
        while self.reading.commands.len() >= COMMANDS
            || self.reading.metadata + cost + reserve > METADATA_BYTES
        {
            if let Some(old) = self.reading.commands.pop_front() {
                self.reading.metadata -= old.cost;
            }
        }
        self.reading.next_id = self.reading.next_id.checked_add(1).expect("command id exhausted");
        let command = CompletedCommand { id: self.reading.next_id, command, exit_code, output };
        self.reading.commands.push_back(CommandRecord { command, guard, cost });
        self.reading.metadata += cost;
    }
}

// 标签只在一个有界完整结果上组装；输出复制应在锁外使用相同的物理行语义。
fn display_text(chunk: &ReadingChunk) -> String {
    let mut text = String::new();
    for row in &chunk.rows {
        text.push_str(&row.text);
        if row.hard_break {
            text.truncate(text.trim_end_matches(' ').len());
            text.push('\n');
        }
    }
    text.trim_end_matches([' ', '\n']).to_owned()
}

#[cfg(test)]
mod tests;
