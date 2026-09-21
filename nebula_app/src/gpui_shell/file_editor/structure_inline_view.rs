//! Native editing of one run in a paragraph containing embedded objects. The
//! remaining runs keep their normal text, image and formula presentation.

use super::block_structure::{BlockStructure, InlineRun, PartKind};
use super::*;
use gpui_component::text::{TextView, TextViewStyle};

impl TextFileView {
    pub(super) fn render_inline_parts(
        &self,
        runs: &[InlineRun],
        structure: &BlockStructure,
        source: &str,
        block: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut line = h_flex().w_full().min_w_0().flex_wrap().items_end();
        for run in runs {
            let part = run.part;
            let spec = &structure.parts[part];
            let active = self
                .live_edit
                .as_ref()
                .filter(|edit| edit.block == block && edit.part == Some(part));
            let mut frame = div()
                .id(("inline-edit-part", part))
                .debug_selector(move || format!("markdown-inline-part-{block}-{part}"))
                .min_w(px(2.0))
                .max_w_full()
                .flex_shrink_0()
                .cursor_text()
                .text_size(px(reader_presentation::heading_size(spec.heading)))
                .when(spec.heading.is_some(), |frame| frame.font_semibold())
                .when(run.marks.bold, |frame| frame.font_bold())
                .when(run.marks.italic, |frame| frame.italic())
                .when(run.marks.link, |frame| frame.text_color(cx.theme().link))
                .on_click(cx.listener(move |view, event, window, cx| {
                    view.begin_live_part_at(block, Some(part), Some(event), window, cx);
                    cx.stop_propagation();
                }));
            if let Some(edit) = active {
                self.inline_views.borrow_mut().remove(&(block, part));
                // Only the active run needs an input. Its unwrapped glyph width
                // lets short formulas remain inline; the parent caps long edits
                // at the reading column and the native input soft-wraps them.
                let mut style = window.text_style();
                style.font_family = if matches!(spec.kind, PartKind::Source) {
                    cx.theme().mono_font_family.clone()
                } else {
                    cx.theme().font_family.clone()
                };
                if run.marks.bold {
                    style.font_weight = gpui::FontWeight::BOLD;
                }
                if run.marks.italic {
                    style.font_style = gpui::FontStyle::Italic;
                }
                let font_size = px(if matches!(spec.kind, PartKind::Source) {
                    13.0
                } else {
                    reader_presentation::heading_size(spec.heading)
                });
                let width = edit
                    .projection
                    .text
                    .lines()
                    .map(|text| {
                        window
                            .text_system()
                            .shape_line(
                                text.to_owned().into(),
                                font_size,
                                &[style.to_run(text.len())],
                                None,
                            )
                            .width
                    })
                    .fold(px(0.0), |width, next| width.max(next));
                frame = frame.w((width + px(12.0)).max(px(40.0))).child(self.render_live_input(cx));
            } else {
                let raw = &source[spec.range.clone()];
                let text = self.outline.part_source(block, spec.preview.as_deref().unwrap_or(raw));
                let owner = cx.weak_entity();
                let state = window.use_keyed_state(
                    gpui::SharedString::from(format!("inline-state-{block}-{part}")),
                    cx,
                    |_, cx| TextViewState::markdown(&text, cx),
                );
                state.update(cx, |state, cx| state.set_text(&text, cx));
                self.inline_views.borrow_mut().insert((block, part), state.downgrade());
                // A fragment's Markdown parser owns only its content. Spaces
                // between a text run and an object still belong to the outer
                // paragraph, including when a run contains only whitespace.
                let leading = raw.len() - raw.trim_start_matches([' ', '\t', '\n', '\r']).len();
                let trailing = raw.trim_start_matches([' ', '\t', '\n', '\r']).len()
                    - raw.trim_matches([' ', '\t', '\n', '\r']).len();
                let style = window.text_style();
                let space = window
                    .text_system()
                    .shape_line(
                        " ".into(),
                        px(reader_presentation::heading_size(spec.heading)),
                        &[style.to_run(1)],
                        None,
                    )
                    .width;
                frame = frame.pl(space * leading as f32).pr(space * trailing as f32);
                frame = frame.child(
                    TextView::new(&state)
                        .min_w_0()
                        .max_w_full()
                        .selectable(true)
                        .scrollable(false)
                        .style(TextViewStyle {
                            paragraph_gap: gpui::rems(0.0),
                            image_base: self.path.parent().map(std::sync::Arc::from),
                            highlight_theme: cx.theme().highlight_theme.clone(),
                            is_dark: cx.theme().is_dark(),
                            inline_code: gpui::HighlightStyle {
                                background_color: Some(super::super::theme::code_block_background(
                                    cx,
                                )),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                        .on_link_click(move |url, event, window, cx| {
                            if event.modifiers().control || event.modifiers().platform {
                                cx.open_url(url);
                            } else {
                                let _ = owner.update(cx, |view, cx| {
                                    view.begin_live_part_at(
                                        block,
                                        Some(part),
                                        Some(event),
                                        window,
                                        cx,
                                    );
                                });
                            }
                        })
                        .markdown_extensions(self.preview_extensions.clone()),
                );
            }
            line = line.child(frame);
        }
        line.into_any_element()
    }
}
