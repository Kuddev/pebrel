//! Editable text projected from Markdown source positions, with formatting kept
//! in the source. Unchanged leaves and link destinations remain byte-for-byte.

use markdown::mdast::Node;
use std::ops::Range;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub link: bool,
    pub code: bool,
}

#[derive(Clone)]
struct Leaf {
    source: Range<usize>,
    visible: Range<usize>,
    text: String,
    marks: Marks,
    raw: bool,
    revealable: bool,
}

#[derive(Clone)]
struct Expandable {
    source: Range<usize>,
    visible: Range<usize>,
}

pub(super) struct Projection {
    source: String,
    pub text: String,
    leaves: Vec<Leaf>,
    containers: Vec<Range<usize>>,
    expandables: Vec<Expandable>,
    reveal_range: Option<Range<usize>>,
    pub rich: bool,
    literal: bool,
}

/// A single replacement in Unicode text; never split a CJK character or emoji.
pub(super) fn changed_span(before: &str, after: &str) -> (Range<usize>, Range<usize>) {
    let prefix = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    let suffix = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    (prefix..before.len() - suffix, prefix..after.len() - suffix)
}

fn shift_reveal(
    reveal: &Range<usize>,
    changed: &Range<usize>,
    inserted: usize,
    source_len: usize,
) -> Range<usize> {
    let delta = inserted as isize - changed.len() as isize;
    let (start, end) = if changed.is_empty() {
        if changed.start < reveal.start {
            (reveal.start.saturating_add_signed(delta), reveal.end.saturating_add_signed(delta))
        } else if changed.start <= reveal.end {
            (reveal.start, reveal.end.saturating_add_signed(delta))
        } else {
            (reveal.start, reveal.end)
        }
    } else if changed.end <= reveal.start {
        (reveal.start.saturating_add_signed(delta), reveal.end.saturating_add_signed(delta))
    } else if changed.start >= reveal.end {
        (reveal.start, reveal.end)
    } else {
        let start = reveal.start.min(changed.start);
        let end = if changed.end <= reveal.end {
            reveal.end.saturating_add_signed(delta)
        } else {
            changed.start.saturating_add(inserted)
        };
        (start, end.max(start))
    };
    let start = start.min(source_len);
    let end = end.min(source_len).max(start);
    start..end
}

impl Projection {
    /// During a composition or a trailing-space edit, Markdown can temporarily
    /// be incomplete. Retain the existing leaf/mark identity until it parses to
    /// the same visible text; do not replace the native input to "repair" it.
    pub fn accept(&mut self, text: &str, source: &str) -> bool {
        if self.literal {
            *self = Self::raw(source);
            return true;
        }
        let (source_before, source_after) = changed_span(&self.source, source);
        let reveal = self
            .reveal_range
            .as_ref()
            .map(|range| shift_reveal(range, &source_before, source_after.len(), source.len()));
        let parsed = Self::with_reveal(source, reveal.clone());
        let replaced_reveal = self.reveal_range.as_ref().is_some_and(|range| {
            source_before.start <= range.start && source_before.end >= range.end
        });
        if parsed.text == text
            && (self.reveal_range.is_none() || parsed.revealed().is_some() || replaced_reveal)
        {
            *self = parsed;
            return true;
        }
        let (visible_before, visible_after) = changed_span(&self.text, text);
        let Some(index) = self.leaves.iter().position(|leaf| {
            leaf.visible.start <= visible_before.start && leaf.visible.end >= visible_before.end
        }) else {
            return false;
        };
        let shift =
            |range: &mut Range<usize>, change: &Range<usize>, inserted: usize, affected: bool| {
                let delta = inserted as isize - change.len() as isize;
                if affected {
                    range.end = range.end.saturating_add_signed(delta);
                } else if range.start >= change.end {
                    range.start = range.start.saturating_add_signed(delta);
                    range.end = range.end.saturating_add_signed(delta);
                }
            };
        for (i, leaf) in self.leaves.iter_mut().enumerate() {
            shift(&mut leaf.source, &source_before, source_after.len(), i == index);
            shift(&mut leaf.visible, &visible_before, visible_after.len(), i == index);
            if i == index {
                leaf.text = text[leaf.visible.clone()].to_owned();
            }
        }
        for range in &mut self.containers {
            let affected = range.start <= source_before.start && range.end >= source_before.end;
            shift(range, &source_before, source_after.len(), affected);
        }
        self.source = source.to_owned();
        self.text = text.to_owned();
        self.reveal_range = reveal;
        self.rebuild_expandables();
        true
    }

    pub fn new(source: &str) -> Self {
        Self::with_reveal(source, None)
    }

    /// Build a rich projection while exposing one inline node's source syntax.
    ///
    /// The returned text contains the selected node's exact source range, while
    /// all unrelated leaves retain their rendered text and marks. This keeps a
    /// single native input usable for local source reveal without making the
    /// surrounding paragraph raw.
    pub fn with_reveal(source: &str, reveal: Option<Range<usize>>) -> Self {
        let mut result = Self {
            source: source.to_owned(),
            text: String::new(),
            leaves: vec![],
            containers: vec![],
            expandables: vec![],
            reveal_range: None,
            rich: false,
            literal: false,
        };
        let mut options = markdown::ParseOptions::gfm();
        options.constructs.math_text = true;
        options.constructs.math_flow = true;
        if let Ok(Node::Root(root)) = markdown::to_mdast(source, &options) {
            if root.children.len() == 1 {
                match &root.children[0] {
                    Node::Heading(heading) => {
                        result.rich = true;
                        for child in &heading.children {
                            result.collect(child, Marks::default());
                        }
                    },
                    Node::Paragraph(paragraph) => {
                        result.rich = true;
                        for child in &paragraph.children {
                            result.collect(child, Marks::default());
                        }
                    },
                    _ => {},
                }
            }
        }
        if result.leaves.is_empty() {
            if result.rich {
                result.push(
                    source.len()..source.len(),
                    String::new(),
                    Marks::default(),
                    false,
                    false,
                );
            } else {
                result.rich = source.is_empty();
                result.push(0..source.len(), source.to_owned(), Marks::default(), true, false);
            }
        }
        result.rebuild_expandables();
        if let Some(range) = reveal.and_then(|range| result.normalize_expandable(range)) {
            result.apply_reveal(range);
        }
        result
    }

    pub fn raw(source: &str) -> Self {
        let mut result = Self {
            source: source.to_owned(),
            text: String::new(),
            leaves: vec![],
            containers: vec![],
            expandables: vec![],
            reveal_range: None,
            rich: false,
            literal: true,
        };
        result.push(0..source.len(), source.to_owned(), Marks::default(), true, false);
        result
    }

    fn collect(&mut self, node: &Node, mut marks: Marks) {
        let Some(position) = node.position() else { return };
        let span = position.start.offset..position.end.offset;
        match node {
            Node::Text(text) => self.push(span, text.value.clone(), marks, false, false),
            Node::InlineCode(code) => {
                marks.code = true;
                self.push(span, code.value.clone(), marks, false, true);
            },
            Node::Strong(_)
            | Node::Emphasis(_)
            | Node::Delete(_)
            | Node::Link(_)
            | Node::LinkReference(_) => {
                match node {
                    Node::Strong(_) => marks.bold = true,
                    Node::Emphasis(_) => marks.italic = true,
                    Node::Delete(_) => marks.strike = true,
                    _ => marks.link = true,
                }
                self.containers.push(span);
                for child in node.children().into_iter().flatten() {
                    self.collect(child, marks);
                }
            },
            Node::Break(_) => self.push(span, "\n".to_owned(), marks, false, false),
            Node::Html(html) if matches!(html.value.as_str(), "<br>" | "<br/>" | "<br />") => {
                self.push(span, "\n".to_owned(), marks, true, false);
            },
            _ => {
                // Embedded objects keep an explicit Markdown representation in
                // the focused block; their untouched source is never serialized.
                let text = self.source[span.clone()].to_owned();
                self.push(span, text, marks, true, true);
            },
        }
    }

    fn push(
        &mut self,
        source: Range<usize>,
        text: String,
        marks: Marks,
        raw: bool,
        revealable: bool,
    ) {
        let start = self.text.len();
        self.text.push_str(&text);
        self.leaves.push(Leaf {
            source,
            visible: start..self.text.len(),
            text,
            marks,
            raw,
            revealable,
        });
    }

    fn rebuild_expandables(&mut self) {
        let mut expandables = Vec::with_capacity(self.containers.len() + self.leaves.len());
        for source in &self.containers {
            if let Some(visible) = self.visible_range_for_source(source) {
                expandables.push(Expandable { source: source.clone(), visible });
            }
        }
        for leaf in &self.leaves {
            if leaf.revealable {
                expandables.push(Expandable {
                    source: leaf.source.clone(),
                    visible: leaf.visible.clone(),
                });
            }
        }
        self.expandables = expandables;
    }

    fn visible_range_for_source(&self, source: &Range<usize>) -> Option<Range<usize>> {
        let mut visible: Option<Range<usize>> = None;
        for leaf in &self.leaves {
            if leaf.source.start < source.start || leaf.source.end > source.end {
                continue;
            }
            if leaf.visible.is_empty() {
                continue;
            }
            visible = Some(match visible {
                Some(current) => {
                    current.start.min(leaf.visible.start)..current.end.max(leaf.visible.end)
                },
                None => leaf.visible.clone(),
            });
        }
        visible
    }

    fn normalize_expandable(&self, source: Range<usize>) -> Option<Range<usize>> {
        self.expandables
            .iter()
            .find(|expandable| expandable.source == source)
            .map(|expandable| expandable.source.clone())
    }

    fn apply_reveal(&mut self, source: Range<usize>) {
        let Some(raw) = self.source.get(source.clone()).map(str::to_owned) else { return };
        let old_leaves = std::mem::take(&mut self.leaves);
        let mut leaves = Vec::with_capacity(old_leaves.len());
        let mut text = String::with_capacity(self.text.len() + raw.len());
        let mut inserted = false;

        for mut leaf in old_leaves {
            let overlaps = leaf.source.start < source.end && source.start < leaf.source.end;
            if overlaps {
                if !inserted {
                    let start = text.len();
                    text.push_str(&raw);
                    leaves.push(Leaf {
                        source: source.clone(),
                        visible: start..text.len(),
                        text: raw.clone(),
                        marks: Marks::default(),
                        raw: true,
                        revealable: true,
                    });
                    inserted = true;
                }
                continue;
            }
            let start = text.len();
            text.push_str(&leaf.text);
            leaf.visible = start..text.len();
            leaves.push(leaf);
        }

        if !inserted {
            self.leaves = leaves;
            return;
        }
        self.text = text;
        self.leaves = leaves;
        self.containers
            .retain(|container| !(source.start <= container.start && container.end <= source.end));
        self.reveal_range = Some(source);
        self.rebuild_expandables();
    }

    pub fn marks(&self) -> impl Iterator<Item = (Range<usize>, Marks)> + '_ {
        self.leaves.iter().map(|leaf| (leaf.visible.clone(), leaf.marks))
    }

    pub fn toggle_mark(&self, selection: Range<usize>, marker: &str) -> Option<String> {
        if !self.rich || selection.is_empty() {
            return None;
        }
        let first = self.leaves.iter().find(|leaf| leaf.visible.contains(&selection.start))?;
        let last = self
            .leaves
            .iter()
            .find(|leaf| leaf.visible.start < selection.end && leaf.visible.end >= selection.end)?;
        if first.raw || last.raw || first.marks.code || last.marks.code {
            return None;
        }
        let start = first.source.start
            + raw_offset(
                &self.source[first.source.clone()],
                &first.text,
                selection.start - first.visible.start,
            )?;
        let end = last.source.start
            + raw_offset(
                &self.source[last.source.clone()],
                &last.text,
                selection.end - last.visible.start,
            )?;
        let mut source = self.source.clone();
        if start >= marker.len()
            && source.get(start - marker.len()..start) == Some(marker)
            && source.get(end..end + marker.len()) == Some(marker)
        {
            source.replace_range(end..end + marker.len(), "");
            source.replace_range(start - marker.len()..start, "");
        } else {
            source.insert_str(end, marker);
            source.insert_str(start, marker);
        }
        (Self::new(&source).text == self.text).then_some(source)
    }

    /// Translate a source byte offset to the corresponding projected offset.
    ///
    /// Offsets that fall inside Markdown delimiters map to the nearest visible
    /// leaf boundary. Both sides are clamped to a UTF-8 character boundary.
    pub fn visible_offset(&self, offset: usize) -> usize {
        let offset = self.source.floor_char_boundary(offset.min(self.source.len()));
        let leaf = self
            .leaves
            .iter()
            .find(|leaf| leaf.source.end >= offset)
            .or_else(|| self.leaves.last())
            .unwrap();
        let local = offset.saturating_sub(leaf.source.start).min(leaf.source.len());
        let raw = &self.source[leaf.source.clone()];
        let local = if leaf.marks.code {
            inline_code_visible_offset(raw, &leaf.text, local)
        } else {
            visible_from_source(raw, &leaf.text, local)
        }
        .unwrap_or_else(|| leaf.text.floor_char_boundary(local.min(leaf.text.len())));
        leaf.visible.start + local
    }

    /// Translate a projected byte offset back to the source buffer.
    ///
    /// This is the inverse of `visible_offset` for leaf content. A projected
    /// offset at a formatting boundary resolves to the adjacent source boundary;
    /// revealed raw leaves therefore map delimiters exactly.
    pub fn source_offset(&self, offset: usize) -> usize {
        let offset = self.text.floor_char_boundary(offset.min(self.text.len()));
        let leaf = self
            .leaves
            .iter()
            .find(|leaf| {
                !leaf.visible.is_empty()
                    && leaf.visible.start <= offset
                    && offset < leaf.visible.end
            })
            .or_else(|| {
                self.leaves
                    .iter()
                    .find(|leaf| !leaf.visible.is_empty() && leaf.visible.start == offset)
            })
            .or_else(|| self.leaves.iter().find(|leaf| leaf.visible.end >= offset))
            .or_else(|| self.leaves.last())
            .unwrap();
        let local = offset.saturating_sub(leaf.visible.start).min(leaf.text.len());
        let raw = &self.source[leaf.source.clone()];
        let local = if leaf.marks.code {
            inline_code_source_offset(raw, &leaf.text, local)
        } else {
            raw_offset(raw, &leaf.text, local)
        }
        .unwrap_or_else(|| leaf.text.floor_char_boundary(local));
        leaf.source.start + local
    }

    /// Return the innermost expandable inline at a projected byte offset.
    ///
    /// When this projection already exposes a raw inline, its range wins at the
    /// end boundary too. This keeps hit testing stable while the source is
    /// temporarily incomplete during IME or delimiter edits.
    pub fn reveal_at(&self, visible_offset: usize) -> Option<Range<usize>> {
        let offset = self.text.floor_char_boundary(visible_offset.min(self.text.len()));
        if let Some(range) = &self.reveal_range
            && let Some(expandable) = self.expandables.iter().find(|expandable| {
                expandable.source == *range
                    && expandable.visible.start <= offset
                    && offset <= expandable.visible.end
            })
        {
            return Some(expandable.source.clone());
        }
        self.expandables
            .iter()
            .filter(|expandable| {
                expandable.visible.start <= offset && offset < expandable.visible.end
            })
            .min_by_key(|expandable| (expandable.source.len(), expandable.source.start))
            .map(|expandable| expandable.source.clone())
    }

    pub fn revealed(&self) -> Option<Range<usize>> {
        self.reveal_range.clone()
    }

    /// Translate a visible edit into source edits. Deleting across formatting
    /// boundaries removes empty wrappers, while surviving text keeps its marks.
    pub fn replace(&self, next: &str) -> String {
        if !self.rich {
            return next.to_owned();
        }
        if next.is_empty() {
            return String::new();
        }
        let (removed, inserted) = changed_span(&self.text, next);
        if removed.is_empty() && inserted.is_empty() {
            return self.source.clone();
        }
        let insertion = self
            .leaves
            .iter()
            .enumerate()
            .filter(|(_, leaf)| {
                leaf.visible.start <= removed.start && leaf.visible.end >= removed.start
            })
            .max_by_key(|(_, leaf)| leaf.marks != Marks::default())
            .map(|(index, _)| index)
            .unwrap_or(self.leaves.len() - 1);
        let mut edits = Vec::new();
        let mut emptied = Vec::new();
        for (index, leaf) in self.leaves.iter().enumerate() {
            let start = removed.start.max(leaf.visible.start);
            let end = removed.end.min(leaf.visible.end);
            if start >= end && index != insertion {
                continue;
            }
            let local_start = start.saturating_sub(leaf.visible.start).min(leaf.text.len());
            let local_end = end.saturating_sub(leaf.visible.start).max(local_start);
            let mut text = leaf.text.clone();
            text.replace_range(
                local_start..local_end,
                if index == insertion { &next[inserted.clone()] } else { "" },
            );
            if text.is_empty() {
                emptied.push(leaf.source.clone());
            }
            let replacement = if index == insertion { &next[inserted.clone()] } else { "" };
            let raw = &self.source[leaf.source.clone()];
            if !leaf.raw && !leaf.marks.code {
                if let (Some(start), Some(end)) = (
                    raw_offset(raw, &leaf.text, local_start),
                    raw_offset(raw, &leaf.text, local_end),
                ) {
                    edits.push((
                        leaf.source.start + start..leaf.source.start + end,
                        escape_text(replacement),
                    ));
                    continue;
                }
            }
            let encoded = if leaf.raw {
                text
            } else if leaf.marks.code {
                inline_code(&text)
            } else {
                escape_text(&text)
            };
            // When raw and decoded text agree, keep escapes and whitespace in
            // the untouched prefix/suffix, including soft line breaks.
            if !leaf.marks.code && self.source[leaf.source.clone()] == leaf.text {
                let (old, new) = changed_span(&leaf.text, &encoded);
                edits.push((
                    leaf.source.start + old.start..leaf.source.start + old.end,
                    encoded[new].to_owned(),
                ));
            } else {
                edits.push((leaf.source.clone(), encoded));
            }
        }
        for container in &self.containers {
            let children: Vec<_> = self
                .leaves
                .iter()
                .filter(|leaf| {
                    container.start <= leaf.source.start && container.end >= leaf.source.end
                })
                .collect();
            if !children.is_empty() && children.iter().all(|leaf| emptied.contains(&leaf.source)) {
                if edits
                    .iter()
                    .any(|(span, _)| span.start <= container.start && span.end >= container.end)
                {
                    continue;
                }
                edits.retain(|(span, _)| {
                    !(container.start <= span.start && container.end >= span.end)
                });
                edits.push((container.clone(), String::new()));
            }
        }
        edits.sort_by_key(|(range, _)| range.start);
        let mut source = self.source.clone();
        for (range, text) in edits.into_iter().rev() {
            source.replace_range(range, &text);
        }
        source
    }
}

fn raw_offset(raw: &str, text: &str, offset: usize) -> Option<usize> {
    if raw == text {
        return Some(offset);
    }
    let (mut source, mut visible) = (0, 0);
    while visible < offset {
        let tail = &raw[source..];
        if tail.as_bytes().first() == Some(&b'\\')
            && tail.as_bytes().get(1).is_some_and(u8::is_ascii_punctuation)
        {
            source += 2;
            visible += 1;
        } else if tail.starts_with("\r\n") {
            source += 2;
            visible += 1;
        } else if tail.as_bytes().first() == Some(&b'&') {
            let length = tail.find(';').filter(|end| *end < 40).map(|end| end + 1);
            if let Some(length) = length {
                let decoded = html_escape::decode_html_entities(&tail[..length]);
                if decoded.as_ref() != &tail[..length]
                    && text[visible..].starts_with(decoded.as_ref())
                {
                    if visible + decoded.len() > offset {
                        return Some(source);
                    }
                    source += length;
                    visible += decoded.len();
                    continue;
                }
            }
            source += 1;
            visible += 1;
        } else {
            let ch = tail.chars().next()?;
            if text[visible..].chars().next() != Some(ch) {
                return None;
            }
            source += ch.len_utf8();
            visible += ch.len_utf8();
        }
    }
    (visible == offset).then_some(source)
}

fn inline_code_content(raw: &str) -> Option<Range<usize>> {
    let fence = raw.bytes().take_while(|byte| *byte == b'`').count();
    if fence == 0 || raw.len() < fence * 2 {
        return None;
    }
    let mut start = fence;
    let mut end = raw.len() - fence;
    if start < end
        && raw.as_bytes()[start] == b' '
        && raw.as_bytes()[end - 1] == b' '
        && raw[start..end].bytes().any(|byte| byte != b' ')
    {
        start += 1;
        end -= 1;
    }
    Some(start..end)
}

fn inline_code_visible_offset(raw: &str, text: &str, offset: usize) -> Option<usize> {
    let content = inline_code_content(raw)?;
    if offset <= content.start {
        return Some(0);
    }
    if offset >= content.end {
        return Some(text.len());
    }
    let mut source = content.start;
    let mut visible = 0;
    while source < offset {
        let tail = &raw[source..content.end];
        let (source_length, visible_length) = if tail.starts_with("\r\n") {
            (2, 1)
        } else if tail.as_bytes().first().is_some_and(|byte| matches!(byte, b'\r' | b'\n')) {
            (1, 1)
        } else {
            let ch = tail.chars().next()?;
            (ch.len_utf8(), ch.len_utf8())
        };
        if source + source_length > offset || visible + visible_length > text.len() {
            return None;
        }
        source += source_length;
        visible += visible_length;
    }
    (source == offset).then_some(visible)
}

fn inline_code_source_offset(raw: &str, text: &str, offset: usize) -> Option<usize> {
    let content = inline_code_content(raw)?;
    if offset == 0 {
        return Some(content.start);
    }
    if offset >= text.len() {
        return Some(content.end);
    }
    let mut source = content.start;
    let mut visible = 0;
    while source < content.end {
        if visible >= offset {
            return Some(source);
        }
        let tail = &raw[source..content.end];
        let (source_length, visible_length) = if tail.starts_with("\r\n") {
            (2, 1)
        } else if tail.as_bytes().first().is_some_and(|byte| matches!(byte, b'\r' | b'\n')) {
            (1, 1)
        } else {
            let ch = tail.chars().next()?;
            (ch.len_utf8(), ch.len_utf8())
        };
        if visible + visible_length > text.len() {
            return None;
        }
        source += source_length;
        visible += visible_length;
    }
    (visible == offset).then_some(source)
}

fn visible_from_source(raw: &str, text: &str, offset: usize) -> Option<usize> {
    if raw == text {
        return Some(offset.min(raw.len()));
    }
    let mut source = 0;
    let mut visible = 0;
    while source < offset {
        let tail = &raw[source..];
        let (source_length, visible_length) = if tail.as_bytes().first() == Some(&b'\\')
            && tail.as_bytes().get(1).is_some_and(u8::is_ascii_punctuation)
        {
            (2, 1)
        } else if tail.starts_with("\r\n") {
            (2, 1)
        } else if tail.as_bytes().first() == Some(&b'&') {
            let length = tail.find(';').filter(|end| *end < 40).map(|end| end + 1);
            if let Some(length) = length {
                let decoded = html_escape::decode_html_entities(&tail[..length]);
                if decoded.as_ref() != &tail[..length]
                    && text[visible..].starts_with(decoded.as_ref())
                {
                    (length, decoded.len())
                } else {
                    let ch = tail.chars().next()?;
                    (ch.len_utf8(), ch.len_utf8())
                }
            } else {
                let ch = tail.chars().next()?;
                (ch.len_utf8(), ch.len_utf8())
            }
        } else {
            let ch = tail.chars().next()?;
            if text[visible..].chars().next() != Some(ch) {
                return None;
            }
            (ch.len_utf8(), ch.len_utf8())
        };
        if source + source_length > offset {
            // A source offset inside an escape, entity, or line-ending token
            // has no distinct projected position; keep it at the token start.
            return Some(visible);
        }
        source += source_length;
        visible += visible_length;
    }
    (source == offset).then_some(visible)
}

fn escape_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '`' | '~' | '&' | '#' | '!') {
            result.push('\\');
        }
        result.push(ch);
    }
    result
}

fn inline_code(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let length = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0) + 1;
    let fence = "`".repeat(length);
    let padding = if text.as_bytes().first().is_some_and(|byte| matches!(byte, b'`' | b' '))
        || text.as_bytes().last().is_some_and(|byte| matches!(byte, b'`' | b' '))
    {
        " "
    } else {
        ""
    };
    format!("{fence}{padding}{text}{padding}{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatted_text_edits_preserve_delimiters_and_destinations() {
        for (source, visible, next, expected) in [
            ("## 中文 **标题** ##", "中文 标题", "中文 新标题", "## 中文 **新标题** ##"),
            ("Title\n===", "Title", "New Title", "New Title\n==="),
            (
                "a **bold** [label](url \"title\") z",
                "a bold label z",
                "a bolder label z",
                "a **bolder** [label](url \"title\") z",
            ),
            ("a &amp; b", "a & b", "a & c", "a &amp; c"),
            ("`code`", "code", "co`de", "``co`de``"),
        ] {
            let projection = Projection::new(source);
            assert_eq!(projection.text, visible);
            let actual = projection.replace(next);
            assert_eq!(actual, expected);
            assert_eq!(Projection::new(&actual).text, next);
        }
    }

    #[test]
    fn deletion_across_nested_marks_leaves_valid_surviving_text() {
        let projection = Projection::new("a **bold *inner*** and [link](dest) z");
        assert_eq!(projection.text, "a bold inner and link z");
        let source = projection.replace("a z");
        assert_eq!(source, "a z");
        assert_eq!(Projection::new(&source).text, "a z");
    }

    #[test]
    fn formatting_commands_keep_selection_text_and_unrelated_links() {
        let original = "A 中文 title and [link](destination)";
        let formatted = Projection::new(original).toggle_mark(2..8, "**").unwrap();
        assert_eq!(formatted, "A **中文** title and [link](destination)");
        assert_eq!(Projection::new(&formatted).toggle_mark(2..8, "**").unwrap(), original);
    }

    #[test]
    fn typing_spaces_does_not_duplicate_invisible_markdown_whitespace() {
        for initial in ["", "# ", "**start**"] {
            let mut projection = Projection::new(initial);
            let mut text = projection.text.clone();
            let mut source = initial.to_owned();
            for ch in " new paragraph".chars() {
                text.push(ch);
                source = projection.replace(&text);
                assert!(projection.accept(&text, &source));
                assert_eq!(projection.text, text);
            }
            assert!(!source.ends_with(' '), "{source:?}");
        }
    }

    #[test]
    fn unchanged_and_unsupported_blocks_round_trip_exactly() {
        for source in [
            "# A  #",
            "| a | b |\n| - | - |\n| 1 | 2 |",
            "$$\nx^2\n$$",
            "- a\n- b",
            "a \\*literal\\* &copy; b",
        ] {
            let projection = Projection::new(source);
            assert_eq!(projection.replace(&projection.text), source);
        }
    }

    #[test]
    fn unicode_edits_do_not_split_codepoints() {
        for (before, after) in [("中😀文", "中🌿文"), ("abc", ""), ("", "新段落"), ("same", "same")]
        {
            let (old, new) = changed_span(before, after);
            let mut source = before.to_owned();
            source.replace_range(old, &after[new]);
            assert_eq!(source, after);
        }
    }

    #[test]
    fn reveal_exposes_only_the_clicked_formatted_inline() {
        let source = "left **粗体** middle [link](url \"title\") right";
        let folded = Projection::new(source);
        assert_eq!(folded.text, "left 粗体 middle link right");

        let bold_visible = folded.text.find("粗体").unwrap();
        let bold_start = source.find("**粗体**").unwrap();
        let bold = bold_start..bold_start + "**粗体**".len();
        assert_eq!(folded.reveal_at(bold_visible), Some(bold.clone()));

        let revealed = Projection::with_reveal(source, Some(bold.clone()));
        assert_eq!(revealed.text, "left **粗体** middle link right");
        assert_eq!(revealed.revealed(), Some(bold));
        assert_eq!(&revealed.text[0..5], "left ");
        assert_eq!(&revealed.text[5.."left **粗体**".len()], "**粗体**");

        let link_visible = revealed.text.find("link").unwrap();
        let link_start = source.find("[link](url \"title\")").unwrap();
        let link = link_start..link_start + "[link](url \"title\")".len();
        assert_eq!(revealed.reveal_at(link_visible), Some(link.clone()));
        let link_revealed = Projection::with_reveal(source, Some(link.clone()));
        assert_eq!(link_revealed.text, "left 粗体 middle [link](url \"title\") right");
        assert_eq!(link_revealed.revealed(), Some(link));
    }

    #[test]
    fn reveal_picks_the_innermost_nested_mark() {
        let source = "**outer *inner***";
        let folded = Projection::new(source);
        assert_eq!(folded.text, "outer inner");

        let outer = 0..source.len();
        let inner_start = source.find("*inner*").unwrap();
        let inner = inner_start..inner_start + "*inner*".len();
        let inner_visible = folded.text.find("inner").unwrap();
        assert_eq!(folded.reveal_at(inner_visible), Some(inner.clone()));
        assert_eq!(folded.reveal_at(folded.text.find("outer").unwrap()), Some(outer.clone()));

        let inner_revealed = Projection::with_reveal(source, Some(inner.clone()));
        assert_eq!(inner_revealed.text, "outer *inner*");
        assert_eq!(
            inner_revealed.reveal_at(inner_revealed.text.find("inner").unwrap()),
            Some(inner)
        );
        assert_eq!(inner_revealed.reveal_at(0), Some(outer));
    }

    #[test]
    fn inline_code_offsets_skip_fences_padding_and_preserve_unicode() {
        for (raw, text) in [
            ("`code`", "code"),
            ("` code `", "code"),
            ("``co`de``", "co`de"),
            ("`` 中😀 ``", "中😀"),
        ] {
            let content = inline_code_content(raw).unwrap();
            assert_eq!(inline_code_visible_offset(raw, text, content.start), Some(0));
            assert_eq!(inline_code_source_offset(raw, text, 0), Some(content.start));
            assert_eq!(inline_code_visible_offset(raw, text, content.end), Some(text.len()));
            assert_eq!(inline_code_source_offset(raw, text, text.len()), Some(content.end));
        }

        let source = "prefix `` 中😀 `` suffix";
        let projection = Projection::new(source);
        let leaf = projection.leaves.iter().find(|leaf| leaf.marks.code).unwrap();
        let content = inline_code_content(&source[leaf.source.clone()]).unwrap();
        for local in [content.start, content.start + "中".len()] {
            let source_offset = leaf.source.start + local;
            let visible_offset = projection.visible_offset(source_offset);
            assert_eq!(projection.source_offset(visible_offset), source_offset);
        }
        assert_eq!(projection.visible_offset(leaf.source.end), leaf.visible.end);
        // At a shared visible boundary the following leaf owns the caret, so
        // walking out of code can collapse its delimiters without re-entering it.
        assert_eq!(projection.visible_offset(leaf.source.start + content.end), leaf.visible.end);
        assert_eq!(projection.source_offset(leaf.visible.end), leaf.source.end);
        let trailing = Projection::new("`` 中😀 ``");
        assert_eq!(trailing.source_offset(trailing.text.len()), "`` 中😀".len());
    }

    #[test]
    fn source_visible_offsets_clamp_escapes_entities_and_unicode() {
        let source = "a \\*x\\* &amp; 中😀";
        let projection = Projection::new(source);
        assert_eq!(projection.text, "a *x* & 中😀");

        let escaped = source.find("\\*").unwrap();
        let star = projection.text.find('*').unwrap();
        assert_eq!(projection.visible_offset(escaped), star);
        assert_eq!(projection.visible_offset(escaped + 1), star);
        assert_eq!(projection.source_offset(star), escaped);
        assert_eq!(projection.source_offset(star + 1), escaped + 2);

        let entity = source.find("&amp;").unwrap();
        let ampersand = projection.text.find('&').unwrap();
        assert_eq!(projection.visible_offset(entity), ampersand);
        assert_eq!(projection.visible_offset(entity + 2), ampersand);
        assert_eq!(projection.source_offset(ampersand), entity);
        assert_eq!(projection.source_offset(ampersand + 1), entity + "&amp;".len());

        for visible in projection
            .text
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(projection.text.len()))
        {
            let source_offset = projection.source_offset(visible);
            assert!(source.is_char_boundary(source_offset));
            assert_eq!(projection.visible_offset(source_offset), visible);
        }
    }

    #[test]
    fn image_and_math_fallbacks_have_local_reveal_ranges() {
        for source in ["before ![alt](image.png) after", "before $x^2$ after"] {
            let projection = Projection::new(source);
            let marker = if source.contains("![") { "![alt](image.png)" } else { "$x^2$" };
            let start = source.find(marker).unwrap();
            let range = start..start + marker.len();
            let visible = projection.text.find(marker).unwrap();
            assert_eq!(projection.reveal_at(visible), Some(range.clone()));
            let revealed = Projection::with_reveal(source, Some(range.clone()));
            assert_eq!(revealed.text, projection.text);
            assert_eq!(revealed.revealed(), Some(range));
        }
    }

    #[test]
    fn invalid_reveal_edit_keeps_the_raw_leaf_active() {
        let source = "before **bold** after";
        let start = source.find("**bold**").unwrap();
        let reveal = start..start + "**bold**".len();
        let mut projection = Projection::with_reveal(source, Some(reveal.clone()));
        let edited = "before **bold* after";
        assert!(projection.accept(edited, edited));
        assert_eq!(projection.text, edited);
        let edited_range = edited.find("**bold*").unwrap();
        assert_eq!(projection.revealed(), Some(edited_range..edited_range + "**bold*".len()));
    }

    #[test]
    fn replacing_a_revealed_inline_and_its_neighbour_keeps_the_input_projection() {
        let source = "before **bold** after";
        let start = source.find("**bold**").unwrap();
        let reveal = start..start + "**bold**".len();
        let mut projection = Projection::with_reveal(source, Some(reveal.clone()));
        assert!(projection.accept("before after", "before after"));
        assert_eq!(projection.text, "before after");
        assert_eq!(projection.revealed(), None);
    }
}
