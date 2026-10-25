//! 结果就地复制：命中只读有界命令元数据，目标变化和点击时才执行完整范围 guard。
use super::super::super::reading::OutputTarget;
use super::*;
use crate::gpui_shell::copy_feedback::COPY_FEEDBACK_TTL;
use nebula_terminal::Term;

mod chrome;

type SourceTerm = Term<session::EventProxy>;

#[derive(Clone, Copy)]
pub(super) struct OutputGeometry {
    pub region: Bounds<Pixels>,
    pub button_zone: Bounds<Pixels>,
    pub top: Pixels,
}

fn is_agent_command(command: &str) -> bool {
    // 这里只判断程序名；异常长命令不能把一次鼠标移动变成全文扫描。
    let end = command.char_indices().nth(128).map_or(command.len(), |(byte, _)| byte);
    let command = &command[..end];
    command.split_whitespace().next().is_some_and(|program| {
        let program =
            program.trim_matches(['\'', '"']).rsplit(['/', '\\']).next().unwrap_or(program);
        crate::ai_agents::AgentKind::parse(program).is_some()
    })
}

impl TerminalView {
    fn output_hover_allowed(&self, mode: TermMode) -> bool {
        self.output_visible && self.cursor_window_active
            && self.answer_reader.is_none() && self.reading.panel.is_none()
            && !self.selecting
            && self.scrollbar_drag.is_none() && !suggest::popup_active(&self.suggest)
            && !mode.intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE)
            // 不能用 command_running_disproved 反证绕过已识别 Agent 的产品边界。
            && self.running_program.as_deref().and_then(crate::ai_agents::AgentKind::parse).is_none()
    }
    fn output_viewport(&self, term: &SourceTerm) -> (i64, usize, usize) {
        let floor = term.grid().scrolled_out().saturating_add(term.history_size()) as i64;
        (floor + i64::from(term.viewport_origin_for(self.rows).0), self.cols, self.rows)
    }
    fn retained_output(term: &SourceTerm, target: OutputTarget) -> bool {
        let retained = term.reading_span();
        retained.epoch == target.span.epoch
            && retained.start <= target.span.start
            && target.span.end <= retained.end
            && term
                .completed_commands()
                .any(|command| command.id == target.id && command.output == target.span)
    }
    fn output_geometry(&self, term: &SourceTerm, target: OutputTarget) -> Option<OutputGeometry> {
        if self.rows == 0 || self.cols == 0 || target.span.start >= target.span.end {
            return None;
        }
        let (viewport_top, _, _) = self.output_viewport(term);
        let start = target.span.start.line as i64;
        let end = target.span.end.line as i64 + i64::from(target.span.end.column != 0);
        let vertical = self.native_results.projection.borrow();
        let first = vertical.row((start - viewport_top) as f32).max(0.0);
        let last = vertical.row((end - viewport_top) as f32).min(self.rows as f32);
        if first >= last {
            return None;
        }
        let offset = self.line_height * first as f32;
        Some(OutputGeometry {
            region: Bounds::new(
                point(self.origin.x, self.origin.y + offset),
                gpui::size(
                    self.cell_width * self.cols as f32,
                    self.line_height * (last - first) as f32,
                ),
            ),
            button_zone: Bounds::new(
                point(
                    self.origin.x + (self.cell_width * self.cols as f32 - px(180.0)).max(px(0.0)),
                    self.origin.y + offset,
                ),
                gpui::size((self.cell_width * self.cols as f32).min(px(180.0)), px(32.0)),
            ),
            // 此偏移相对终端 root 的8px内边距，不参与网格或PTY尺寸计算。
            top: px(8.0) + offset,
        })
    }
    pub(in crate::gpui_shell::terminal) fn clear_output_hover(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.reading.output_hover;
        if state.hovered.is_none() && state.feedback_target.is_none() {
            return;
        }
        state.clear_targets();
        if let Some(feedback) = &state.feedback {
            feedback.update(cx, |feedback, cx| feedback.clear(cx));
        }
        cx.notify();
    }
    pub(in crate::gpui_shell::terminal) fn reconcile_output_hover(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.reading.output_hover.hovered.is_none()
            && self.reading.output_hover.feedback_target.is_none()
        {
            return;
        }
        let Some(source) = self.session.as_ref().map(|s| s.term.clone()) else {
            self.clear_output_hover(cx);
            return;
        };
        let term = source.lock();
        if !self.output_hover_allowed(*term.mode()) {
            drop(term);
            self.clear_output_hover(cx);
            return;
        }
        let viewport = self.output_viewport(&term);
        let hovered = self.reading.output_hover.hovered.filter(|target| {
            self.reading.output_hover.viewport == Some(viewport)
                && Self::retained_output(&term, *target)
                && self.output_geometry(&term, *target).is_some()
        });
        let feedback_target = self.reading.output_hover.feedback_target.filter(|target| {
            Self::retained_output(&term, *target) && self.output_geometry(&term, *target).is_some()
        });
        drop(term);
        if hovered != self.reading.output_hover.hovered {
            self.reading.output_hover.hovered = hovered;
            cx.notify();
        }
        if feedback_target != self.reading.output_hover.feedback_target {
            self.reading.output_hover.cancel_copy();
            self.reading.output_hover.feedback_target = None;
            self.reading.output_hover.notice = None;
            if let Some(feedback) = &self.reading.output_hover.feedback {
                feedback.update(cx, |feedback, cx| feedback.clear(cx));
            }
            cx.notify();
        }
    }
    pub(in crate::gpui_shell::terminal) fn update_output_hover(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button.is_some() {
            self.leave_output_hover(None, cx);
            return;
        }
        let Some(source) = self.session.as_ref().map(|s| s.term.clone()) else {
            return;
        };
        let term = source.lock();
        if !self.output_hover_allowed(*term.mode()) {
            drop(term);
            self.clear_output_hover(cx);
            return;
        }
        let grid = Bounds::new(
            self.origin,
            gpui::size(self.cell_width * self.cols as f32, self.line_height * self.rows as f32),
        );
        if !grid.contains(&event.position) {
            drop(term);
            self.leave_output_hover(None, cx);
            return;
        }
        let viewport = self.output_viewport(&term);
        drop(term);
        // grid_point 使用当前数学投影的 source_point，不能拿视觉列号猜输出边界。
        let (source_point, _) = self.grid_point(event.position);
        let term = source.lock();
        if viewport != self.output_viewport(&term) {
            return;
        }
        let floor = term.grid().scrolled_out().saturating_add(term.history_size()) as i64;
        let line = floor + i64::from(source_point.line.0);
        if line < 0 {
            return;
        }
        let point = ReadingPoint { line: line as usize, column: source_point.column.0 };
        let previous = self.reading.output_hover.hovered;
        let mut target = term
            .completed_commands()
            .find(|command| command.output.start <= point && point < command.output.end)
            .filter(|command| {
                previous.is_some_and(|target| target.id == command.id)
                    || !is_agent_command(&command.command)
            })
            .map(|command| OutputTarget { id: command.id, span: command.output });
        if target.is_none() {
            // 结果正文通往右侧按钮的同一行留白也属于这次手势，不能跨过去就消失。
            target = previous.filter(|target| {
                Self::retained_output(&term, *target)
                    && self
                        .output_geometry(&term, *target)
                        .is_some_and(|g| g.region.contains(&event.position))
            });
        }
        if target != previous {
            target = target
                .filter(|target| term.command_output_span(target.id).ok() == Some(target.span));
        }
        drop(term);
        self.reading.output_hover.viewport = Some(viewport);
        if self.reading.output_hover.hovered != target {
            self.reading.output_hover.hovered = target;
            cx.notify();
        }
    }
    pub(in crate::gpui_shell::terminal) fn leave_output_hover(
        &mut self,
        pointer: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let target = self.reading.output_hover.hovered;
        let keep = pointer.zip(target).is_some_and(|(pointer, target)| {
            self.session.as_ref().is_some_and(|s| {
                self.output_geometry(&s.term.lock(), target).is_some_and(|geometry| {
                    geometry.region.contains(&pointer) || geometry.button_zone.contains(&pointer)
                })
            })
        });
        if !keep && self.reading.output_hover.hovered.take().is_some() {
            cx.notify();
        }
    }
    fn observe_output_copy_feedback(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.reading.output_hover;
        if state.notice.is_none()
            && state.feedback.as_ref().is_some_and(|feedback| {
                let feedback = feedback.read(cx);
                !feedback.is_pending() && !feedback.is_copied()
            })
        {
            state.feedback_target = None;
        }
        cx.notify();
    }
    pub(in crate::gpui_shell::terminal) fn copy_hovered_output(
        &mut self,
        target: OutputTarget,
        cx: &mut Context<Self>,
    ) {
        // 点击携带渲染时的ID；排队期间指针换到别的结果不能改变这一次的复制目标。
        let state = &self.reading.output_hover;
        if state.hovered != Some(target) && state.feedback_target != Some(target) {
            return;
        }
        let Some(source) = self.session.as_ref().map(|s| s.term.clone()) else {
            return;
        };
        let term = source.lock();
        if !self.output_hover_allowed(*term.mode())
            || self.output_geometry(&term, target).is_none()
            || term.command_output_span(target.id).ok() != Some(target.span)
        {
            drop(term);
            self.clear_output_hover(cx);
            return;
        }
        drop(term);
        let expected = self.reading.output_hover.cancel_copy();
        if self.reading.output_hover.feedback.is_none() {
            let feedback = cx.new(|_| CopyFeedback::new());
            let subscription =
                cx.observe(&feedback, |view, _, cx| view.observe_output_copy_feedback(cx));
            self.reading.output_hover.feedback = Some(feedback);
            self.reading.output_hover.feedback_subscription = Some(subscription);
        }
        let state = &mut self.reading.output_hover;
        state.feedback_target = Some(target);
        state.notice = None;
        state
            .feedback
            .as_ref()
            .expect("feedback")
            .update(cx, |feedback, cx| feedback.mark_pending(cx));
        let generation = state.generation.clone();
        let checked_source = source.clone();
        let executor = cx.background_executor().clone();
        let job = executor.spawn(async move {
            worker::copy_command(checked_source, target.id, target.span, generation, expected)
        });
        state.task = Some(cx.spawn(async move |view, cx| {
            let result = job.await;
            let error_shown = view
                .update(cx, |view, cx| {
                    if view.reading.output_hover.generation.load(Ordering::Acquire) != expected
                        || view.reading.output_hover.feedback_target != Some(target)
                    {
                        return false;
                    }
                    let term = source.lock();
                    let valid = view.output_hover_allowed(*term.mode())
                        && view.output_geometry(&term, target).is_some()
                        && term.command_output_span(target.id).ok() == Some(target.span);
                    if !valid {
                        drop(term);
                        view.clear_output_hover(cx);
                        return false;
                    }
                    // 原生剪贴板可能等待系统锁；最终范围检查后立即释放PTY网格锁。
                    drop(term);
                    let state = &mut view.reading.output_hover;
                    let feedback = state.feedback.as_ref().expect("feedback");
                    let failed = match result {
                        Ok(text) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            feedback.update(cx, |feedback, cx| feedback.mark_copied(cx));
                            false
                        },
                        Err(issue) => {
                            state.notice = Some(issue);
                            feedback.update(cx, |feedback, cx| feedback.clear(cx));
                            log::warn!(
                                "terminal output copy rejected for command {}: {issue:?}",
                                target.id
                            );
                            true
                        },
                    };
                    cx.notify();
                    failed
                })
                .unwrap_or(false);
            if error_shown {
                // 失败没有假成功对勾；只保留一次有界的就地错误说明。
                executor.timer(COPY_FEEDBACK_TTL).await;
                let _ = view.update(cx, |view, cx| {
                    let state = &mut view.reading.output_hover;
                    if state.generation.load(Ordering::Acquire) == expected {
                        state.feedback_target = None;
                        state.notice = None;
                        cx.notify();
                    }
                });
            }
        }));
        cx.notify();
    }
}
