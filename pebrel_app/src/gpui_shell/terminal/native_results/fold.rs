//! 目录输出仅折叠中间源行，首尾仍由终端绘制，保留颜色、链接与选择语义。
use super::*;
use gpui_component::button::ButtonCustomVariant;

pub(super) const KEEP: usize = 10;

pub(super) fn is_fold(source: &Surface) -> bool {
    source.has_grid_source() && source.mime.as_ref() == "text/x-terminal-fold"
}

pub(super) fn element(
    view: &mut TerminalView,
    source: &Surface,
    row: f32,
    window: &mut Window,
    cx: &mut Context<TerminalView>,
) -> AnyElement {
    let id = source.id;
    let expanded = view.native_results.fold_expanded(id);
    let count = usize::from(source.rows()).saturating_sub(KEEP * 2).to_string();
    let label = crate::gpui_shell::config::ui_language(cx).format(
        if expanded { Message::NativeResultFoldCollapse } else { Message::NativeResultFoldExpand },
        &[("count", &count)],
    );
    let focus = window
        .use_keyed_state(("terminal-fold-focus", id), cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone();
    let on_down_focus = focus.clone();
    div()
        .id(("terminal-fold", id))
        .debug_selector(|| "terminal-output-fold".into())
        .absolute()
        .left(px(12.0))
        .top(px(8.0) + view.line_height * row)
        .w(view.native_result_width())
        .h(view.line_height)
        .bg(cx.theme().background)
        .track_focus(&focus)
        .child(
            Button::new(("terminal-fold-toggle", id))
                .debug_selector(|| "terminal-output-fold-toggle".into())
                .xsmall()
                .w_full()
                .h(view.line_height)
                .px_0()
                .aria_label(label.clone())
                .custom(
                    ButtonCustomVariant::new(cx)
                        .foreground(cx.theme().muted_foreground)
                        .hover(cx.theme().foreground.opacity(0.04))
                        .active(cx.theme().foreground.opacity(0.08)),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(4.0))
                        .font_family(cx.theme().font_family.clone())
                        .text_size(px(12.0))
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .size(px(14.0)),
                        )
                        .child(label),
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.toggle_output_fold(id, cx);
                })),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, window, cx| {
                window.focus(&on_down_focus, cx);
                cx.stop_propagation();
            }),
        )
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(cx.listener(|view, event, window, cx| {
            view.on_scroll(event, window, cx);
            cx.stop_propagation();
        }))
        .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                window.focus(&view.focus_handle, cx);
                cx.stop_propagation();
            }
        }))
        .into_any_element()
}
