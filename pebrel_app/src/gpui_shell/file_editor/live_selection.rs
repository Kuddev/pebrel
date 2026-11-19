//! Inline source visibility follows the native caret. Changing that visibility
//! never edits the document, adds undo entries, or replaces the input entity.

use super::inline_edit::Projection;
use super::*;
use gpui::EntityInputHandler;

impl TextFileView {
    pub(super) fn sync_live_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.live_edit.as_mut() else { return };
        let input = &edit.input;
        let selection = input.read(cx).selected_range();
        if selection == edit.last_selection {
            return;
        }
        // A range selection (including an in-progress mouse drag) owns its
        // geometry. Do not change its text or its direction under the pointer.
        if !selection.is_empty()
            || !edit.projection.rich
            || input.update(cx, |input, cx| input.marked_text_range(window, cx).is_some())
        {
            edit.last_selection = selection;
            cx.notify();
            return;
        }
        let reveal = edit.projection.reveal_at(selection.start);
        edit.last_selection = selection.clone();
        if reveal == edit.projection.revealed() {
            return;
        }
        let source_cursor = edit.projection.source_offset(selection.start);
        let projection = Projection::with_reveal(&edit.source, reveal);
        let cursor = projection.visible_offset(source_cursor);
        // This is at most the active bounded block. set_value suppresses Change
        // events; the canonical source and shared document history stay intact.
        input.update(cx, |input, cx| {
            input.set_value(projection.text.clone(), window, cx);
            input.set_selected_range(cursor..cursor, cx);
        });
        edit.last_selection = cursor..cursor;
        edit.decorations.set(super::live_edit::decorations(&projection, cx), cx);
        edit.projection = projection;
        let block = edit.block;
        self.invalidate_live_block(block);
        cx.notify();
    }
}
