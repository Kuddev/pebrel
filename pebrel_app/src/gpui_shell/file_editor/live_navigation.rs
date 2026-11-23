//! Keyboard movement crosses native-input lifetimes without leaving the
//! formatted document; structural deletions use the same document history.

use super::*;
use gpui::EntityInputHandler;

impl TextFileView {
    pub(super) fn move_live_edge(
        &mut self,
        backwards: bool,
        document_edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.preview {
            return false;
        }
        let Some(edit) = &self.live_edit else { return false };
        if edit.input.update(cx, |input, cx| input.marked_text_range(window, cx).is_some()) {
            return false;
        }
        let selection = edit.input.read(cx).selected_range();
        let end = edit.input.read(cx).value().len();
        if !document_edge
            && (!selection.is_empty() || selection.start != if backwards { 0 } else { end })
        {
            return false;
        }
        let (block, part) = if document_edge {
            let block = if backwards { 0 } else { self.outline.blocks.len().saturating_sub(1) };
            let part =
                self.outline.structures.get(block).and_then(Option::as_ref).and_then(|structure| {
                    if backwards {
                        (!structure.parts.is_empty()).then_some(0)
                    } else {
                        structure.parts.len().checked_sub(1)
                    }
                });
            (block, part)
        } else if let Some(part) = edit.part.filter(|part| {
            if backwards {
                *part > 0
            } else {
                *part + 1 < self.outline.structures[edit.block].as_ref().unwrap().parts.len()
            }
        }) {
            (edit.block, Some(if backwards { part - 1 } else { part + 1 }))
        } else {
            let block = if backwards {
                edit.block.checked_sub(1)
            } else {
                (edit.block + 1 < self.outline.blocks.len()).then_some(edit.block + 1)
            };
            let Some(block) = block else { return false };
            let part =
                self.outline.structures.get(block).and_then(Option::as_ref).and_then(|structure| {
                    if backwards {
                        structure.parts.len().checked_sub(1)
                    } else {
                        (!structure.parts.is_empty()).then_some(0)
                    }
                });
            (block, part)
        };
        self.update_live_edit(window, cx);
        self.begin_live_part_at(block, part, None, window, cx);
        if let Some(edit) = &self.live_edit {
            edit.input.update(cx, |input, cx| {
                let offset = if backwards == document_edge { 0 } else { input.value().len() };
                input.set_selected_range(offset..offset, cx);
            });
        }
        self.scroll.scroll_to_reveal_item(block);
        true
    }

    pub(super) fn join_live_paragraph(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(edit) = &self.live_edit else { return false };
        if edit.part.is_some()
            || !edit.projection.rich
            || edit.input.read(cx).selected_range() != (0..0)
        {
            return false;
        }
        if self.outline.heading_level(edit.block).is_some() {
            let text = super::live_commands::paragraph_source(&edit.source);
            let cursor = edit.range.start;
            self.commit_structure_edit(edit.range.clone(), &text, Some(cursor), window, cx);
            return true;
        }
        let Some(previous) = edit.block.checked_sub(1) else { return false };
        let range = &self.outline.source_ranges[previous];
        let source = self.input.read(cx).value();
        if !super::inline_edit::Projection::new(&source[range.clone()]).rich {
            return false;
        }
        let cursor = range.end;
        self.commit_structure_edit(range.end..edit.range.start, "", Some(cursor), window, cx);
        true
    }
}
