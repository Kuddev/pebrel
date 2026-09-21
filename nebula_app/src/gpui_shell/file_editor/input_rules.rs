//! Committed Markdown typing and paste can introduce structure without leaving
//! the document. Composition updates never run these input rules.

use super::inline_edit::{Marks, Projection};
use super::*;
use gpui::EntityInputHandler;
use markdown::mdast::Node;

fn block_prefix(text: &str) -> bool {
    if matches!(text, "- " | "* " | "+ " | "> ") {
        return true;
    }
    if let Some(hashes) = text.strip_suffix(' ') {
        if (1..=6).contains(&hashes.len()) && hashes.bytes().all(|ch| ch == b'#') {
            return true;
        }
        let digits = hashes.trim_end_matches(['.', ')']);
        return digits.len() < hashes.len()
            && !digits.is_empty()
            && digits.len() <= 9
            && digits.bytes().all(|ch| ch.is_ascii_digit());
    }
    false
}

fn enter_block(text: &str) -> Option<(String, usize)> {
    let trimmed = text.trim();
    if trimmed == "$$" {
        return Some(("$$\n\n$$".to_owned(), 3));
    }
    let marker = trimmed.chars().next()?;
    if matches!(marker, '`' | '~') {
        let length = trimmed.chars().take_while(|ch| *ch == marker).count();
        if length >= 3 && !trimmed.contains('\n') {
            let fence = marker.to_string().repeat(length);
            return Some((format!("{trimmed}\n\n{fence}"), trimmed.len() + 1));
        }
    }
    if matches!(trimmed, "---" | "***" | "___") {
        return Some((format!("{trimmed}\n\n"), trimmed.len() + 2));
    }
    None
}

impl TextFileView {
    pub(super) fn apply_input_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.live_edit else { return };
        if !matches!(edit.kind, super::block_structure::PartKind::Rich)
            || edit.projection.revealed().is_some()
            || edit.input.update(cx, |input, cx| input.marked_text_range(window, cx).is_some())
        {
            return;
        }
        let text = edit.input.read(cx).value().to_string();
        let no_marks = edit.projection.marks().all(|(_, marks)| marks == Marks::default());
        if !no_marks {
            return;
        }
        let mut convert = edit.part.is_none()
            && self.outline.heading_level(edit.block).is_none()
            && block_prefix(&text);
        if let Some(part) = edit.part {
            if let Some(structure) = self.outline.structures[edit.block].as_ref() {
                convert |= structure.root.list_item(part).is_some_and(|item| item.check.is_none())
                    && matches!(text.as_str(), "[ ] " | "[x] " | "[X] ");
            }
        } else if text.contains('\n') {
            if let Ok(Node::Root(root)) = markdown::to_mdast(&text, &markdown::ParseOptions::gfm())
            {
                convert |= root.children.len() > 1
                    || root.children.first().is_some_and(|node| {
                        matches!(
                            node,
                            Node::Table(_)
                                | Node::List(_)
                                | Node::Code(_)
                                | Node::Blockquote(_)
                                | Node::Heading(_)
                        )
                    });
            }
        }
        let parsed = Projection::new(&text);
        convert |= parsed.rich
            && parsed.text != text
            && parsed.marks().any(|(_, marks)| marks != Marks::default());
        if convert {
            let cursor = edit.range.start + text.len();
            self.commit_structure_edit(edit.range.clone(), &text, Some(cursor), window, cx);
        }
    }

    pub(super) fn insert_block_on_enter(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(edit) = &self.live_edit else { return false };
        if edit.part.is_some() {
            return false;
        }
        let Some((source, offset)) = enter_block(&edit.input.read(cx).value()) else {
            return false;
        };
        let cursor = edit.range.start + offset;
        self.commit_structure_edit(edit.range.clone(), &source, Some(cursor), window, cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{block_prefix, enter_block};

    #[test]
    fn markdown_prefixes_require_complete_markers_and_keep_fence_language() {
        for prefix in ["# ", "###### ", "- ", "* ", "> ", "12. ", "1) "] {
            assert!(block_prefix(prefix));
        }
        for plain in ["#word", "####### ", "-word", "12 ", "ordinary text"] {
            assert!(!block_prefix(plain));
        }
        assert_eq!(enter_block("```rust"), Some(("```rust\n\n```".to_owned(), 8)));
        assert!(enter_block("ordinary text").is_none());
    }
}
