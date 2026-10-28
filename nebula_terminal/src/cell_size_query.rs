//! Answers the `CSI 16 t` cell-size query.
//!
//! Programs that draw pixel graphics have to convert pixels to cells before
//! they can lay an image out: Sixel and OSC 1337 both do it, and so does every
//! terminal graphics protocol
//! all measure their preview rectangle in cells, then scale the bitmap to fit
//! that many cells with their *own* idea of the cell size. When the terminal
//! never answers, those programs fall back to a fixed pixel budget and stretch
//! the bitmap over the whole rectangle, which is what turns a photo into a
//! squashed preview.
//!
//! The query travels as `CSI 16 t` and expects `CSI 6 ; <cell height> ;
//! <cell width> t` — height first, both in device pixels. VTE has no handler
//! for either direction, so the bytes would otherwise be dropped silently.
//! This module watches the raw stream for the query, and answers from the pane
//! geometry the view reports through [`crate::event_loop::Msg::CellMetrics`].
//!
//! The answer can only be produced once the view knows the cell size. A pane
//! whose born geometry already equals its target geometry never sends a
//! resize, so queries arrive before any geometry is known; they are counted
//! here and answered as soon as the geometry lands, instead of being dropped.

use crate::event::{Event, EventListener, WindowSize};

/// `CSI 16 t`: report the cell size in pixels.
const QUERY: &[u8] = b"\x1b[16t";

/// Collected queries awaiting a known cell size.
#[derive(Default)]
pub struct CellSizeQueries {
    /// Trailing bytes that may begin a query split across two reads.
    tail: Vec<u8>,
    /// Queries seen but not answered yet.
    pending: u32,
}

impl CellSizeQueries {
    /// Count queries in `bytes`, keeping a split query for the next read.
    pub fn scan(&mut self, bytes: &[u8]) {
        let mut probe = std::mem::take(&mut self.tail);
        probe.extend_from_slice(bytes);

        let found = count_queries(&probe);
        if found > 0 {
            // A client that keeps asking must not grow this counter without
            // bound: one answer per drain is enough for every client we know
            // of, and a stuck counter would only answer stale questions.
            self.pending = self.pending.saturating_add(found).min(8);
        }

        self.tail = query_suffix(&probe);
    }

    /// Answer the collected queries once the cell size is known.
    ///
    /// Unknown geometry leaves the queries pending: the view reports the
    /// metric as soon as it lays the pane out.
    pub fn answer<U: EventListener>(&mut self, window_size: Option<WindowSize>, event_proxy: &U) {
        let Some(window_size) = window_size else {
            return;
        };
        if self.pending == 0 {
            return;
        }
        self.pending = 0;
        event_proxy.send_event(Event::PtyWrite(format!(
            "\x1b[6;{};{}t",
            window_size.cell_height, window_size.cell_width
        )));
    }
}

fn count_queries(bytes: &[u8]) -> u32 {
    if bytes.len() < QUERY.len() {
        return 0;
    }
    bytes.windows(QUERY.len()).filter(|window| *window == QUERY).count() as u32
}

/// Longest suffix of `bytes` that could begin a query.
fn query_suffix(bytes: &[u8]) -> Vec<u8> {
    let shortest = bytes.len().saturating_sub(QUERY.len() - 1);
    for start in shortest..bytes.len() {
        let suffix = &bytes[start..];
        if QUERY.starts_with(suffix) {
            return suffix.to_vec();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl EventListener for Capture {
        fn send_event(&self, event: Event) {
            self.0.lock().unwrap().push(format!("{event:?}"));
        }
    }

    fn replies() -> (Capture, Arc<Mutex<Vec<String>>>) {
        let capture = Capture::default();
        let seen = Arc::clone(&capture.0);
        (capture, seen)
    }

    fn size(cell_width: u16, cell_height: u16) -> WindowSize {
        WindowSize { num_lines: 24, num_cols: 80, cell_width, cell_height }
    }

    #[test]
    fn query_inside_a_larger_read_is_answered_once() {
        let (capture, seen) = replies();
        let mut queries = CellSizeQueries::default();
        queries.scan(b"noise\x1b[16tmore noise");
        queries.answer(Some(size(14, 36)), &capture);
        assert_eq!(seen.lock().unwrap().as_slice(), ["PtyWrite(\u{1b}[6;36;14t)"]);
    }

    #[test]
    fn query_split_across_reads_is_reassembled() {
        let (capture, seen) = replies();
        let mut queries = CellSizeQueries::default();
        queries.scan(b"prefix\x1b[1");
        queries.answer(Some(size(12, 30)), &capture);
        assert!(seen.lock().unwrap().is_empty(), "half a query is not a query");
        queries.scan(b"6t suffix");
        queries.answer(Some(size(12, 30)), &capture);
        assert_eq!(seen.lock().unwrap().as_slice(), ["PtyWrite(\u{1b}[6;30;12t)"]);
    }

    #[test]
    fn queries_waits_for_geometry_instead_of_dropping_the_answer() {
        let (capture, seen) = replies();
        let mut queries = CellSizeQueries::default();
        queries.scan(b"\x1b[16t");
        queries.answer(None, &capture);
        assert!(seen.lock().unwrap().is_empty());
        queries.answer(Some(size(9, 20)), &capture);
        assert_eq!(seen.lock().unwrap().as_slice(), ["PtyWrite(\u{1b}[6;20;9t)"]);
        queries.answer(Some(size(9, 20)), &capture);
        assert_eq!(seen.lock().unwrap().len(), 1, "one answer per question");
    }

    #[test]
    fn unrelated_sequences_do_not_answer() {
        let (capture, seen) = replies();
        let mut queries = CellSizeQueries::default();
        queries.scan(b"\x1b[14t\x1b[18t\x1b[16");
        queries.scan(b"]8;;");
        queries.answer(Some(size(14, 36)), &capture);
        assert!(seen.lock().unwrap().is_empty());
    }
}
