//! One document history across source and formatted inputs. Each input is a
//! presentation: its lifetime must not decide which edits can be undone.

use super::inline_edit::changed_span;
use gpui_component::Rope;
use std::{
    ops::Range,
    time::{Duration, Instant},
};

const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;

struct Change {
    start: usize,
    before: String,
    after: String,
}

impl Change {
    fn bytes(&self) -> usize {
        self.before.len() + self.after.len()
    }
}

/// Find the changed byte range without materializing the whole Rope.
///
/// `InputState::value()` turns its backing Rope into a new String. Ordinary
/// typing only needs the small replacement at the cursor, so compare the
/// character streams and copy just that replacement into the history state.
fn changed_rope_span(before: &str, after: &Rope) -> (Range<usize>, Range<usize>) {
    let mut prefix = 0;
    let mut after_chars = after.chars();
    for (offset, before_char) in before.char_indices() {
        if after_chars.next() == Some(before_char) {
            prefix = offset + before_char.len_utf8();
        } else {
            break;
        }
    }

    let mut before_suffix = before[prefix..].chars().rev();
    let after_tail = after.slice(prefix..);
    let mut after_suffix = after_tail.chars_at(after_tail.len());
    let mut suffix = 0;
    while let (Some(before_char), Some(after_char)) = (before_suffix.next(), after_suffix.prev()) {
        if before_char != after_char {
            break;
        }
        suffix += before_char.len_utf8();
    }

    (prefix..before.len() - suffix, prefix..after.len() - suffix)
}

#[derive(Default)]
pub(super) struct EditHistory {
    current: String,
    undo: Vec<Vec<Change>>,
    redo: Vec<Vec<Change>>,
    bytes: usize,
    last_edit: Option<Instant>,
}

impl EditHistory {
    pub fn reset(&mut self, text: &str) {
        *self = Self { current: text.to_owned(), ..Self::default() };
    }

    pub fn barrier(&mut self) {
        self.last_edit = None;
    }

    /// Record a local edit directly from the input's Rope.
    ///
    /// This is the hot path for ordinary source typing. The Rope is scanned
    /// without creating a document-sized String, and `current` is edited in
    /// place so the history does not retain a full snapshot per keystroke.
    pub fn record_rope(&mut self, text: &Rope) {
        let (before, after) = changed_rope_span(&self.current, text);
        if before.is_empty() && after.is_empty() {
            return;
        }

        let change = Change {
            start: before.start,
            before: self.current[before.clone()].to_owned(),
            after: text.slice(after).to_string(),
        };
        self.current.replace_range(before, &change.after);
        self.push_change(change);
    }

    pub fn record(&mut self, text: &str) {
        if text == self.current {
            return;
        }
        let (before, after) = changed_span(&self.current, text);
        let change = Change {
            start: before.start,
            before: self.current[before.clone()].to_owned(),
            after: text[after].to_owned(),
        };
        self.current.replace_range(before, &change.after);
        self.push_change(change);
    }

    fn push_change(&mut self, change: Change) {
        self.bytes -= self.redo.iter().flatten().map(Change::bytes).sum::<usize>();
        self.redo.clear();
        let now = Instant::now();
        let join = self
            .last_edit
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(600))
            && self.undo.last().and_then(|group| group.last()).is_some_and(|last| {
                change.start <= last.start + last.after.len()
                    && change.start + change.before.len() >= last.start
            });
        self.bytes += change.bytes();
        if join {
            self.undo.last_mut().unwrap().push(change);
        } else {
            self.undo.push(vec![change]);
        }
        while self.bytes > MAX_HISTORY_BYTES && !self.undo.is_empty() {
            self.bytes -= self.undo.remove(0).iter().map(Change::bytes).sum::<usize>();
        }
        self.last_edit = Some(now);
    }

    pub fn travel(&mut self, redo: bool) -> Option<(String, Range<usize>)> {
        self.barrier();
        let changes = if redo { self.redo.pop()? } else { self.undo.pop()? };
        let mut cursor = 0;
        if redo {
            for change in &changes {
                self.current
                    .replace_range(change.start..change.start + change.before.len(), &change.after);
                cursor = change.start + change.after.len();
            }
            self.undo.push(changes);
        } else {
            for change in changes.iter().rev() {
                self.current
                    .replace_range(change.start..change.start + change.after.len(), &change.before);
                cursor = change.start + change.before.len();
            }
            self.redo.push(changes);
        }
        Some((self.current.clone(), cursor..cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_a_repeated_suffix_does_not_overlap_the_common_prefix() {
        let mut history = EditHistory::default();
        history.reset("中文abc中文abc");
        history.record_rope(&Rope::from("中文abc"));
        assert_eq!(history.travel(false).unwrap().0, "中文abc中文abc");
        assert_eq!(history.travel(true).unwrap().0, "中文abc");
    }

    #[test]
    fn history_spans_input_lifetimes_and_discards_redo_after_a_new_edit() {
        let mut history = EditHistory::default();
        history.reset("# 中文\n\nbody");
        history.record("# 中文😀\n\nbody");
        history.barrier();
        history.record("# 中文😀\n\nchanged");
        assert_eq!(history.travel(false).unwrap().0, "# 中文😀\n\nbody");
        assert_eq!(history.travel(false).unwrap().0, "# 中文\n\nbody");
        assert_eq!(history.travel(true).unwrap().0, "# 中文😀\n\nbody");
        history.record("# 中文🌿\n\nbody");
        assert!(history.travel(true).is_none());
        assert_eq!(history.travel(false).unwrap().0, "# 中文😀\n\nbody");
    }

    #[test]
    fn rope_records_only_the_local_unicode_change() {
        let mut history = EditHistory::default();
        history.reset("prefix 中文 suffix");

        history.record_rope(&Rope::from("prefix 🌿 suffix"));
        assert_eq!(history.current, "prefix 🌿 suffix");
        assert_eq!(history.travel(false).unwrap().0, "prefix 中文 suffix");
        assert_eq!(history.travel(true).unwrap().0, "prefix 🌿 suffix");
    }
}
