//! Structured blocks stay mounted in the document's virtual list. Only the
//! focused cell or item owns a native input; surrounding content stays rendered.

use super::block_structure::{BlockStructure, StructureNode};
use super::*;
use gpui_component::text::{TextView, TextViewStyle};
use markdown::mdast::AlignKind;

impl TextFileView {
    pub(super) fn render_structured_block(
        &self,
        block: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let structure = self.outline.structures.get(block)?.as_ref()?.clone();
        let range = self.outline.source_ranges.get(block)?;
        let source = self.source_slice(range.clone(), cx)?;
        Some(self.render_structure_node(&structure.root, &structure, &source, block, window, cx))
    }

    fn render_structure_node(
        &self,
        node: &StructureNode,
        structure: &BlockStructure,
        source: &str,
        block: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match node {
            StructureNode::Inline(runs) => {
                self.render_inline_parts(runs, structure, source, block, window, cx)
            },
            StructureNode::Literal(part) => {
                let part = *part;
                let active = self
                    .live_edit
                    .as_ref()
                    .is_some_and(|edit| edit.block == block && edit.part == Some(part));
                div()
                    .id(("literal-edit-part", part))
                    .w_full()
                    .min_w_0()
                    .cursor_text()
                    .on_click(cx.listener(move |view, event, window, cx| {
                        view.begin_live_part_at(block, Some(part), Some(event), window, cx);
                        cx.stop_propagation();
                    }))
                    .child(if active {
                        self.render_live_input(cx)
                    } else {
                        div()
                            .whitespace_normal()
                            .child(source[structure.parts[part].range.clone()].to_owned())
                            .into_any_element()
                    })
                    .into_any_element()
            },
            StructureNode::Text(part) => {
                self.render_edit_part(*part, structure, source, block, window, cx)
            },
            StructureNode::Code { part, span, language } => {
                let live = self
                    .live_edit
                    .as_ref()
                    .is_some_and(|edit| edit.block == block && edit.part == Some(*part));
                let input = live.then(|| self.render_live_input(cx));
                let spec = super::code_actions::CodeSpec {
                    source: source[structure.parts[*part].range.clone()].to_owned().into(),
                    language: self
                        .preview_code_languages
                        .get(&(block, span.start))
                        .cloned()
                        .or_else(|| (!language.is_empty()).then(|| language.clone().into())),
                    span: Some((span.start, span.end)),
                };
                let part = *part;
                div()
                    .id(("editable-code", part))
                    .cursor_text()
                    .w_full()
                    .min_w_0()
                    .on_click(cx.listener(move |view, event, window, cx| {
                        view.begin_live_part_at(block, Some(part), Some(event), window, cx);
                        cx.stop_propagation();
                    }))
                    .child(super::code_actions::render_with_input(
                        cx.weak_entity(),
                        block,
                        spec,
                        format!("code-{block}-{part}").into(),
                        input,
                        window,
                        cx,
                    ))
                    .into_any_element()
            },
            StructureNode::Math { part, span } => {
                let part = *part;
                let active = self
                    .live_edit
                    .as_ref()
                    .is_some_and(|edit| edit.block == block && edit.part == Some(part));
                let object = !self.live_mode;
                v_flex()
                    .id(("editable-math", part))
                    .debug_selector(move || format!("markdown-math-block-{block}"))
                    .w_full()
                    .min_h(px(40.0))
                    .relative()
                    .justify_center()
                    .cursor_text()
                    .when(object && self.object_editable(), |frame| {
                        frame
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .rounded(px(6.0))
                            .focus_visible(|style| style.bg(cx.theme().muted.opacity(0.35)))
                            .on_key_down(cx.listener(
                                move |view, event: &gpui::KeyDownEvent, window, cx| {
                                    if view.object_edit.as_ref().is_some_and(|edit| {
                                        edit.block == block && edit.part == part
                                    }) {
                                        return;
                                    }
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        view.begin_object_edit(block, part, None, window, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                    })
                    .when(
                        self.object_edit
                            .as_ref()
                            .is_some_and(|edit| edit.block == block && edit.part == part),
                        |frame| frame.bg(cx.theme().muted.opacity(0.45)),
                    )
                    .on_click(cx.listener(move |view, event, window, cx| {
                        if object {
                            view.begin_object_edit(block, part, Some(event), window, cx);
                        } else {
                            view.begin_live_part_at(block, Some(part), Some(event), window, cx);
                        }
                        cx.stop_propagation();
                    }))
                    .child(if active {
                        div()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(super::super::theme::code_block_background(cx))
                            .child(self.render_live_input(cx))
                            .into_any_element()
                    } else {
                        TextView::markdown(
                            ("formula-preview", part),
                            self.object_edit
                                .as_ref()
                                .filter(|edit| edit.block == block && edit.part == part)
                                .map(|edit| edit.preview.clone())
                                .unwrap_or_else(|| source[span.clone()].to_owned()),
                        )
                        .w_full()
                        .min_w_0()
                        .scrollable(false)
                        .style(TextViewStyle {
                            paragraph_gap: gpui::rems(0.0),
                            ..Default::default()
                        })
                        .into_any_element()
                    })
                    .when(
                        self.object_edit
                            .as_ref()
                            .is_some_and(|edit| edit.block == block && edit.part == part),
                        |frame| frame.child(self.render_object_editor(window, cx)),
                    )
                    .into_any_element()
            },
            StructureNode::Quote(children) => {
                let mut column = v_flex()
                    .w_full()
                    .min_w_0()
                    .pl_4()
                    .border_l_2()
                    .border_color(cx.theme().border)
                    .gap_2();
                for child in children {
                    column = column.child(
                        self.render_structure_node(child, structure, source, block, window, cx),
                    );
                }
                column.into_any_element()
            },
            StructureNode::List(items) => {
                let mut list = v_flex().w_full().min_w_0().gap(px(3.0));
                for item in items {
                    let marker = if let Some(check) = &item.check {
                        let checked = source
                            .get(check.clone())
                            .is_some_and(|text| text.eq_ignore_ascii_case("x"));
                        let check = check.clone();
                        Button::new(("markdown-task", item.range.start))
                            .ghost()
                            .disabled(!self.preview_editable())
                            .size(px(24.0))
                            .p_0()
                            .flex_shrink_0()
                            .debug_selector(move || {
                                format!("markdown-task-{block}-{}", check.start)
                            })
                            .child(
                                div()
                                    .size(px(14.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .debug_selector(move || format!("markdown-task-box-{block}"))
                                    .rounded(px(3.0))
                                    .border_1()
                                    .border_color(cx.theme().muted_foreground)
                                    .when(checked, |box_| {
                                        box_.bg(cx.theme().primary).child(
                                            Icon::new(IconName::Check)
                                                .size(px(12.0))
                                                .text_color(cx.theme().primary_foreground),
                                        )
                                    }),
                            )
                            .on_click({
                                let check = item.check.clone().unwrap();
                                cx.listener(move |view, _, window, cx| {
                                    view.toggle_list_check(block, check.clone(), window, cx)
                                })
                            })
                            .into_any_element()
                    } else {
                        div()
                            .min_w(px(24.0))
                            .flex_shrink_0()
                            .text_right()
                            .pr_2()
                            .child(item.marker.clone())
                            .into_any_element()
                    };
                    let mut content = v_flex().flex_1().min_w_0().gap(px(3.0));
                    for child in &item.children {
                        content = content.child(
                            self.render_structure_node(child, structure, source, block, window, cx),
                        );
                    }
                    list = list.child(
                        h_flex().w_full().items_start().min_w_0().child(marker).child(content),
                    );
                }
                list.into_any_element()
            },
            StructureNode::Table { rows, align, span } => {
                let columns = align.len().max(1);
                let mut table = v_flex()
                    .min_w(px(columns as f32 * 100.0))
                    .w_full()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(px(6.0))
                    .rounded_t(px(reader_presentation::TABLE_TOP_RADIUS))
                    .overflow_hidden();
                for (row_index, row) in rows.iter().enumerate() {
                    let mut line = h_flex()
                        .w_full()
                        .items_stretch()
                        .when(row_index > 0, |row| row.border_t_1().border_color(cx.theme().border))
                        .when(row_index == 0, |row| row.bg(cx.theme().muted));
                    for column in 0..columns {
                        let cell = div()
                            .flex_1()
                            .min_w_0()
                            .min_h(px(36.0))
                            .px(px(10.0))
                            .py(px(6.0))
                            .when(column > 0, |cell| {
                                cell.border_l_1().border_color(cx.theme().border)
                            })
                            .when(row_index == 0, |cell| cell.font_semibold())
                            .when(align.get(column) == Some(&AlignKind::Right), |cell| {
                                cell.text_right()
                            })
                            .when(align.get(column) == Some(&AlignKind::Center), |cell| {
                                cell.text_center()
                            });
                        line = line.child(if let Some(part) = row.get(column) {
                            cell.child(
                                self.render_edit_part(*part, structure, source, block, window, cx),
                            )
                        } else {
                            cell
                        });
                    }
                    table = table.child(line);
                }
                div()
                    .id(("editable-table", span.start))
                    .w_full()
                    .min_w_0()
                    .overflow_x_scroll()
                    .child(table)
                    .into_any_element()
            },
        }
    }

    fn render_edit_part(
        &self,
        part: usize,
        structure: &BlockStructure,
        source: &str,
        block: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let active = self
            .live_edit
            .as_ref()
            .is_some_and(|edit| edit.block == block && edit.part == Some(part));
        let content = if active {
            self.render_live_input(cx)
        } else {
            let spec = &structure.parts[part];
            let text = self.outline.part_preview(block, spec, &source[spec.range.clone()]);
            let state = window.use_keyed_state(
                gpui::SharedString::from(format!("document-part-{block}-{part}")),
                cx,
                |_, cx| cx.new(|cx| TextViewState::markdown(&text, cx)),
            );
            let state = state.read(cx).clone();
            state.update(cx, |state, cx| state.set_text(&text, cx));
            self.inline_views.borrow_mut().insert((block, part), state.downgrade());
            let hover_source = state.downgrade();
            let hover_owner = cx.weak_entity();
            TextView::new(&state)
                .w_full()
                .min_w_0()
                .selectable(true)
                .scrollable(false)
                .on_link_hover(move |hover, _, cx| {
                    let Some(source) =
                        hover_source.upgrade().map(|source| source.read(cx).source())
                    else {
                        return;
                    };
                    let _ = hover_owner.update(cx, |view, cx| {
                        view.update_link_hover(block, Some(part), &source, hover, cx)
                    });
                })
                .style(TextViewStyle {
                    paragraph_gap: gpui::rems(0.0),
                    image_base: self.path.parent().map(std::sync::Arc::from),
                    highlight_theme: cx.theme().highlight_theme.clone(),
                    is_dark: cx.theme().is_dark(),
                    inline_code: gpui::HighlightStyle {
                        background_color: Some(super::super::theme::inline_code_background(cx)),
                        ..Default::default()
                    },
                    inline_code_radius: px(reader_presentation::INLINE_CODE_RADIUS),
                    ..Default::default()
                })
                .markdown_extensions(self.preview_extensions.clone())
                .into_any_element()
        };
        div()
            .id(("document-edit-part", part))
            .w_full()
            .min_w_0()
            .min_h(px(24.0))
            .cursor_text()
            .debug_selector(move || format!("markdown-part-{block}-{part}"))
            .on_click(cx.listener(move |view, event, window, cx| {
                view.begin_live_part_at(block, Some(part), Some(event), window, cx);
                cx.stop_propagation();
            }))
            .child(content)
            .into_any_element()
    }
}
