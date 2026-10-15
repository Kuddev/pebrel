//! 默认只显示一条查找栏；范围、大小写和输出操作收进菜单，保持原有输入隔离。
use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    Disableable as _, IconName,
    button::{Button, ButtonVariants as _},
    input::Input,
    menu::DropdownMenu as _,
};

impl TerminalView {
    pub(in crate::gpui_shell::terminal) fn reading_chrome(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let t = |message| language.text(message);
        let Some(panel) = &self.reading.panel else {
            let mut entry = div().absolute().top_1().right_3().occlude().child(
                Button::new("terminal-find")
                    .debug_selector(|| "terminal-find".to_owned())
                    .ghost()
                    .size(px(32.0))
                    .icon(IconName::Search)
                    .aria_label(t(Message::TerminalReadingOpen))
                    .tooltip(t(Message::TerminalReadingShortcut))
                    .on_click(
                        cx.listener(|view, _, window, cx| view.open_reading(false, window, cx)),
                    ),
            );
            // 搜索入口只消费自己的命中区域，不把点击继续交给终端鼠标协议。
            for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                entry = entry
                    .on_mouse_down(button, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(button, |_, _, cx| cx.stop_propagation());
            }
            return div()
                .absolute()
                .inset_0()
                .child(self.output_hover_chrome(cx))
                .child(entry)
                .into_any_element();
        };
        let copied = panel.feedback.read(cx).is_copied();
        let pending = panel.feedback.read(cx).is_pending();
        let selected = panel.selected.map_or(0, |i| i + 1);
        let count = language.format(
            Message::TerminalReadingCount,
            &[("current", &selected.to_string()), ("total", &panel.hits.len().to_string())],
        );
        let title = panel.scope.map_or_else(
            || t(Message::TerminalReadingAll).to_owned(),
            |id| {
                panel
                    .commands
                    .iter()
                    .find(|command| command.id == id)
                    .map(|command| command.command.clone())
                    .filter(|label| !label.is_empty())
                    .unwrap_or_else(|| {
                        language
                            .format(Message::TerminalReadingExecution, &[("id", &id.to_string())])
                    })
            },
        );
        let options = self.reading_options(cx);
        let mut overlay =
            div()
                .id("terminal-reading-overlay")
                .debug_selector(|| "terminal-reading-overlay".to_owned())
                .w_full()
                .max_w(px(440.0))
                .occlude()
                .flex()
                .flex_col()
                .rounded(px(8.0))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .text_color(cx.theme().foreground)
                .text_size(px(13.0))
                .shadow_sm()
                .on_key_down(cx.listener(Self::reading_key))
                .child(
                    div()
                        .debug_selector(|| "terminal-reading-search-row".to_owned())
                        .h(px(40.0))
                        .px(px(5.0))
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .child(
                            div()
                                .id("reading-input-wrap")
                                .debug_selector(|| "reading-input".to_owned())
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .border_b_1()
                                .border_color(cx.theme().border.opacity(0.6))
                                .child(
                                    Input::new(&panel.input)
                                        .appearance(false)
                                        .bordered(false)
                                        .focus_bordered(false)
                                        .text_size(px(13.0)),
                                )
                                .child(panel.focus_line.underline(cx.theme().ring)),
                        )
                        .child(
                            div()
                                .min_w(px(38.0))
                                .flex_shrink_0()
                                .text_right()
                                .text_size(px(11.0))
                                .text_color(cx.theme().muted_foreground)
                                .child(if panel.running { "…".to_owned() } else { count }),
                        )
                        .child(
                            Button::new("reading-previous")
                                .ghost()
                                .size(px(28.0))
                                .icon(IconName::ChevronUp)
                                .aria_label(t(Message::TerminalReadingPrevious))
                                .tooltip(t(Message::TerminalReadingPrevious))
                                .disabled(panel.hits.is_empty())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.navigate_reading_match(true, cx)
                                })),
                        )
                        .child(
                            Button::new("reading-next")
                                .ghost()
                                .size(px(28.0))
                                .icon(IconName::ChevronDown)
                                .aria_label(t(Message::TerminalReadingNext))
                                .tooltip(t(Message::TerminalReadingNext))
                                .disabled(panel.hits.is_empty())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.navigate_reading_match(false, cx)
                                })),
                        )
                        .child(options)
                        .child(
                            Button::new("reading-close")
                                .ghost()
                                .size(px(28.0))
                                .icon(IconName::Close)
                                .aria_label(t(Message::TerminalReadingClose))
                                .tooltip(t(Message::TerminalReadingClose))
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.close_reading(window, cx)
                                })),
                        ),
                )
                .when(panel.scope.is_some(), |overlay| {
                    overlay.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .pb_1()
                            .text_size(px(11.0))
                            .text_color(cx.theme().muted_foreground)
                            .child(div().flex_1().min_w_0().truncate().child(title))
                            .when(copied || pending, |row| {
                                row.child(t(if copied {
                                    Message::TerminalReadingCopied
                                } else {
                                    Message::TerminalReadingCopying
                                }))
                            })
                            .child(
                                Button::new("reading-clear-scope")
                                    .ghost()
                                    .size(px(24.0))
                                    .icon(IconName::Close)
                                    .aria_label(t(Message::TerminalReadingAll))
                                    .tooltip(t(Message::TerminalReadingAll))
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.choose_reading_scope(None, cx)
                                    })),
                            ),
                    )
                });
        if panel.commands_open {
            let mut list = div()
                .id("reading-command-list")
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .max_h(px(180.0))
                .overflow_y_scroll()
                .child(
                    Button::new("reading-all")
                        .small()
                        .label(t(Message::TerminalReadingAll))
                        .on_click(
                            cx.listener(|view, _, _, cx| view.choose_reading_scope(None, cx)),
                        ),
                );
            if panel.commands.is_empty() {
                list = list.child(div().child(t(Message::TerminalReadingNoCommands)));
            }
            for command in &panel.commands {
                let id = command.id;
                let label = if command.command.is_empty() {
                    language.format(Message::TerminalReadingExecution, &[("id", &id.to_string())])
                } else {
                    command.command.clone()
                };
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new(("reading-command", id))
                                .small()
                                .label(label.clone())
                                .tooltip(label)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.choose_reading_scope(Some(id), cx)
                                })),
                        )
                        .child(
                            Button::new(("reading-command-copy", id))
                                .small()
                                .icon(IconName::Copy)
                                .aria_label(t(Message::TerminalReadingCopy))
                                .tooltip(t(Message::TerminalReadingCopy))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.choose_reading_scope(Some(id), cx);
                                    view.copy_reading_command(cx);
                                })),
                        ),
                );
            }
            overlay = overlay.child(list);
        }
        let notice = panel
            .notice
            .map(|issue| match issue {
                ReadingIssue::Changed => Message::TerminalReadingChanged,
                ReadingIssue::Unavailable => Message::TerminalReadingUnavailable,
                ReadingIssue::QueryTooLong => Message::TerminalReadingQueryLimit,
                ReadingIssue::CellTooLarge => Message::TerminalReadingCellLimit,
                ReadingIssue::CopyTooLarge => Message::TerminalReadingCopyLimit,
                ReadingIssue::EmptyOutput => Message::TerminalReadingEmpty,
            })
            .or_else(|| panel.truncated.then_some(Message::TerminalReadingTruncated));
        overlay = overlay.when_some(notice, |overlay, notice| {
            overlay.child(div().px_2().pb_2().text_size(px(11.0)).child(t(notice)))
        });
        // Bubble listeners let the Input/Button consume its own event first; occlude alone
        // does not prevent ancestor terminal handlers from reporting mouse input to a TUI.
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            overlay = overlay
                .on_mouse_down(button, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(button, |_, _, cx| cx.stop_propagation());
        }
        let overlay = overlay
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation());
        // 外层负责可用宽度，避免 max-width:100% 再加 right inset 后越过左边界。
        div()
            .absolute()
            .top_1()
            .left_3()
            .right_3()
            .flex()
            .justify_end()
            .child(overlay)
            .into_any_element()
    }

    fn reading_options(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let panel = self.reading.panel.as_ref().expect("reading panel");
        let language = crate::gpui_shell::config::ui_language(cx);
        let match_case = panel.match_case;
        let unavailable = panel.scope.is_none() || panel.source.is_none();
        let copying = panel.feedback.read(cx).is_pending();
        let owner = cx.weak_entity();
        Button::new("reading-options")
            .ghost()
            .size(px(28.0))
            .icon(IconName::Ellipsis)
            .aria_label(language.text(Message::TerminalReadingOptions))
            .tooltip(language.text(Message::TerminalReadingOptions))
            .dropdown_menu(move |menu, _, _| {
                use gpui_component::menu::PopupMenuItem;
                let case_owner = owner.clone();
                let refresh_owner = owner.clone();
                let scope_owner = owner.clone();
                let copy_owner = owner.clone();
                let start_owner = owner.clone();
                let end_owner = owner.clone();
                menu.item(
                    PopupMenuItem::new(language.text(Message::TerminalReadingMatchCase))
                        .checked(match_case)
                        .on_click(move |_, _, cx| {
                            let _ = case_owner.update(cx, |view, cx| {
                                if let Some(panel) = view.reading.panel.as_mut() {
                                    panel.match_case = !panel.match_case;
                                }
                                view.restart_reading_search(cx);
                            });
                        }),
                )
                .item(PopupMenuItem::new(language.text(Message::TerminalReadingRefresh)).on_click(
                    move |_, _, cx| {
                        let _ =
                            refresh_owner.update(cx, |view, cx| view.restart_reading_search(cx));
                    },
                ))
                .item(PopupMenuItem::new(language.text(Message::TerminalReadingChoose)).on_click(
                    move |_, _, cx| {
                        let _ = scope_owner.update(cx, |view, cx| {
                            view.refresh_reading_commands(cx);
                            if let Some(panel) = view.reading.panel.as_mut() {
                                panel.commands_open = true;
                            }
                            cx.notify();
                        });
                    },
                ))
                .item(
                    PopupMenuItem::new(language.text(Message::TerminalReadingCopy))
                        .disabled(unavailable || copying)
                        .on_click(move |_, _, cx| {
                            let _ = copy_owner.update(cx, |view, cx| view.copy_reading_command(cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(language.text(Message::TerminalReadingStart))
                        .disabled(unavailable)
                        .on_click(move |_, _, cx| {
                            let _ = start_owner
                                .update(cx, |view, cx| view.jump_reading_command(false, cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(language.text(Message::TerminalReadingEnd))
                        .disabled(unavailable)
                        .on_click(move |_, _, cx| {
                            let _ = end_owner
                                .update(cx, |view, cx| view.jump_reading_command(true, cx));
                        }),
                )
            })
            .into_any_element()
    }
}
