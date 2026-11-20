//! 只在目标结果区域内绘制复制控件；无hover、无操作反馈时不绘制任何工具栏。
use super::*;
use gpui_component::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
};

impl TerminalView {
    pub(in crate::gpui_shell::terminal) fn output_hover_chrome(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.reconcile_output_hover(cx);
        let state = &self.reading.output_hover;
        if state.hovered.is_none() && state.feedback_target.is_none() {
            return div().into_any_element();
        }
        let mut targets = Vec::with_capacity(2);
        if let Some(target) = state.feedback_target {
            targets.push(target);
        }
        if let Some(target) = state.hovered.filter(|target| Some(*target) != state.feedback_target)
        {
            targets.push(target);
        }
        if targets.is_empty() {
            return div().into_any_element();
        }
        let Some(session) = &self.session else {
            return div().into_any_element();
        };
        let geometries = {
            let term = session.term.lock();
            targets
                .into_iter()
                .filter_map(|target| self.output_geometry(&term, target).map(|g| (target, g)))
                .collect::<Vec<_>>()
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        // 容器与终端root同原点；不能作为网格后的普通流式子项，否则按钮会落到屏外。
        let mut overlays = div().absolute().inset_0();
        for (target, geometry) in geometries {
            let is_feedback = state.feedback_target == Some(target);
            let pending =
                is_feedback && state.feedback.as_ref().is_some_and(|f| f.read(cx).is_pending());
            let copied =
                is_feedback && state.feedback.as_ref().is_some_and(|f| f.read(cx).is_copied());
            let issue = if is_feedback { state.notice } else { None };
            let label = language.text(if copied {
                Message::TerminalReadingCopied
            } else if pending {
                Message::TerminalReadingCopying
            } else {
                Message::TerminalReadingCopy
            });
            let tooltip = language.text(match issue {
                Some(ReadingIssue::Changed) => Message::TerminalReadingChanged,
                Some(ReadingIssue::Unavailable) => Message::TerminalReadingUnavailable,
                Some(ReadingIssue::QueryTooLong) => Message::TerminalReadingQueryLimit,
                Some(ReadingIssue::CellTooLarge) => Message::TerminalReadingCellLimit,
                Some(ReadingIssue::CopyTooLarge) => Message::TerminalReadingCopyLimit,
                Some(ReadingIssue::EmptyOutput) => Message::TerminalReadingEmpty,
                None if copied => Message::TerminalReadingCopied,
                None => Message::TerminalReadingCopy,
            });
            let mut control = div()
                .id(("terminal-output-copy", target.id))
                .debug_selector(move || format!("terminal-output-copy-{}", target.id))
                .absolute()
                .top(geometry.top)
                .right(px(12.0))
                .max_w(geometry.region.size.width)
                .occlude()
                .rounded_md()
                .bg(cx.theme().popover)
                .child(
                    Button::new(("terminal-output-copy-button", target.id))
                        .small()
                        .ghost()
                        .icon(if copied {
                            IconName::Check
                        } else if issue.is_some() {
                            IconName::Close
                        } else {
                            IconName::Copy
                        })
                        .label(label)
                        .tooltip(tooltip)
                        .disabled(pending)
                        .on_click(cx.listener(move |view, _, window, cx| {
                            // 只在用户点击时归还终端焦点；后台完成绝不抢回别处的输入焦点。
                            window.focus(&view.focus_handle, cx);
                            cx.emit(TerminalViewEvent::FocusRequested);
                            view.copy_hovered_output(target, cx);
                        })),
                );
            // 子Button先消费点击；外围只阻止PTY鼠标处理，不能抢占Input或Button的事件。
            for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                control = control
                    .on_mouse_down(button, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(button, |_, _, cx| cx.stop_propagation());
            }
            overlays = overlays.child(
                control
                    .on_mouse_move(|_, _, cx| cx.stop_propagation())
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation()),
            );
        }
        overlays.into_any_element()
    }
}
