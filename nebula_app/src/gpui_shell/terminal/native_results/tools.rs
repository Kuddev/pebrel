//! 原型中的轻量模式切换和复制反馈；浮层不参与结果高度。
use super::*;
use gpui_component::button::ButtonCustomVariant;

impl NativeResult {
    pub(super) fn toolbar_element(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let copied = self.feedback.read(cx).is_copied();
        let foreground = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;
        let target = if self.raw_mode { 0.5 } else { 0.0 };
        let indicator = div()
            .id("native-mode-indicator")
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(target))
            .w(px(63.0))
            .rounded(px(4.0))
            .bg(cx.theme().secondary)
            .border_1()
            .border_color(foreground.opacity(0.12));
        let indicator = crate::gpui_shell::widgets::segmented::animate_segment_indicator(
            format!("native-mode-{}", self.source.upgrade().map_or(0, |source| source.id)).into(),
            indicator,
            target,
            window,
            cx,
        );
        let variant = || {
            ButtonCustomVariant::new(cx)
                .foreground(muted)
                .hover(foreground.opacity(0.04))
                .active(foreground.opacity(0.08))
        };
        let label = |icon, message| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(5.0))
                .font_family(cx.theme().font_family.clone())
                .text_size(px(11.0))
                .line_height(px(16.0))
                .child(Icon::new(icon).size(px(14.0)))
                .child(language.text(message))
        };
        let modes = div().p(px(1.0)).rounded(px(6.0)).bg(cx.theme().background.opacity(0.5)).child(
            div()
                .relative()
                .flex()
                .w(px(126.0))
                .h(px(28.0))
                .child(indicator)
                .child(
                    Button::new("native-view")
                        .debug_selector(|| "native-result-view".into())
                        .small()
                        .h(px(28.0))
                        .w(px(63.0))
                        .px_0()
                        .rounded(px(4.0))
                        .custom(variant())
                        .toggled(!self.raw_mode)
                        .tooltip(language.text(Message::NativeResultView))
                        .child(label(IconName::Eye, Message::NativeResultView))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.raw_mode = false;
                            window.focus(&this.focus, cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("native-raw")
                        .debug_selector(|| "native-result-raw".into())
                        .small()
                        .h(px(28.0))
                        .w(px(63.0))
                        .px_0()
                        .rounded(px(4.0))
                        .custom(variant())
                        .toggled(self.raw_mode)
                        .tooltip(language.text(Message::NativeResultRaw))
                        .child(label(IconName::Code, Message::NativeResultRaw))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.raw_mode = true;
                            window.focus(&this.focus, cx);
                            cx.notify();
                        })),
                ),
        );
        let top = if self.visible_top > 0.0 {
            self.visible_top
        } else {
            -self.viewport_top.max(0.0).min(38.0)
        };
        let mut right: f32 = if self.viewport_top + top < 40.0 && self.available_width <= px(900.0)
        {
            36.0
        } else {
            3.0
        };
        if self.mime.as_ref() == "text/x-diff" && top > -36.0 {
            // 贴近视口顶端时浮层不能移到结果上方，须让出文件头右侧折叠按钮的完整命中区。
            right = right.max(68.0);
        }
        let table = matches!(
            self.mime.as_ref(),
            "application/json" | "text/x-directory-listing" | "text/x-powershell-directory"
        );
        div()
            .id("native-result-tools")
            .debug_selector(|| "native-result-tools".into())
            .absolute()
            .top(px(top))
            .right(px(right))
            .flex()
            .items_center()
            .gap(px(3.0))
            .p(px(3.0))
            .rounded(px(9.0))
            .bg(crate::gpui_shell::theme::settings_panel_bg(cx))
            .border_1()
            .border_color(foreground.opacity(0.12))
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if this.toolbar_hovered != *hovered {
                    this.toolbar_hovered = *hovered;
                    cx.notify();
                }
            }))
            .when(self.raw_mode && self.mime.as_ref() == "text/x-git-progress", |tools| {
                tools
                    .child(
                        Button::new("native-stdout")
                            .label("stdout")
                            .ghost()
                            .small()
                            .selected(!self.raw_stderr)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.raw_stderr = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("native-stderr")
                            .label("stderr")
                            .ghost()
                            .small()
                            .selected(self.raw_stderr)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.raw_stderr = true;
                                cx.notify();
                            })),
                    )
            })
            .child(modes)
            .child(div().w(px(1.0)).h(px(14.0)).mx(px(3.0)).bg(foreground.opacity(0.09)))
            .when(table, |tools| {
                tools.child(
                    Button::new("native-filter")
                        .debug_selector(|| "native-result-filter-toggle".into())
                        .small()
                        .h(px(30.0))
                        .w(px(30.0))
                        .px_0()
                        .rounded(px(5.0))
                        .custom(variant())
                        .tooltip(language.text(Message::NativeResultFilter))
                        .toggled(self.filter_open)
                        .disabled(self.table.is_none())
                        .child(Icon::new(IconName::Search).size(px(14.0)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.filter_open = !this.filter_open;
                            this.raw_mode = false;
                            if this.filter_open {
                                this.filter.update(cx, |input, cx| input.focus(window, cx));
                            } else {
                                this.filter.update(cx, |input, cx| input.set_value("", window, cx));
                                this.order_table(cx);
                                window.focus(&this.focus, cx);
                            }
                            cx.notify();
                        })),
                )
            })
            .child(
                Button::new("native-copy")
                    .debug_selector(|| "native-result-copy".into())
                    .small()
                    .h(px(30.0))
                    .min_w(px(30.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .custom(variant())
                    .disabled(self.status == Status::Evicted)
                    .tooltip(language.text(if copied {
                        Message::NativeResultCopied
                    } else {
                        Message::NativeResultCopy
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .font_family(cx.theme().font_family.clone())
                            .text_size(px(11.0))
                            .text_color(if copied { cx.theme().success } else { muted })
                            .child(
                                Icon::new(if copied { IconName::Check } else { IconName::Copy })
                                    .size(px(14.0)),
                            )
                            .when(copied, |button| {
                                button.child(language.text(Message::NativeResultCopied))
                            }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.copy_source(cx))),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .into_any_element()
    }
}
