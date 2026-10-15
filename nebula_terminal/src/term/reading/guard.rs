//! 完成命令的紧凑内容守卫；只保存 D 时仍可能被覆盖的 live 尾，不存搜索映射。
use super::{ReadingError, ReadingPoint, ReadingSpan};
use crate::grid::Dimensions;
use crate::index::{Column, Line};
use crate::term::Term;
use crate::term::cell::{Cell, Flags};

pub(super) const MAX_CELLS: usize = 32768;
pub(super) const MAX_BYTES: usize = 128 * 1024;
const CELL_FLAGS: Flags =
    Flags::WIDE_CHAR.union(Flags::WIDE_CHAR_SPACER).union(Flags::LEADING_WIDE_CHAR_SPACER);

struct ZeroRun {
    cell: u32,
    start: u32,
    len: u32,
}

pub(super) struct TailGuard {
    pub(super) span: ReadingSpan,
    columns: usize,
    // Unicode 占低 21 位，原 flags 的位 4/5/6/10 放进高 11 位，每个普通 cell 只需 4 字节。
    codes: Box<[u32]>,
    // 组合字符稀疏存储，既不保留 CellExtra 的超链接，也不为每格分配 Vec。
    zero_runs: Box<[ZeroRun]>,
    zero_chars: Box<[char]>,
    leading_break: Option<bool>,
}

impl TailGuard {
    pub(super) fn capture<T>(term: &Term<T>, span: ReadingSpan) -> Result<Self, ReadingError> {
        term.validate_reading_span(span)?;
        let columns = term.columns();
        let count = distance(span.start, span.end, columns);
        if count > MAX_CELLS {
            return Err(ReadingError::BudgetTooSmall);
        }
        let leading_break = (span.start.column == columns && span.start < span.end)
            .then(|| wraps(term, span.start.line));
        let mut guard = Self {
            span,
            columns,
            codes: Box::default(),
            zero_runs: Box::default(),
            zero_chars: Box::default(),
            leading_break,
        };
        let mut codes = Vec::with_capacity(count);
        let mut zero_runs = Vec::new();
        let mut zero_chars = Vec::new();
        let mut bytes = count * std::mem::size_of::<u32>();
        let mut point = guard.point_at(0);
        for index in 0..count {
            let cell = cell_at(term, point);
            let zero = cell.zerowidth().unwrap_or_default();
            if !zero.is_empty() {
                let remaining = MAX_BYTES.saturating_sub(bytes);
                let header = std::mem::size_of::<ZeroRun>();
                // 先 O(1) 拒绝恶意组合序列，不能为读取守卫复制超预算的零宽字符。
                if remaining < header || zero.len() > (remaining - header) / 4 {
                    return Err(ReadingError::BudgetTooSmall);
                }
                bytes += header + zero.len() * 4;
                zero_runs.push(ZeroRun {
                    cell: index as u32,
                    start: zero_chars.len() as u32,
                    len: zero.len() as u32,
                });
                zero_chars.extend_from_slice(zero);
            }
            codes.push(guard.code(cell, point));
            advance(&mut point, columns);
        }
        // 收紧容量后计入总 metadata 预算；临时 Vec 不会长期保留倍增的空余容量。
        guard.codes = codes.into_boxed_slice();
        guard.zero_runs = zero_runs.into_boxed_slice();
        guard.zero_chars = zero_chars.into_boxed_slice();
        Ok(guard)
    }

    pub(super) fn heap_bytes(&self) -> usize {
        std::mem::size_of_val(self.codes.as_ref())
            + std::mem::size_of_val(self.zero_runs.as_ref())
            + std::mem::size_of_val(self.zero_chars.as_ref())
    }

    /// 只检查本块实际读取的相交部分；完整复制开始/结束时由 command_output_span 全检。
    /// 绝不因旧尾已滚入历史而丢弃 D 时原值，覆盖后再冻结同样会被发现。
    pub(super) fn validate<T>(
        &self,
        term: &Term<T>,
        read: ReadingSpan,
    ) -> Result<(), ReadingError> {
        let start = read.start.max(self.span.start);
        let end = read.end.min(self.span.end);
        if start >= end {
            return Ok(());
        }
        term.validate_reading_span(self.span)?;
        if start.column == self.columns {
            // 上块的字节预算可能恰好用尽，本块从待消费的换行开始；即使零个 cell，仍校验它。
            let expected = if start == self.span.start {
                self.leading_break
            } else {
                let last = ReadingPoint { column: self.columns - 1, ..start };
                let index = distance(self.span.start, last, self.columns);
                Some(self.codes[index] & (u32::from(Flags::WRAPLINE.bits()) << 21) != 0)
            };
            if expected.is_some_and(|expected| expected != wraps(term, start.line)) {
                return Err(ReadingError::ContentChanged);
            }
        }
        let first = distance(self.span.start, start, self.columns);
        let last = distance(self.span.start, end, self.columns);
        let mut zero_index = self.zero_runs.partition_point(|run| (run.cell as usize) < first);
        let mut point = self.point_at(first);
        for index in first..last {
            let cell = cell_at(term, point);
            if self.code(cell, point) != self.codes[index] {
                return Err(ReadingError::ContentChanged);
            }
            let expected = match self.zero_runs.get(zero_index) {
                Some(run) if run.cell as usize == index => {
                    zero_index += 1;
                    &self.zero_chars[run.start as usize..(run.start + run.len) as usize]
                },
                _ => &[],
            };
            if cell.zerowidth().unwrap_or_default() != expected {
                return Err(ReadingError::ContentChanged);
            }
            advance(&mut point, self.columns);
        }
        Ok(())
    }

    fn point_at(&self, index: usize) -> ReadingPoint {
        let offset = self.span.start.column + index;
        ReadingPoint {
            line: self.span.start.line + offset / self.columns,
            column: offset % self.columns,
        }
    }

    fn code(&self, cell: &Cell, point: ReadingPoint) -> u32 {
        let mut mask = CELL_FLAGS;
        let boundary = ReadingPoint { line: point.line, column: self.columns };
        // 输出停在满行末尾时，随后 append 产生的 softwrap 不属于旧命令，不应误失效。
        if point.column + 1 == self.columns && boundary < self.span.end {
            mask |= Flags::WRAPLINE;
        }
        u32::from(cell.c) | (u32::from((cell.flags & mask).bits()) << 21)
    }
}

fn distance(start: ReadingPoint, end: ReadingPoint, columns: usize) -> usize {
    (end.line - start.line) * columns + end.column - start.column
}

fn cell_at<T>(term: &Term<T>, point: ReadingPoint) -> &Cell {
    let row = point.line as i64 - term.grid.scrolled_out() as i64 - term.grid.history_size() as i64;
    &term.grid[Line(row as i32)][Column(point.column)]
}

fn wraps<T>(term: &Term<T>, line: usize) -> bool {
    cell_at(term, ReadingPoint { line, column: term.columns() - 1 }).flags.contains(Flags::WRAPLINE)
}

fn advance(point: &mut ReadingPoint, columns: usize) {
    point.column += 1;
    if point.column == columns {
        point.line += 1;
        point.column = 0;
    }
}
