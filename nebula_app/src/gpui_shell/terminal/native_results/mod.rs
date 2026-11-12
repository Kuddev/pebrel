//! 原生结果视图。网格只给出锚点；主题、控件、解析和交互由应用拥有。
use crate::native_result_model as model;

use super::view::TerminalView;
use crate::gpui_shell::copy_feedback::CopyFeedback;
use crate::gpui_shell::prelude::*;
use crate::i18n::Message;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px, relative,
    rems,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::text::{
    MarkdownExtensions, MarkdownNode, TextView, TextViewState, TextViewStyle,
};
use nebula_terminal::grid::{Dimensions as _, GridCell as _};
use nebula_terminal::index::{Column, Point};
use nebula_terminal::inline_content::{Status, Surface};
use nebula_terminal::term::TermMode;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

mod code;
mod diff;
mod fold;
mod json;
mod markdown;
mod table;
mod tools;
mod viewport;
pub(super) use viewport::Views;

impl TerminalView {
    pub(super) fn native_result_elements(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Vec<AnyElement> {
        let Some(session) = &self.session else { return Vec::new() };
        self.native_results.reconcile_retention(cx);
        self.native_results.sync(&mut session.term.lock(), self.grid_rows(), cx);
        let placements = self.native_results.placements();
        let available_width = self.native_result_width();
        let parent = cx.entity().downgrade();
        placements
            .into_iter()
            .map(|(surface, row, rows)| {
                if fold::is_fold(&surface) {
                    return fold::element(self, &surface, row, window, cx);
                }
                let entity = self
                    .native_results
                    .entities
                    .entry(surface.id)
                    .or_insert_with(|| {
                        cx.new(|cx| {
                            NativeResult::new(
                                &surface,
                                self.focus_handle.clone(),
                                parent.clone(),
                                window,
                                cx,
                            )
                        })
                    })
                    .clone();
                entity.update(cx, |view, cx| {
                    let top = (-row * self.line_height.as_f32()).max(0.0);
                    if view.row_height != self.line_height
                        || view.font_size != self.font_size
                        || view.available_width != available_width
                        || (view.visible_top - top).abs() > 0.5
                    {
                        cx.notify();
                    }
                    view.row_height = self.line_height;
                    view.font_size = self.font_size;
                    view.font_family = self.font.family.clone();
                    view.available_width = available_width;
                    view.viewport_top = 8.0 + row * self.line_height.as_f32();
                    view.visible_top = top;
                    view.visible_height = self.grid_rows() as f32 * self.line_height.as_f32();
                    view.sync(cx);
                });
                div()
                    .absolute()
                    .left(px(12.0))
                    .w(self.native_result_width())
                    .top(px(8.0) + self.line_height * row)
                    .min_h(self.line_height * rows)
                    .child(entity)
                    .into_any_element()
            })
            .collect()
    }
}

pub(super) struct NativeResult {
    source: Weak<Surface>,
    mime: Arc<str>,
    row_height: Pixels,
    font_size: Pixels,
    font_family: gpui::SharedString,
    measured_height: Pixels,
    visible_top: f32,
    visible_height: f32,
    available_width: Pixels,
    viewport_top: f32,
    parent: gpui::WeakEntity<TerminalView>,
    hovered: bool,
    toolbar_hovered: bool,
    revision: u64,
    chunks: usize,
    raw: model::RawText,
    stderr: model::RawText,
    raw_stderr: bool,
    status: Status,
    raw_mode: bool,
    markdown: Entity<TextViewState>,
    extensions: MarkdownExtensions,
    code_blocks: HashMap<usize, Entity<code::Code>>,
    diff: Option<Arc<model::diff::Patch>>,
    collapsed_files: HashSet<usize>,
    json: json::JsonView,
    table: Option<Arc<model::Table>>,
    table_layout: table::Layout,
    table_scroll: gpui::ScrollHandle,
    order: Vec<usize>,
    sort: Option<(usize, bool)>,
    filter: Entity<InputState>,
    filter_open: bool,
    query_epoch: u64,
    parsing: bool,
    notice: Option<Message>,
    git: model::GitProgress,
    parent_focus: FocusHandle,
    focus: FocusHandle,
    feedback: Entity<CopyFeedback>,
    task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl NativeResult {
    #[cfg(test)]
    pub(super) fn height(&self) -> Pixels {
        self.measured_height
    }
    #[cfg(test)]
    pub(super) fn body_font_size(&self) -> Pixels {
        self.font_size
    }
    #[cfg(test)]
    pub(super) fn displayed_rows(&self) -> Vec<Vec<String>> {
        self.table.as_ref().map_or_else(Vec::new, |table| {
            self.order.iter().map(|&row| table.rows[row].clone()).collect()
        })
    }
    #[cfg(test)]
    pub(super) fn code_identities(&self) -> Vec<gpui::EntityId> {
        self.code_blocks.values().map(Entity::entity_id).collect()
    }
    fn new(
        source: &Arc<Surface>,
        parent_focus: FocusHandle,
        parent: gpui::WeakEntity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let language = crate::gpui_shell::config::ui_language(cx);
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(Message::NativeResultFilter))
        });
        let feedback = cx.new(|_| CopyFeedback::new());
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.observe(&feedback, |_, _, cx| cx.notify()),
            cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.order_table(cx);
                }
            }),
        ];
        // 扩展实例必须稳定：每帧重建会触发上游重新解析整份 Markdown。
        let extensions = markdown::extensions(cx.entity().downgrade());
        Self {
            source: Arc::downgrade(source),
            mime: source.mime.clone(),
            row_height: px(22.0),
            font_size: px(14.0),
            font_family: "monospace".into(),
            measured_height: px(0.0),
            visible_top: 0.0,
            visible_height: 600.0,
            available_width: px(900.0),
            viewport_top: 0.0,
            parent,
            hovered: false,
            toolbar_hovered: false,
            revision: 0,
            chunks: 0,
            raw: model::RawText::default(),
            stderr: model::RawText::default(),
            raw_stderr: false,
            status: Status::Receiving,
            raw_mode: false,
            markdown: cx.new(|cx| TextViewState::markdown("", cx)),
            extensions,
            code_blocks: HashMap::new(),
            diff: None,
            collapsed_files: HashSet::new(),
            json: json::JsonView::default(),
            table: None,
            table_layout: table::Layout::default(),
            table_scroll: gpui::ScrollHandle::new(),
            order: Vec::new(),
            sort: None,
            filter,
            filter_open: false,
            query_epoch: 0,
            parsing: false,
            notice: None,
            git: model::GitProgress::default(),
            parent_focus,
            focus,
            feedback,
            task: None,
            _subscriptions: subscriptions,
        }
    }

    fn sync(&mut self, cx: &mut Context<'_, Self>) {
        let Some(source) = self.source.upgrade() else { return };
        if source.revision() == self.revision {
            return;
        }
        let update = source.update_since(self.chunks);
        self.revision = update.revision;
        self.chunks = update.chunk_count;
        self.status = update.status;
        if self.status == Status::Evicted {
            self.raw = model::RawText::default();
            self.stderr = model::RawText::default();
            self.table = None;
            self.table_layout = table::Layout::default();
            self.code_blocks.clear();
            self.diff = None;
            self.collapsed_files.clear();
            self.json = json::JsonView::default();
            self.order.clear();
            self.task = None;
            self.query_epoch += 1;
            self.markdown = cx.new(|cx| TextViewState::markdown("", cx));
            self.notice = Some(Message::NativeResultEvicted);
            cx.notify();
            return;
        }
        for chunk in update.chunks {
            if chunk.stderr {
                if !self.raw_mode && self.stderr.text.is_empty() {
                    self.raw_stderr = true;
                }
                self.stderr.append(&chunk.text);
            } else {
                self.raw.append(&chunk.text);
            }
            if self.mime.as_ref() == "text/markdown" {
                self.markdown.update(cx, |text, cx| text.push_str(&chunk.text, cx));
            } else if self.mime.as_ref() == "text/x-git-progress" {
                self.git.feed(&chunk.text, chunk.stderr);
            }
        }
        if self.mime.as_ref() == "text/x-git-output" {
            // 使用长于正文反引号序列的围栏，文件名/日志内容不会改变文档结构。
            let fence = "`".repeat(
                self.raw.text.split(|c| c != '`').map(str::len).max().unwrap_or(0).max(2) + 1,
            );
            let source = format!("{fence}text\n{}\n{fence}\n", self.raw.text);
            self.markdown = cx.new(|cx| TextViewState::markdown(&source, cx));
        }
        if self.status != Status::Receiving {
            self.git.finish();
        }
        if self.status == Status::Limited {
            self.notice = Some(Message::NativeResultLimit);
        }
        if self.mime.as_ref() == "text/x-diff" && self.status != Status::Receiving {
            self.parse_diff(source.has_grid_source(), cx);
        }
        if self.mime.as_ref() == "application/json" && self.status != Status::Receiving {
            self.parse_json(cx);
        }
        if matches!(self.mime.as_ref(), "text/x-directory-listing" | "text/x-powershell-directory")
            && self.status != Status::Receiving
            && self.table.is_none()
            && !self.parsing
        {
            self.parsing = true;
            let raw = self.raw.text.clone();
            let mime = self.mime.clone();
            self.task = Some(cx.spawn(async move |this, cx| {
                let table = cx
                    .background_spawn(async move {
                        if mime.as_ref() == "application/json" {
                            model::Table::parse(&raw)
                        } else {
                            model::Table::parse_directory(
                                &raw,
                                mime.as_ref() == "text/x-powershell-directory",
                            )
                        }
                        .map(|table| {
                            let layout = table::Layout::prepare(&table);
                            (Arc::new(table), layout)
                        })
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.parsing = false;
                    match table {
                        Ok((table, layout)) => {
                            this.table = Some(table);
                            this.table_layout = layout;
                            this.order_table(cx);
                        },
                        Err(error) => {
                            this.notice = Some(if error == "limit" {
                                Message::NativeResultTableLimit
                            } else {
                                Message::NativeResultTableInvalid
                            });
                            this.raw_mode = true;
                        },
                    }
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    fn order_table(&mut self, cx: &mut Context<'_, Self>) {
        if self.mime.as_ref() == "application/json" {
            self.rebuild_json(cx);
            return;
        }
        let Some(table) = self.table.clone() else { return };
        self.query_epoch += 1;
        let epoch = self.query_epoch;
        let query = self.filter.read(cx).value().to_string();
        let sort = self.sort;
        self.task = Some(cx.spawn(async move |this, cx| {
            let order = cx.background_spawn(async move { table.order(&query, sort) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.query_epoch == epoch {
                    this.order = order;
                    cx.notify();
                }
            });
        }));
    }

    fn raw_element(&self, _: &mut Context<'_, Self>) -> AnyElement {
        let raw = self.selected_raw();
        let first = (self.visible_top / self.row_height.as_f32()).floor() as usize;
        let last = ((self.visible_top + self.visible_height) / self.row_height.as_f32()).ceil()
            as usize
            + 2;
        div()
            .debug_selector(|| "native-result-raw-content".into())
            .relative()
            .w_full()
            .h(self.row_height * raw.lines.len() as f32)
            .children((first.saturating_sub(2)..last.min(raw.lines.len())).map(|index| {
                div()
                    .absolute()
                    .top(self.row_height * index as f32)
                    .h(self.row_height)
                    .px_2()
                    .font_family(self.font_family.clone())
                    .whitespace_nowrap()
                    .child(raw.text[raw.lines[index].clone()].to_owned())
            }))
            .into_any_element()
    }

    fn selected_raw(&self) -> &model::RawText {
        if self.raw_stderr { &self.stderr } else { &self.raw }
    }

    fn copy_source(&mut self, cx: &mut Context<'_, Self>) {
        if self.status == Status::Evicted {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.selected_raw().text.clone()));
        self.feedback.update(cx, |feedback, cx| feedback.mark_copied(cx));
    }

    fn git_element(&self, cx: &Context<'_, Self>) -> AnyElement {
        let mut body = div()
            .id("native-git-phases")
            .w_full()
            .max_w(px(834.0))
            .pl(px(18.0))
            .pr(px(16.0))
            .flex()
            .flex_col();
        for (index, phase) in self.git.phases.iter().enumerate() {
            let complete = phase.percent == Some(100)
                || phase.detail.ends_with("done.")
                || index + 1 < self.git.phases.len()
                || self.status == Status::Finished(0);
            let detail = phase
                .percent
                .and_then(|_| phase.detail.split_once('%'))
                .map_or(phase.detail.as_str(), |(_, detail)| detail.trim());
            let item = div()
                .id(("native-git-phase", index))
                .debug_selector(|| "native-git-phase".into())
                .flex()
                .items_center()
                .gap(px(12.0))
                .h(px(29.0))
                .flex_shrink_0()
                .min_w_0()
                .rounded(px(5.0))
                .hover(|row| row.bg(cx.theme().foreground.opacity(0.035)))
                .child(
                    div().w(px(18.0)).flex_shrink_0().child(
                        Icon::new(if complete { IconName::Check } else { IconName::ArrowRight })
                            .size(px(14.0))
                            .text_color(if complete {
                                cx.theme().success
                            } else {
                                cx.theme().primary
                            }),
                    ),
                )
                .child(
                    div()
                        .w(relative(0.28))
                        .min_w(px(100.0))
                        .flex_shrink_0()
                        .text_ellipsis()
                        .text_color(if complete {
                            cx.theme().muted_foreground
                        } else {
                            cx.theme().foreground
                        })
                        .child(phase.name.clone()),
                )
                .child(
                    div()
                        .w(relative(0.20))
                        .max_w(px(180.0))
                        .h(px(3.0))
                        .rounded_sm()
                        .bg(cx.theme().foreground.opacity(0.09))
                        .when_some(phase.percent, |bar, percent| {
                            bar.child(
                                div()
                                    .h_full()
                                    .w(relative(f32::from(percent) / 100.0))
                                    .rounded_sm()
                                    .bg(cx.theme().primary.opacity(if percent == 100 {
                                        0.65
                                    } else {
                                        1.0
                                    })),
                            )
                        }),
                )
                .child(
                    div()
                        .w(px(44.0))
                        .text_size(px(12.0))
                        .flex_shrink_0()
                        .text_right()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            phase
                                .percent
                                .map_or_else(|| "—".into(), |percent| format!("{percent}%")),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.0))
                        .text_ellipsis()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail.to_owned()),
                );
            body = body.child(item);
        }
        for line in self.git.tail.iter().map(String::as_str).chain(self.git.pending_lines()) {
            body = body.child(
                div()
                    .h(self.row_height)
                    .flex_shrink_0()
                    .text_ellipsis()
                    .text_color(if line.starts_with("fatal:") || line.starts_with("error:") {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(line.to_owned()),
            );
        }
        body.into_any_element()
    }
}

impl Render for NativeResult {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let t = |id| language.text(id);
        let status = match self.status {
            Status::Receiving => Message::NativeResultReceiving,
            Status::Finished(0) => Message::NativeResultComplete,
            Status::Finished(_) => Message::NativeResultFailed,
            Status::Interrupted => Message::NativeResultInterrupted,
            Status::Limited => Message::NativeResultLimit,
            Status::Evicted => Message::NativeResultEvicted,
        };
        let copied = self.feedback.read(cx).is_copied();
        // 鼠标点击留下的焦点不应把工具栏钉住；键盘进入和复制反馈仍可发现操作。
        let tools_visible = self.hovered
            || self.toolbar_hovered
            || copied
            || (self.focus.contains_focused(window, cx) && window.last_input_was_keyboard());
        let show_status = self.mime.as_ref() == "text/x-git-progress"
            || !matches!(self.status, Status::Receiving | Status::Finished(0));
        let body = if self.raw_mode {
            self.raw_element(cx)
        } else {
            match self.mime.as_ref() {
                "text/x-diff" => self.diff_element(window, cx),
                "text/markdown" | "text/x-git-output" => {
                    // 输出中的远程图片/HTML不能因预览而悄悄发起请求或读取本地文件。
                    div()
                        .w_full()
                        .pl(px(18.0))
                        .pr(px(16.0))
                        .line_height(self.font_size * 1.65)
                        .child(
                            TextView::new(&self.markdown)
                                .selectable(true)
                                .scrollable(false)
                                .markdown_extensions(self.extensions.clone())
                                .style(TextViewStyle {
                                    heading_base_font_size: self.font_size,
                                    // 终端 surface 的标题层级保持紧凑，不沿用文档页默认的两倍字号。
                                    heading_font_size: Some(Arc::new(|level, base| {
                                        base * match level {
                                            1 => 1.3,
                                            2 => 1.05,
                                            _ => 1.0,
                                        }
                                    })),
                                    paragraph_gap: rems(10.0 / window.rem_size().as_f32()),
                                    inline_code_radius: px(4.0),
                                    inline_code: gpui::HighlightStyle {
                                        background_color: Some(
                                            cx.theme().foreground.opacity(0.035),
                                        ),
                                        ..Default::default()
                                    },
                                    highlight_theme: cx.theme().highlight_theme.clone(),
                                    is_dark: cx.theme().is_dark(),
                                    ..Default::default()
                                }),
                        )
                        .into_any_element()
                },
                "application/json" => self.json_element(window, cx),
                "text/x-directory-listing" | "text/x-powershell-directory" => {
                    self.table_element(window, cx)
                },
                "text/x-git-progress" => self.git_element(cx),
                _ => self.raw_element(cx),
            }
        };
        let tools = tools_visible.then(|| self.toolbar_element(window, cx));
        let content_count =
            1 + usize::from(self.notice.is_some()) + usize::from(show_status && !self.raw_mode);
        let measured = cx.entity().downgrade();
        let mut root = div().on_children_prepainted(move |bounds, _, cx| {
            let height = bounds.iter().take(content_count).fold(0.0_f32, |height, bounds| height + bounds.size.height.as_f32());
            let _ = measured.update(cx, |this, cx| {
                if (this.measured_height.as_f32() - height).abs() > 0.5 {
                    this.measured_height = px(height);
                    let _ = this.parent.update(cx, |_, cx| cx.notify());
                }
            });
        })
            .id("native-result")
            .debug_selector(|| "native-result".into())
            .occlude()
            .relative()
            .w_full()
            .max_w(px(900.0))
            .min_w_0()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_size(self.font_size)
            .font_family(self.font_family.clone())
            .line_height(self.row_height)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus)
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if this.hovered != *hovered {
                    this.hovered = *hovered;
                    cx.notify();
                }
            }))
            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
                // 原生区域会遮挡网格命中；明确交回终端的同一个滚动入口。
                let vertical = match &event.delta {
                    gpui::ScrollDelta::Pixels(delta) => delta.y.as_f32() != 0.0,
                    gpui::ScrollDelta::Lines(delta) => delta.y != 0.0,
                };
                if vertical {
                    let _ = this.parent.update(cx, |view, cx| view.on_scroll(event, window, cx));
                }
                cx.stop_propagation();
            }))
            .key_context("PebrelNativeResult")
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if super::keymap::is_native_window_shortcut(&event.keystroke) {
                    return;
                }
                if this.focus.is_focused(window) && event.keystroke.key == "c"
                    && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                {
                    let selected = window.selected_text(cx);
                    if selected.is_empty() {
                        this.copy_source(cx);
                    } else {
                        cx.write_to_clipboard(ClipboardItem::new_string(selected));
                    }
                }
                if event.keystroke.key == "escape" {
                    if this.filter_open {
                        this.filter_open = false;
                        this.filter.update(cx, |input, cx| input.set_value("", window, cx));
                        this.order_table(cx);
                        window.focus(&this.focus, cx);
                    } else {
                        window.focus(&this.parent_focus, cx);
                    }
                    cx.notify();
                }
                // 子输入框需要平台继续生成字符/IME 消息；终端自身已按焦点隔离 PTY 输入。
                if event.keystroke.key == "escape" || (this.focus.is_focused(window) && event.keystroke.key != "tab") {
                    cx.stop_propagation();
                }
            }))
            .when_some(self.notice, |root, notice| {
                root.child(div().px_2().text_color(cx.theme().warning).child(t(notice)))
            })
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    // 上下留白使用同一尺寸；实际内容高度交给投影，不再补齐到终端整行。
                    .when(
                        !self.raw_mode
                            && matches!(
                                self.mime.as_ref(),
                                "text/markdown"
                                    | "application/json"
                                    | "text/x-directory-listing"
                                    | "text/x-powershell-directory"
                                    | "text/x-diff"
                                    | "text/x-git-output"
                            ),
                        |body| body.py(px(6.0)),
                    )
                    .child(body),
            )
            .when(show_status && !self.raw_mode, |root| {
                root.child(
                    div()
                        .pl(px(18.0))
                        .pr(px(16.0))
                        .h(self.row_height)
                        .when(!self.git.phases.is_empty(), |status| status.h(self.row_height + px(11.0)).pt(px(11.0)))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .text_color(
                            if matches!(self.status, Status::Finished(code) if code != 0) {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            },
                        )
                        .child(match self.status {
                            Status::Finished(code) if code != 0 => {
                                format!("{} · {code}", t(status))
                            },
                            _ => t(status).to_string(),
                        }),
                )
            })
            // 细线固定表达结果归属，悬停只改变语义色。
            .child(
                div()
                    .debug_selector(|| "native-result-line".into())
                    .absolute()
                    .left_0()
                    .top(px(5.0))
                    .bottom(px(5.0))
                    .w(px(2.0))
                    .rounded(px(1.0))
                    .bg(cx.theme().foreground.opacity(0.09)),
            )
            .when(self.hovered, |root| root.child(
                div().debug_selector(|| "native-result-hover-line".into())
                    .absolute().left_0().top(px(5.0)).bottom(px(5.0)).w(px(2.0)).rounded(px(1.0))
                    .bg(cx.theme().primary)
            ))
            // 浮层不占正文行，悬停前后内容位置保持不变。
            .when_some(tools, |root, tools| root.child(tools));
        // occlude 已隔离下方终端命中；左键/移动必须继续冒泡给窗口级文本选区。
        root = root.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                window.focus(&this.focus, cx);
            }),
        );
        for button in [MouseButton::Right, MouseButton::Middle] {
            root = root
                .on_mouse_down(button, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(button, |_, _, cx| cx.stop_propagation());
        }
        root
    }
}

fn field_preview(value: &str) -> String {
    // 保留原文供复制，表格只给字体系统发送有界的单行预览。
    let mut end = value.len().min(512);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut text: String =
        value[..end].chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if end < value.len() {
        text.push('…');
    }
    text
}
