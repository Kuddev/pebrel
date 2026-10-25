//! 搜索和本次执行输出的动作；不通过按键模拟驱动终端。
use super::super::reading::{MAX_QUERY_BYTES, ReadingIssue, ReadingPanel, worker};
use super::*;
use crate::gpui_shell::copy_feedback::CopyFeedback;
use crate::i18n::Message;
use futures::{StreamExt as _, channel::mpsc};
use gpui_component::input::{InputEvent, InputState};
use nebula_terminal::term::reading::{ReadingPoint, ReadingSpan};
use std::sync::atomic::Ordering;
mod chrome;
mod output_hover;
#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

impl TerminalView {
    pub(in crate::gpui_shell) fn open_reading(
        &mut self,
        commands: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session.is_none() || self.answer_reader.is_some() {
            return;
        }
        self.clear_output_hover(cx);
        self.selecting = false;
        self.stop_selection_scroll();
        self.scrollbar_drag = None;
        self.completion_viewport.scrollbar_grab = None;
        suggest::popup_dismiss(&mut self.suggest);
        if self.reading.panel.is_none() {
            let language = crate::gpui_shell::config::ui_language(cx);
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(language.text(Message::TerminalReadingPlaceholder))
            });
            let feedback = cx.new(|_| CopyFeedback::new());
            let feedback_events = cx.observe(&feedback, |_, _, cx| cx.notify());
            let input_events =
                cx.subscribe_in(&input, window, |view, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => view.restart_reading_search(cx),
                    InputEvent::PressEnter { shift, .. } => view.navigate_reading_match(*shift, cx),
                    InputEvent::Focus | InputEvent::Blur => {
                        if let Some(panel) = view.reading.panel.as_mut() {
                            panel.focus_line.set_focused(matches!(event, InputEvent::Focus));
                        }
                        cx.notify();
                    },
                });
            self.reading.panel = Some(ReadingPanel {
                input,
                focus_line: Default::default(),
                feedback,
                _input_events: input_events,
                _feedback_events: feedback_events,
                scope: None,
                source: None,
                hits: Vec::new(),
                selected: None,
                match_case: false,
                running: false,
                truncated: false,
                notice: None,
                commands: Vec::new(),
                commands_open: commands,
                highlight: None,
            });
        }
        self.refresh_reading_commands(cx);
        if commands {
            let latest = self.reading.panel.as_ref().and_then(|p| p.commands.first()).map(|c| c.id);
            self.choose_reading_scope(latest, cx);
        }
        let panel = self.reading.panel.as_mut().expect("reading panel");
        panel.commands_open = commands && panel.scope.is_none();
        panel.input.update(cx, |input, cx| input.focus(window, cx));
        cx.emit(TerminalViewEvent::FocusRequested);
        cx.notify();
    }
    pub(super) fn close_reading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reading.cancel_query();
        self.reading.cancel_copy();
        self.reading.panel = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }
    pub(super) fn reading_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if keymap::is_native_window_shortcut(&event.keystroke)
            || self.reading.panel.as_ref().is_some_and(|panel| {
                panel.input.update(cx, |input, cx| {
                    gpui::EntityInputHandler::marked_text_range(input, window, cx).is_some()
                })
            })
        {
            return;
        }
        let mods = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            // 交给组件 Root 的焦点遍历，祖先终端另有实际焦点守卫。
            "tab" => return,
            "escape" => self.close_reading(window, cx),
            "f3" => self.navigate_reading_match(mods.shift, cx),
            "f" if (mods.control || mods.platform) && !mods.alt && !mods.shift => {
                if let Some(panel) = &self.reading.panel {
                    panel.input.update(cx, |input, cx| input.focus(window, cx));
                    let input = panel.input.clone();
                    cx.on_next_frame(window, move |_, window, cx| {
                        if input.read(cx).focus_handle(cx).is_focused(window) {
                            window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
                        }
                    });
                }
            },
            _ => return,
        }
        // 普通字符须继续交给 GPUI 的文本输入派发；终端自己的焦点守卫隔离 PTY。
        // 只消费已处理的搜索快捷键，否则输入框会收到按键却收不到字符。
        cx.stop_propagation();
    }
    pub(super) fn refresh_reading_commands(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.reading.panel.as_mut() else {
            return;
        };
        let Some(session) = &self.session else {
            return;
        };
        panel.commands = session.term.lock().completed_commands().cloned().collect();
        panel.commands.reverse();
        cx.notify();
    }
    pub(super) fn choose_reading_scope(&mut self, command: Option<u64>, cx: &mut Context<Self>) {
        self.reading.cancel_copy();
        if let Some(panel) = self.reading.panel.as_mut() {
            panel.scope = command;
            panel.commands_open = false;
            panel.feedback.update(cx, |feedback, cx| feedback.clear(cx));
        }
        self.restart_reading_search(cx);
    }
    pub(super) fn restart_reading_search(&mut self, cx: &mut Context<Self>) {
        let expected = self.reading.cancel_query();
        let Some(panel) = self.reading.panel.as_mut() else {
            return;
        };
        panel.hits.clear();
        panel.selected = None;
        panel.highlight = None;
        panel.source = None;
        panel.notice = None;
        panel.truncated = false;
        panel.running = false;
        let Some(source) = self.session.as_ref().map(|s| s.term.clone()) else {
            return;
        };
        let span = {
            let term = source.lock();
            panel.scope.map_or_else(|| Ok(term.reading_span()), |id| term.command_output_span(id))
        };
        let span = match span {
            Ok(span) => span,
            Err(error) => {
                panel.notice = Some(error.into());
                cx.notify();
                return;
            },
        };
        // 空查询也保存本次输出范围：复制/跳转同样必须拒绝已失效的命令。
        panel.source = Some(span);
        let query = panel.input.read(cx).value().to_string();
        if query.is_empty() {
            cx.notify();
            return;
        }
        if query.len() > MAX_QUERY_BYTES || query.chars().any(char::is_control) {
            panel.notice = Some(ReadingIssue::QueryTooLong);
            cx.notify();
            return;
        }
        panel.running = true;
        let match_case = panel.match_case;
        let generation = self.reading.query_generation.clone();
        let executor = cx.background_executor().clone();
        self.reading.query_task = Some(cx.spawn(async move |view, cx| {
            executor.timer(std::time::Duration::from_millis(100)).await;
            if generation.load(Ordering::Acquire) != expected {
                return;
            }
            let (sender, mut receiver) = mpsc::channel(4);
            let worker_generation = generation.clone();
            let task = executor.spawn(async move {
                worker::search(
                    source,
                    span,
                    query,
                    match_case,
                    worker_generation,
                    expected,
                    sender,
                )
                .await;
            });
            while let Some(update) = receiver.next().await {
                if generation.load(Ordering::Acquire) != expected {
                    break;
                }
                if view
                    .update(cx, |view, cx| {
                        if view.reading.query_generation.load(Ordering::Acquire) != expected {
                            return;
                        }
                        let Some(panel) = view.reading.panel.as_mut() else {
                            return;
                        };
                        if update.issue.is_some() {
                            panel.hits.clear();
                            panel.selected = None;
                            panel.highlight = None;
                        }
                        panel.hits.extend(update.hits);
                        panel.running = !update.done;
                        panel.truncated = update.truncated;
                        panel.notice = update.issue;
                        let reveal_first = panel.selected.is_none()
                            && !panel.hits.is_empty()
                            && view.reading.navigation_task.is_none();
                        view.observe_reading_output(cx);
                        if reveal_first {
                            view.navigate_reading_match(false, cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
            task.await;
        }));
        cx.notify();
    }
    pub(super) fn navigate_reading_match(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let Some(panel) = &self.reading.panel else {
            return;
        };
        if panel.hits.is_empty() {
            return;
        }
        let index = match panel.selected {
            None => {
                if backwards {
                    panel.hits.len() - 1
                } else {
                    0
                }
            },
            Some(index) => {
                if backwards {
                    (index + panel.hits.len() - 1) % panel.hits.len()
                } else {
                    (index + 1) % panel.hits.len()
                }
            },
        };
        let hit = panel.hits[index];
        let Some(source_span) = panel.source else {
            return;
        };
        let scope = panel.scope;
        let span = ReadingSpan { epoch: source_span.epoch, start: hit.start, end: hit.end };
        let query = panel.input.read(cx).value().to_string();
        let match_case = panel.match_case;
        let Some(source) = self.session.as_ref().map(|s| s.term.clone()) else {
            return;
        };
        if span.start < source_span.start
            || span.end > source_span.end
            || scope
                .is_some_and(|id| source.lock().command_output_span(id).ok() != Some(source_span))
        {
            self.invalidate_reading(cx);
            return;
        }
        self.reading.navigation_epoch = self.reading.navigation_epoch.wrapping_add(1);
        let navigation_epoch = self.reading.navigation_epoch;
        let query_epoch = self.reading.query_generation.load(Ordering::Acquire);
        let checked_source = source.clone();
        let check = cx
            .background_executor()
            .spawn(async move { worker::validate_hit(&checked_source, span, &query, match_case) });
        self.reading.navigation_task = Some(cx.spawn(async move |view, cx| {
            let guard = check.await;
            let _ = view.update(cx, |view, cx| {
                if view.reading.navigation_epoch != navigation_epoch
                    || view.reading.query_generation.load(Ordering::Acquire) != query_epoch
                {
                    return;
                }
                // 验证和定位共用一次锁，避免核验后被 PTY 重绘替换。
                let mut term = source.lock();
                let valid = scope
                    .is_none_or(|id| term.command_output_span(id).ok() == Some(source_span))
                    && guard.as_ref().is_some_and(|guard| worker::guard_valid(&term, span, guard));
                if valid {
                    scroll_to_point(&mut term, span.start, span.epoch);
                    if let Some(panel) = view.reading.panel.as_mut() {
                        panel.selected = Some(index);
                        panel.highlight = Some(span);
                        panel.notice = None;
                    }
                }
                drop(term);
                if !valid {
                    view.invalidate_reading(cx);
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn jump_reading_command(&mut self, end: bool, cx: &mut Context<Self>) {
        let Some(panel) = &self.reading.panel else {
            return;
        };
        let (Some(id), Some(expected)) = (panel.scope, panel.source) else {
            return;
        };
        let Some(source) = self.session.as_ref().map(|s| s.term.clone()) else {
            return;
        };
        let mut term = source.lock();
        if term.command_output_span(id).ok() == Some(expected) {
            let point =
                if end && expected.end.column == 0 && expected.end.line > expected.start.line {
                    ReadingPoint { line: expected.end.line - 1, column: 0 }
                } else if end {
                    expected.end
                } else {
                    expected.start
                };
            scroll_to_point(&mut term, point, expected.epoch);
        } else {
            drop(term);
            self.invalidate_reading(cx);
        }
        cx.notify();
    }
    pub(in crate::gpui_shell) fn copy_reading_command(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = &self.reading.panel else {
            return;
        };
        let (Some(command), Some(span)) = (panel.scope, panel.source) else {
            return;
        };
        if panel.feedback.read(cx).is_pending() {
            return;
        }
        let Some(source) = self.session.as_ref().map(|s| s.term.clone()) else {
            return;
        };
        if source.lock().command_output_span(command).ok() != Some(span) {
            self.invalidate_reading(cx);
            return;
        }
        let expected = self.reading.cancel_copy();
        let generation = self.reading.copy_generation.clone();
        let panel = self.reading.panel.as_mut().expect("panel");
        panel.notice = None;
        panel.feedback.update(cx, |feedback, cx| feedback.mark_pending(cx));
        let checked_source = source.clone();
        let job = cx.background_executor().spawn(async move {
            worker::copy_command(checked_source, command, span, generation, expected)
        });
        self.reading.copy_task = Some(cx.spawn(async move |view, cx| {
            let result = job.await;
            let _ = view.update(cx, |view, cx| {
                if view.reading.copy_generation.load(Ordering::Acquire) != expected {
                    return;
                }
                let Some(panel) = view.reading.panel.as_mut() else {
                    return;
                };
                if panel.scope != Some(command) || panel.source != Some(span) {
                    return;
                }
                let term = source.lock();
                let valid = term.command_output_span(command).ok() == Some(span);
                // 最终核验后释放网格锁，系统剪贴板的等待不应阻塞 PTY 输出。
                drop(term);
                match result {
                    Ok(text) if valid => {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        panel.feedback.update(cx, |feedback, cx| feedback.mark_copied(cx));
                        panel.notice = None;
                    },
                    result => {
                        panel.feedback.update(cx, |feedback, cx| feedback.clear(cx));
                        panel.notice = Some(result.err().unwrap_or(ReadingIssue::Changed));
                    },
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn invalidate_reading(&mut self, cx: &mut Context<Self>) {
        self.reading.cancel_query();
        self.reading.cancel_copy();
        if let Some(panel) = self.reading.panel.as_mut() {
            panel.hits.clear();
            panel.selected = None;
            panel.highlight = None;
            panel.source = None;
            panel.running = false;
            panel.notice = Some(ReadingIssue::Changed);
            panel.feedback.update(cx, |feedback, cx| feedback.clear(cx));
        }
        cx.notify();
    }
    pub(super) fn observe_reading_output(&mut self, cx: &mut Context<Self>) {
        self.reconcile_output_hover(cx);
        let Some(panel) = &self.reading.panel else {
            return;
        };
        let Some(session) = &self.session else {
            return;
        };
        let term = session.term.lock();
        // Wakeup is hot: the full command guard is reserved for user actions and commits.
        let stale = panel.source.is_some_and(|span| span.epoch != term.reading_epoch());
        let floor = term.grid().scrolled_out().saturating_add(term.history_size());
        drop(term);
        if stale {
            self.invalidate_reading(cx);
        } else if let Some(panel) = self.reading.panel.as_mut() {
            // 活动屏重绘不推进 epoch，故每次输出即撤销活动屏上的视觉覆盖。
            if panel.highlight.is_some_and(|span| span.end.line >= floor) {
                panel.highlight = None;
            }
        }
    }
}
fn scroll_to_point(
    term: &mut nebula_terminal::Term<session::EventProxy>,
    point: ReadingPoint,
    epoch: u64,
) {
    if term.reading_epoch() != epoch || term.mode().contains(TermMode::ALT_SCREEN) {
        return;
    }
    let bottom = term.grid().scrolled_out().saturating_add(term.history_size());
    let top = point.line.saturating_sub(3).max(term.grid().scrolled_out());
    let offset = bottom.saturating_sub(top).min(term.history_size());
    let previous = term.grid().display_offset();
    term.scroll_display(Scroll::Delta(offset as i32 - previous as i32));
}
