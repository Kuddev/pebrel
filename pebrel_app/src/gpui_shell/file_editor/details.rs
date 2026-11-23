//! The workspace hosts document navigation in its shared right sidebar.
//! The document still owns headings, folding, selection and draft state.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::gpui_shell) enum DocumentSection {
    Outline,
    Info,
}

pub(in crate::gpui_shell) struct DocumentDetails {
    document: Entity<TextFileView>,
    section: DocumentSection,
    _subscription: Subscription,
}

impl DocumentDetails {
    pub(in crate::gpui_shell) fn new(
        document: Entity<TextFileView>,
        section: DocumentSection,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&document, |_, _, cx| cx.notify());
        Self { document, section, _subscription: subscription }
    }

    pub(in crate::gpui_shell) fn set_section(
        &mut self,
        section: DocumentSection,
        cx: &mut Context<Self>,
    ) {
        if self.section != section {
            self.section = section;
            cx.notify();
        }
    }
}

impl Render for DocumentDetails {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().min_w_0().min_h_0().child(self.document.update(cx, |view, cx| {
            match self.section {
                DocumentSection::Outline => view.render_outline_content(cx),
                DocumentSection::Info => view.render_info_content(cx),
            }
        }))
    }
}

impl TextFileView {
    pub(in crate::gpui_shell) fn has_outline(&self) -> bool {
        self.markdown
    }

    pub(in crate::gpui_shell) fn host_details(&mut self, open: bool, cx: &mut Context<Self>) {
        let open = open && !self.reader_focus;
        if !self.details_hosted || self.show_details != open {
            self.details_hosted = true;
            self.show_details = open;
            self.details_resize_anchor = None;
            cx.notify();
        }
    }
}
