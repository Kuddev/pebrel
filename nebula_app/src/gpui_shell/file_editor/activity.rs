//! Rendering belongs to the visible tab. Drafts, undo and the structural index
//! outlive its pixel/text-view caches, so resuming an unchanged tab needs no parse.

use super::*;

pub(super) struct EditCursor {
    pub(super) anchor: usize,
    pub(super) selection: std::ops::Range<usize>,
}

impl TextFileView {
    pub(in crate::gpui_shell) fn set_render_active(
        &mut self,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.render_active == active {
            return;
        }
        self.render_active = active;
        if !active {
            self.update_live_edit(window, cx);
            self.finish_live_edit(cx);
            self.resume_edit = self.last_edit_cursor.is_some();
            self.preview_task = None;
            self.revision += 1;
            self.stop_preview_selection_scroll();
            self.inline_views.borrow_mut().clear();
            for block in self.blocks.borrow_mut().iter_mut() {
                *block = None;
            }
            self.preview_scrollbar_hovered = false;
        } else if self.preview_stale {
            self.schedule_preview(cx);
        }
        self.preview_images.update(cx, |images, cx| {
            images.set_active(active && self.preview, window, cx);
        });
        cx.notify();
    }

    pub(super) fn restore_edit_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.resume_edit || !self.render_active || self.preview_stale || self.loading {
            return;
        }
        self.resume_edit = false;
        let Some(cursor) = self.last_edit_cursor.take() else { return };
        let block = self.outline.edit_block_at(cursor.anchor.min(self.input.read(cx).text().len()));
        self.resume_live_at(block, cursor.anchor, window, cx);
        if let Some(edit) = &self.live_edit {
            let start = edit.projection.visible_offset(cursor.selection.start);
            let end = edit.projection.visible_offset(cursor.selection.end);
            edit.input.update(cx, |input, cx| input.set_selected_range(start..end, cx));
        }
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn inactive_document_keeps_draft_undo_and_scroll_but_drops_views_and_pending_parse(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("activity.md");
        let original = format!("# 中文标题\n\n正文\n\n{}", "后面的段落\n\n".repeat(60));
        std::fs::write(&path, &original).unwrap();
        let (file, mut cx) = super::super::tests::open_live(path.clone(), cx);
        cx.update(|window, cx| file.update(cx, |view, cx| view.begin_live_edit(1, window, cx)));
        cx.simulate_input("未保存🌿");
        cx.run_until_parked();
        let expected = file.read_with(&cx, |view, cx| view.draft(cx));
        assert_ne!(expected.as_ref(), original);
        let (cached, top) = cx.update(|window, cx| {
            file.update(cx, |view, cx| {
                let cached = cx.new(|cx| TextViewState::markdown("temporary layout", cx));
                let weak = cached.downgrade();
                view.blocks.borrow_mut()[0] = Some(cached);
                view.scroll.scroll_to(gpui::ListOffset { item_ix: 1, offset_in_item: px(4.0) });
                let top = view.scroll.logical_scroll_top();
                view.set_render_active(false, window, cx);
                assert!(view.dirty && view.live_edit.is_none());
                assert!(view.preview_task.is_none());
                assert!(!view.preview_selection_scroll_active);
                assert!(view.blocks.borrow().iter().all(Option::is_none));
                (weak, top)
            })
        });
        cx.run_until_parked();
        assert!(cached.upgrade().is_none());
        cx.update(|window, cx| {
            file.update(cx, |view, cx| {
                assert_eq!(view.draft(cx), expected);
                assert!(view.preview_task.is_none() && view.preview_stale);
                view.set_render_active(true, window, cx);
            });
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        cx.update(|window, cx| {
            file.update(cx, |view, cx| {
                assert!(!view.preview_stale);
                assert_eq!(view.scroll.logical_scroll_top().item_ix, top.item_ix);
                assert_eq!(view.scroll.logical_scroll_top().offset_in_item, top.offset_in_item);
                assert_eq!(view.draft(cx), expected);
                view.restore_edit_cursor(window, cx);
                let edit = view.live_edit.as_ref().expect("restore the input caret on resume");
                let end = edit.projection.text.len();
                assert_eq!(edit.input.read(cx).selected_range(), end..end);
                view.travel_history(false, window, cx);
                assert_eq!(view.draft(cx).as_ref(), original);
            });
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }
}
