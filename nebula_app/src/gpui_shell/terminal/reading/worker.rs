//! 后台分块读取；匹配、组装和消息回传均不占用终端锁。
use super::super::session::EventProxy;
use super::{
    ReadingIssue, SearchHit,
    text::{LiteralSearch, MAX_COPY_BYTES, TextCollector},
};
use futures::{SinkExt as _, channel::mpsc};
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::term::reading::{ReadingRow, ReadingSpan};
use nebula_terminal::{Term, sync::FairMutex};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

type Source = Arc<FairMutex<Term<EventProxy>>>;
const CHUNK_CELLS: usize = 4096;
const CHUNK_BYTES: usize = 32 * 1024;

pub(in crate::gpui_shell::terminal) struct SearchUpdate {
    pub hits: Vec<SearchHit>,
    pub done: bool,
    pub truncated: bool,
    pub issue: Option<ReadingIssue>,
}
fn cancelled(generation: &AtomicU64, expected: u64) -> bool {
    generation.load(Ordering::Acquire) != expected
}
pub(in crate::gpui_shell::terminal) async fn search(
    source: Source,
    span: ReadingSpan,
    query: String,
    match_case: bool,
    generation: Arc<AtomicU64>,
    expected: u64,
    mut updates: mpsc::Sender<SearchUpdate>,
) {
    let Some(mut matcher) = LiteralSearch::new(&query, match_case) else {
        let _ = updates
            .send(SearchUpdate {
                hits: Vec::new(),
                done: true,
                truncated: false,
                issue: Some(ReadingIssue::QueryTooLong),
            })
            .await;
        return;
    };
    let mut cursor = None;
    loop {
        if cancelled(&generation, expected) {
            return;
        }
        let result = source.lock().reading_chunk(span, cursor, CHUNK_CELLS, CHUNK_BYTES);
        let chunk = match result {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = updates
                    .send(SearchUpdate {
                        hits: Vec::new(),
                        done: true,
                        truncated: false,
                        issue: Some(error.into()),
                    })
                    .await;
                return;
            },
        };
        if cancelled(&generation, expected) {
            return;
        }
        for row in &chunk.rows {
            matcher.feed(row);
        }
        let done = chunk.next.is_none() || matcher.truncated();
        if done {
            matcher.finish(span.end.column != 0);
        }
        let hits = matcher.take_hits();
        if (!hits.is_empty() || done)
            && updates
                .send(SearchUpdate { hits, done, truncated: matcher.truncated(), issue: None })
                .await
                .is_err()
        {
            return;
        }
        if done {
            return;
        }
        if chunk.next == cursor {
            return;
        }
        cursor = chunk.next;
        std::thread::yield_now();
    }
}
pub(in crate::gpui_shell::terminal) fn copy_command(
    source: Source,
    command: u64,
    span: ReadingSpan,
    generation: Arc<AtomicU64>,
    expected: u64,
) -> Result<String, ReadingIssue> {
    let mut collector = TextCollector::new(MAX_COPY_BYTES);
    let mut cursor = None;
    loop {
        if cancelled(&generation, expected) {
            return Err(ReadingIssue::Changed);
        }
        let chunk = source.lock().reading_chunk(span, cursor, CHUNK_CELLS, CHUNK_BYTES)?;
        for row in &chunk.rows {
            collector.feed(row).map_err(|_| ReadingIssue::CopyTooLarge)?;
        }
        if chunk.next.is_none() {
            break;
        }
        if chunk.next == cursor {
            return Err(ReadingIssue::Changed);
        }
        cursor = chunk.next;
        std::thread::yield_now();
    }
    if cancelled(&generation, expected) || source.lock().command_output_span(command)? != span {
        return Err(ReadingIssue::Changed);
    }
    // D 在行中结束时，之前的空格是真实输出而非整行 padding。
    let text = collector.finish(span.end.column != 0).map_err(|_| ReadingIssue::CopyTooLarge)?;
    if text.is_empty() {
        return Err(ReadingIssue::EmptyOutput);
    }
    Ok(text)
}

/// 活动屏不会随每次写入推进 epoch，故最终提交必须重比原文而非只核坐标。
/// 限制在一小块内；更长的命中等进入不可变历史后才允许导航。
pub(in crate::gpui_shell::terminal) enum HitGuard {
    History,
    Live(Vec<ReadingRow>),
}
pub(in crate::gpui_shell::terminal) fn validate_hit(
    source: &Source,
    span: ReadingSpan,
    query: &str,
    match_case: bool,
) -> Option<HitGuard> {
    let (history, live_rows) = {
        let term = source.lock();
        let floor = term.grid().scrolled_out().saturating_add(term.history_size());
        let history = span.end.line < floor || (span.end.line == floor && span.end.column == 0);
        let rows = if history {
            None
        } else {
            let chunk = term.reading_chunk(span, None, 512, 4096).ok()?;
            if chunk.next.is_some() {
                return None;
            }
            Some(chunk.rows)
        };
        (history, rows)
    };
    let mut matcher = LiteralSearch::new(query, match_case)?;
    if let Some(rows) = live_rows {
        for row in &rows {
            matcher.feed(row);
        }
        matcher.finish(true);
        return matcher
            .take_hits()
            .iter()
            .any(|hit| hit.start == span.start && hit.end == span.end)
            .then_some(HitGuard::Live(rows));
    }
    debug_assert!(history);
    let mut cursor = None;
    for _ in 0..32 {
        let chunk = source.lock().reading_chunk(span, cursor, CHUNK_CELLS, CHUNK_BYTES).ok()?;
        for row in &chunk.rows {
            matcher.feed(row);
        }
        if chunk.next.is_none() {
            matcher.finish(true);
            return matcher
                .take_hits()
                .iter()
                .any(|hit| hit.start == span.start && hit.end == span.end)
                .then_some(HitGuard::History);
        }
        if cursor == chunk.next {
            return None;
        }
        cursor = chunk.next;
    }
    None
}
pub(in crate::gpui_shell::terminal) fn guard_valid(
    term: &Term<EventProxy>,
    span: ReadingSpan,
    guard: &HitGuard,
) -> bool {
    let available = term.reading_span();
    if available.epoch != span.epoch || available.start > span.start || available.end < span.end {
        return false;
    }
    match guard {
        HitGuard::History => true,
        HitGuard::Live(rows) => term.reading_chunk(span, None, 512, 4096).is_ok_and(|chunk| {
            chunk.next.is_none()
                && chunk.rows.len() == rows.len()
                && chunk.rows.iter().zip(rows).all(|(a, b)| {
                    a.line == b.line
                        && a.start_column == b.start_column
                        && a.end_column == b.end_column
                        && a.text == b.text
                        && a.hard_break == b.hard_break
                        && a.cells.len() == b.cells.len()
                        && a.cells.iter().zip(&b.cells).all(|(a, b)| {
                            a.byte_start == b.byte_start
                                && a.byte_end == b.byte_end
                                && a.column == b.column
                                && a.width == b.width
                        })
                })
        }),
    }
}
