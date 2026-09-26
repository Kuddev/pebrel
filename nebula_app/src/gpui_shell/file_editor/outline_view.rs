//! Document navigation belongs to Pebrel: hierarchy, folding and row feedback
//! share the same heading model as the reader and source navigation.

use super::*;

impl TextFileView {
    pub(super) fn render_outline_content(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = super::super::config::ui_language(cx);
        let scroll = self.outline_scroll.clone();
        let headings = v_flex()
            .id("markdown-headings")
            .debug_selector(|| "markdown-outline-list".to_owned())
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(12.0))
            .pr(px(20.0))
            .py(px(12.0))
            .when(self.outline.headings.is_empty(), |list| {
                list.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(language.text(Message::EditorNoHeadings)),
                )
            })
            .children(self.outline.visible_headings(&self.collapsed_headings).map(
                |(index, heading)| {
                    let collapsed = self.collapsed_headings.contains(&index);
                    let copied = heading.label.clone();
                    let slot = || {
                        div()
                            .w(px(20.0))
                            .h(px(reader_presentation::OUTLINE_ROW_HEIGHT))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                    };
                    let arrow = if self.outline.has_children(index) {
                        slot()
                            .id(("outline-fold", index))
                            .cursor_pointer()
                            .debug_selector(move || format!("outline-fold-{index}"))
                            .child(
                                Icon::new(if collapsed {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                })
                                .size(px(12.0)),
                            )
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                gpui_component::GlobalState::suppress_text_selection(cx);
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                cx.stop_propagation();
                                if !view.collapsed_headings.remove(&index) {
                                    view.collapsed_headings.insert(index);
                                }
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        slot().into_any_element()
                    };
                    h_flex()
                        .id(("markdown-heading", index))
                        .h(px(reader_presentation::OUTLINE_ROW_HEIGHT))
                        .w_full()
                        .min_w_0()
                        .flex_shrink_0()
                        .items_center()
                        .pl(px(heading.indent as f32 * reader_presentation::OUTLINE_INDENT))
                        .pr(px(8.0))
                        .rounded(px(5.0))
                        .text_size(px(reader_presentation::CHROME_SIZE))
                        .cursor_pointer()
                        .debug_selector(move || format!("outline-row-{index}"))
                        .text_color(if heading.indent == 0 {
                            cx.theme().foreground
                        } else {
                            cx.theme().muted_foreground
                        })
                        .when(self.selected_heading == Some(index), |row| {
                            row.bg(cx.theme().list_hover)
                                .text_color(cx.theme().foreground)
                                .font_semibold()
                        })
                        .hover(|row| row.bg(cx.theme().list_hover))
                        .active(|row| row.bg(cx.theme().list_active))
                        .tab_stop(true)
                        .focus_visible(|row| row.border_1().border_color(cx.theme().ring))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            gpui_component::GlobalState::suppress_text_selection(cx)
                        })
                        .child(arrow)
                        .child(div()
                                .flex_1()
                                .min_w_0()
                                // PR #191: navigation rows stay single-line; the full
                                // authored title remains in the tooltip and copy action.
                                .truncate()
                                .child(heading.label.clone()))
                        .tooltip({
                            let label = heading.label.clone();
                            move |window, cx| {
                                let label = label.clone();
                                Tooltip::element(move |_, _| {
                                    div().max_w(px(560.0)).whitespace_normal().child(label.clone())
                                })
                                .build(window, cx)
                            }
                        })
                        .on_key_down(cx.listener(
                            move |view, event: &gpui::KeyDownEvent, window, cx| {
                                match event.keystroke.key.as_str() {
                                    "enter" | "space" => view.jump_to_heading(index, window, cx),
                                    "left" => {
                                        view.collapsed_headings.insert(index);
                                        cx.notify();
                                    },
                                    "right" => {
                                        view.collapsed_headings.remove(&index);
                                        cx.notify();
                                    },
                                    "escape" => {
                                        view.focus.focus(window, cx);
                                    },
                                    _ => return,
                                }
                                cx.stop_propagation();
                            },
                        ))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.jump_to_heading(index, window, cx)
                        }))
                        .context_menu(move |menu, _, _| {
                            let copied = copied.clone();
                            menu.item(
                                gpui_component::menu::PopupMenuItem::new(
                                    language.text(Message::EditorCopyHeading),
                                )
                                .icon(IconName::Copy)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copied.clone(),
                                    ))
                                }),
                            )
                        })
                },
            ));
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(headings)
            .child(
                div()
                    .id("markdown-outline-scrollbar-host")
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .w(px(16.0))
                    .debug_selector(|| "markdown-outline-scrollbar".to_owned())
                    .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                        view.outline_scrollbar_hovered = *hovered;
                        cx.notify();
                    }))
                    .child(gpui_component::scroll::Scrollbar::vertical(&scroll).scrollbar_show(
                        if self.outline_scrollbar_hovered {
                            gpui_component::scroll::ScrollbarShow::Hover
                        } else {
                            gpui_component::scroll::ScrollbarShow::Scrolling
                        },
                    )),
            )
            .into_any_element()
    }

    pub(super) fn render_outline(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .id("markdown-outline")
            .relative()
            .w(px(reader_presentation::clamp_details_width(self.details_width)))
            .min_w(px(reader_presentation::DETAILS_MIN_WIDTH))
            .max_w(gpui::relative(0.42))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(self.render_details_header(cx))
            .child(self.render_outline_content(cx))
            .child(self.render_details_resize_handle(cx))
            .into_any_element()
    }
}
