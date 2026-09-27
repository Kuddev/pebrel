//! Structural editing commands are document transactions, shared with ordinary
//! typing, saving and undo. No container serializes the rest of the document.

use super::*;
use std::ops::Range;

pub(super) fn table_cell_source(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut slashes = 0;
    for ch in source.chars() {
        if ch == '|' && slashes % 2 == 0 {
            result.push('\\');
        }
        match ch {
            '\n' => result.push_str("<br>"),
            '\r' => {},
            _ => result.push(ch),
        }
        slashes = if ch == '\\' { slashes + 1 } else { 0 };
    }
    result
}

impl TextFileView {
    pub(super) fn commit_structure_edit(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        cursor: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.preview_editable() {
            return;
        }
        self.update_live_edit(window, cx);
        let mut source = self.input.read(cx).value().to_string();
        if source.get(range.clone()).is_none() {
            return;
        }
        self.history.record(&source);
        self.finish_live_edit(cx);
        self.history.barrier();
        source.replace_range(range, replacement);
        self.replace_document_text(&source, window, cx);
        self.history.record(&source);
        self.history.barrier();
        let mut outline = Outline::parse(&source);
        let block = cursor.map(|offset| outline.edit_block_at(offset.min(source.len())));
        self.apply_outline(outline, cx);
        if let (Some(block), Some(cursor)) = (block, cursor) {
            self.resume_live_at(block, cursor, window, cx);
            self.scroll.scroll_to_reveal_item(block);
        }
    }

    pub(super) fn resume_live_at(
        &mut self,
        block: usize,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let part =
            self.outline.structures.get(block).and_then(Option::as_ref).and_then(|structure| {
                structure.part_at(cursor.saturating_sub(self.outline.source_ranges[block].start))
            });
        self.begin_live_part_at(block, part, None, window, cx);
        if let Some(edit) = &self.live_edit {
            let cursor = edit.projection.visible_offset(cursor.saturating_sub(edit.range.start));
            edit.input.update(cx, |input, cx| input.set_selected_range(cursor..cursor, cx));
        }
    }

    pub(super) fn toggle_list_check(
        &mut self,
        block: usize,
        check: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(range) = self.outline.source_ranges.get(block) else { return };
        let range = range.start + check.start..range.start + check.end;
        let source = self.input.read(cx).value();
        let replacement =
            if source.get(range.clone()).is_some_and(|value| value.eq_ignore_ascii_case("x")) {
                " "
            } else {
                "x"
            };
        self.commit_structure_edit(range, replacement, None, window, cx);
    }

    pub(super) fn navigate_table(
        &mut self,
        backwards: bool,
        vertical: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(edit) = &self.live_edit else { return false };
        let Some(part) = edit.part else { return false };
        let Some(structure) = self.outline.structures[edit.block].as_ref() else { return false };
        let Some((rows, span)) = structure.root.table(part) else { return false };
        let cells: Vec<_> = rows.iter().flatten().copied().collect();
        let Some(index) = cells.iter().position(|id| *id == part) else { return false };
        let step = if vertical { rows.first().map_or(1, Vec::len) } else { 1 };
        let next = if backwards { index.checked_sub(step) } else { Some(index + step) };
        let block = edit.block;
        if let Some(next) = next.and_then(|next| cells.get(next)).copied() {
            self.update_live_edit(window, cx);
            self.begin_live_part_at(block, Some(next), None, window, cx);
            if let Some(edit) = &self.live_edit {
                edit.input.update(cx, |input, cx| input.set_selected_range(0..0, cx));
            }
        } else if !backwards {
            let columns = rows.first().map_or(1, Vec::len);
            let at = self.outline.source_ranges[block].start + span.end;
            let row = format!("\n|{}", "   |".repeat(columns));
            self.commit_structure_edit(at..at, &row, Some(at + 2), window, cx);
        }
        true
    }

    pub(super) fn continue_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(edit) = &self.live_edit else { return false };
        let Some(part) = edit.part else { return false };
        let Some(structure) = self.outline.structures[edit.block].as_ref() else { return false };
        let Some(item) = structure.root.list_item(part) else { return false };
        let source = self.input.read(cx).value();
        let base = self.outline.source_ranges[edit.block].start;
        let item_start = base + item.range.start;
        let line_start = source[..item_start].rfind('\n').map_or(0, |index| index + 1);
        let indent = &source[line_start..item_start];
        if edit.projection.text.trim().is_empty() {
            let end = base + item.range.end;
            if !indent.is_empty() {
                return self.indent_list(true, window, cx);
            }
            self.commit_structure_edit(line_start..end, "\n", Some(line_start + 1), window, cx);
            return true;
        }
        let prefix = &source[base + item.prefix.start..base + item.prefix.end];
        let mut marker = prefix.to_owned();
        if let Some(check) = &item.check {
            let local = check.start - item.prefix.start;
            marker.replace_range(local..local + 1, " ");
        }
        let digits = marker.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            if let Ok(number) = marker[..digits].parse::<u64>() {
                marker.replace_range(..digits, &(number.saturating_add(1)).to_string());
            }
        }
        let selection = edit.input.read(cx).selected_range();
        let left = edit.projection.replace(&edit.projection.text[..selection.start]);
        let right = edit.projection.replace(&edit.projection.text[selection.end..]);
        let replacement = format!("{left}\n{indent}{marker}{right}");
        let cursor = edit.range.start + left.len() + 1 + indent.len() + marker.len();
        self.commit_structure_edit(edit.range.clone(), &replacement, Some(cursor), window, cx);
        true
    }

    pub(super) fn indent_list(
        &mut self,
        outdent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(edit) = &self.live_edit else { return false };
        let Some(part) = edit.part else { return false };
        let Some(structure) = self.outline.structures[edit.block].as_ref() else { return false };
        let Some(item) = structure.root.list_item(part) else { return false };
        let source = self.input.read(cx).value();
        let base = self.outline.source_ranges[edit.block].start;
        let item_start = base + item.range.start;
        let start = source[..item_start].rfind('\n').map_or(0, |index| index + 1);
        let end = base + item.range.end;
        let indent = item_start - start;
        if outdent && indent == 0 {
            return true;
        }
        // Indent only beneath an existing sibling; otherwise four spaces could
        // turn the first item into an unrelated code block.
        if !outdent && item.range.start == 0 {
            return true;
        }
        let change = if outdent { indent.min(2) } else { 2 };
        let replacement = source[start..end]
            .split('\n')
            .map(|line| {
                if outdent {
                    line[line.bytes().take(change).take_while(|b| *b == b' ').count()..].to_owned()
                } else {
                    format!("{}{line}", " ".repeat(change))
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let cursor = if outdent { edit.range.start - change } else { edit.range.start + change };
        self.commit_structure_edit(start..end, &replacement, Some(cursor), window, cx);
        true
    }

    pub(super) fn backspace_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(edit) = &self.live_edit else { return false };
        if edit.input.read(cx).selected_range() != (0..0) {
            return false;
        }
        let Some(part) = edit.part else { return false };
        let Some(structure) = self.outline.structures[edit.block].as_ref() else { return false };
        let Some(item) = structure.root.list_item(part) else { return false };
        let base = self.outline.source_ranges[edit.block].start;
        let start = base + item.prefix.start;
        let source = self.input.read(cx).value();
        let line = source[..start].rfind('\n').map_or(0, |at| at + 1);
        if line < start {
            return self.indent_list(true, window, cx);
        }
        let end = base + item.prefix.end;
        let prefix = if line > base { "\n" } else { "" };
        self.commit_structure_edit(start..end, prefix, Some(start + prefix.len()), window, cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::table_cell_source;

    #[test]
    fn cell_input_preserves_escaped_pipes_and_uses_inline_breaks() {
        assert_eq!(table_cell_source("a|b\\|c\nd"), "a\\|b\\|c<br>d");
    }
}
