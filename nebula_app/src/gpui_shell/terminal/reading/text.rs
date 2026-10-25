//! 有界流式字面搜索与复制组装；只消费锁外的终端文本块。
use nebula_terminal::term::reading::{ReadingPoint, ReadingRow};
use std::collections::VecDeque;

pub(in crate::gpui_shell::terminal) const MAX_QUERY_BYTES: usize = 4096;
pub(super) const MAX_MATCHES: usize = 10_000;
pub(super) const MAX_COPY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gpui_shell::terminal) struct SearchHit {
    pub start: ReadingPoint,
    pub end: ReadingPoint,
}
#[derive(Clone, Copy)]
struct Unit {
    start: ReadingPoint,
    end: ReadingPoint,
}
/// KMP 状态及坐标窗口只与查询长度有关，不缓存整条逻辑行。
pub(super) struct LiteralSearch {
    needle: Vec<char>,
    prefix: Vec<usize>,
    matched: usize,
    window: VecDeque<Unit>,
    case_sensitive: bool,
    hits: Vec<SearchHit>,
    pending_space_hits: Vec<SearchHit>,
    pending_overflow: bool,
    emitted: usize,
    truncated: bool,
}
impl LiteralSearch {
    pub(super) fn new(query: &str, case_sensitive: bool) -> Option<Self> {
        if query.is_empty() || query.len() > MAX_QUERY_BYTES || query.chars().any(char::is_control)
        {
            return None;
        }
        let needle: Vec<_> = if case_sensitive {
            query.chars().collect()
        } else {
            query.chars().flat_map(char::to_lowercase).collect()
        };
        let mut prefix = vec![0; needle.len()];
        for i in 1..needle.len() {
            let mut length = prefix[i - 1];
            while length > 0 && needle[i] != needle[length] {
                length = prefix[length - 1];
            }
            if needle[i] == needle[length] {
                length += 1;
            }
            prefix[i] = length;
        }
        Some(Self {
            needle,
            prefix,
            matched: 0,
            window: VecDeque::new(),
            case_sensitive,
            hits: Vec::new(),
            pending_space_hits: Vec::new(),
            pending_overflow: false,
            emitted: 0,
            truncated: false,
        })
    }
    pub(super) fn feed(&mut self, row: &ReadingRow) {
        for cell in &row.cells {
            let Some(text) = row.text.get(cell.byte_start..cell.byte_end) else { continue };
            let unit = Unit {
                start: ReadingPoint { line: row.line, column: cell.column },
                end: ReadingPoint { line: row.line, column: cell.column + cell.width },
            };
            for ch in text.chars() {
                if ch != ' ' {
                    self.flush_space_hits();
                }
                if self.case_sensitive {
                    self.feed_char(ch, unit, ch == ' ');
                } else {
                    for normalized in ch.to_lowercase() {
                        self.feed_char(normalized, unit, ch == ' ');
                    }
                }
            }
        }
        if row.hard_break {
            // 行尾 padding 不属于文本；软折行不能重置匹配状态。
            self.pending_space_hits.clear();
            self.pending_overflow = false;
            self.matched = 0;
            self.window.clear();
        }
    }
    fn feed_char(&mut self, ch: char, unit: Unit, space: bool) {
        if self.window.len() == self.needle.len() {
            self.window.pop_front();
        }
        self.window.push_back(unit);
        while self.matched > 0 && self.needle[self.matched] != ch {
            self.matched = self.prefix[self.matched - 1];
        }
        if self.needle[self.matched] == ch {
            self.matched += 1;
        }
        if self.matched != self.needle.len() {
            return;
        }
        let hit =
            SearchHit { start: self.window.front().expect("matched window").start, end: unit.end };
        self.matched = self.prefix[self.matched - 1];
        if space {
            if self.pending_space_hits.len() < MAX_MATCHES {
                self.pending_space_hits.push(hit);
            } else {
                self.pending_overflow = true;
            }
        } else {
            self.emit(hit);
        }
    }
    fn emit(&mut self, hit: SearchHit) {
        if self.hits.last() == Some(&hit) {
            return;
        }
        if self.emitted >= MAX_MATCHES {
            self.truncated = true;
            return;
        }
        self.emitted += 1;
        self.hits.push(hit);
    }
    fn flush_space_hits(&mut self) {
        let mut pending = std::mem::take(&mut self.pending_space_hits);
        for hit in pending.drain(..) {
            self.emit(hit);
        }
        self.pending_space_hits = pending;
        self.truncated |= std::mem::take(&mut self.pending_overflow);
    }
    pub(super) fn finish(&mut self, preserve_partial_tail: bool) {
        if preserve_partial_tail {
            self.flush_space_hits();
        } else {
            self.pending_space_hits.clear();
            self.pending_overflow = false;
        }
    }
    pub(super) fn take_hits(&mut self) -> Vec<SearchHit> {
        std::mem::take(&mut self.hits)
    }
    pub(super) fn truncated(&self) -> bool {
        self.truncated
    }
}

/// 延迟保存行尾空格数量，长空白行不申请同样大的临时字符串。
pub(super) struct TextCollector {
    text: String,
    spaces: usize,
    limit: usize,
}
impl TextCollector {
    pub(super) fn new(limit: usize) -> Self {
        Self { text: String::new(), spaces: 0, limit }
    }
    pub(super) fn feed(&mut self, row: &ReadingRow) -> Result<(), ()> {
        for ch in row.text.chars() {
            if ch == ' ' {
                self.spaces = self.spaces.checked_add(1).ok_or(())?;
            } else {
                let next = self
                    .text
                    .len()
                    .checked_add(self.spaces)
                    .and_then(|n| n.checked_add(ch.len_utf8()))
                    .ok_or(())?;
                if next > self.limit {
                    return Err(());
                }
                self.text.extend(std::iter::repeat_n(' ', self.spaces));
                self.spaces = 0;
                self.text.push(ch);
            }
        }
        if row.hard_break {
            self.spaces = 0;
            if self.text.len() >= self.limit {
                return Err(());
            }
            self.text.push('\n');
        }
        Ok(())
    }
    pub(super) fn finish(mut self, preserve_partial_tail: bool) -> Result<String, ()> {
        if preserve_partial_tail {
            if self.text.len().checked_add(self.spaces).is_none_or(|n| n > self.limit) {
                return Err(());
            }
            self.text.extend(std::iter::repeat_n(' ', self.spaces));
        }
        Ok(self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_terminal::term::reading::ReadingCell;
    fn row(line: usize, column: usize, text: &str, hard_break: bool) -> ReadingRow {
        let mut cells = Vec::new();
        let mut col = column;
        for (byte_start, ch) in text.char_indices() {
            cells.push(ReadingCell {
                byte_start,
                byte_end: byte_start + ch.len_utf8(),
                column: col,
                width: 1,
            });
            col += 1;
        }
        ReadingRow {
            line,
            start_column: column,
            end_column: col,
            text: text.into(),
            cells,
            hard_break,
        }
    }
    #[test]
    fn literal_matches_span_chunks_and_soft_wraps_not_hard_lines() {
        let mut search = LiteralSearch::new("中文ab", true).unwrap();
        search.feed(&row(0, 3, "中", false));
        search.feed(&row(0, 4, "文", false));
        search.feed(&row(1, 0, "ab", true));
        assert_eq!(
            search.take_hits(),
            vec![SearchHit {
                start: ReadingPoint { line: 0, column: 3 },
                end: ReadingPoint { line: 1, column: 2 }
            }]
        );
        search.feed(&row(2, 0, "中文", true));
        search.feed(&row(3, 0, "ab", true));
        assert!(search.take_hits().is_empty());
    }
    #[test]
    fn overlapping_and_unicode_lowercase_keep_source_coordinates() {
        let mut search = LiteralSearch::new("aa", true).unwrap();
        search.feed(&row(2, 0, "aaa", true));
        assert_eq!(search.take_hits().len(), 2);
        let mut search = LiteralSearch::new("İ世", false).unwrap();
        search.feed(&row(7, 4, "İ世界", true));
        assert_eq!(
            search.take_hits(),
            vec![SearchHit {
                start: ReadingPoint { line: 7, column: 4 },
                end: ReadingPoint { line: 7, column: 6 }
            }]
        );
    }
    #[test]
    fn padding_is_not_a_match_but_spaces_inside_a_line_are() {
        let mut search = LiteralSearch::new("x ", true).unwrap();
        search.feed(&row(0, 0, "x   ", true));
        assert!(search.take_hits().is_empty());
        search.feed(&row(1, 0, "x ", false));
        search.feed(&row(1, 2, "y", true));
        assert_eq!(search.take_hits().len(), 1);
    }
    #[test]
    fn result_count_is_bounded_and_literal_symbols_are_not_regex() {
        let mut search = LiteralSearch::new(".", true).unwrap();
        search.feed(&row(0, 0, &".".repeat(MAX_MATCHES + 1), true));
        assert_eq!(search.take_hits().len(), MAX_MATCHES);
        assert!(search.truncated());
        assert!(LiteralSearch::new("x\ny", true).is_none());
        assert!(LiteralSearch::new(&"x".repeat(MAX_QUERY_BYTES + 1), true).is_none());
    }
    #[test]
    fn collector_preserves_internal_spaces_and_soft_wrap_without_padding() {
        let mut copy = TextCollector::new(100);
        copy.feed(&row(0, 0, "a  ", false)).unwrap();
        copy.feed(&row(1, 0, " b  ", true)).unwrap();
        copy.feed(&row(2, 0, "末 ", false)).unwrap();
        assert_eq!(copy.finish(true).unwrap(), "a   b\n末 ");
        let mut copy = TextCollector::new(3);
        copy.feed(&row(0, 0, "中    ", false)).unwrap();
        assert_eq!(copy.finish(false).unwrap(), "中");
    }
    #[test]
    fn collector_refuses_oversized_payload_without_partial_success() {
        let mut copy = TextCollector::new(2);
        assert!(copy.feed(&row(0, 0, "中文", false)).is_err());
        let mut copy = TextCollector::new(2);
        copy.feed(&row(0, 0, "x  ", false)).unwrap();
        assert!(copy.finish(true).is_err());
    }
}
