//! JSON 的折叠状态与可选择文本属于结果实体，原文切换不重建它们。

use super::*;
use model::json::{Document, Kind, Row};

#[derive(Default)]
pub(super) struct JsonView {
    pub document: Option<Arc<Document>>,
    expanded: HashSet<usize>,
    rows: Vec<Row>,
    text: HashMap<usize, Entity<TextViewState>>,
    language: Option<crate::display::UiLanguage>,
    font: Option<(gpui::SharedString, Pixels)>,
    natural_width: Pixels,
}

impl NativeResult {
    pub(super) fn parse_json(&mut self, cx: &mut Context<Self>) {
        if self.json.document.is_some() || self.parsing || self.notice.is_some() {
            return;
        }
        self.parsing = true;
        let source = self.raw.text.clone();
        let revision = self.revision;
        self.task = Some(cx.spawn(async move |this, cx| {
            let document = cx.background_spawn(async move { Document::parse(&source) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.revision != revision || this.status == Status::Evicted {
                    return;
                }
                this.parsing = false;
                match document {
                    Ok(document) => {
                        this.json.expanded = document.initially_expanded();
                        this.json.document = Some(Arc::new(document));
                        this.rebuild_json(cx);
                    },
                    Err(error) => {
                        this.notice = Some(if error == "limit" {
                            Message::NativeResultTableLimit
                        } else {
                            Message::NativeResultJsonInvalid
                        });
                        this.raw_mode = true;
                    },
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn rebuild_json(&mut self, cx: &mut Context<Self>) {
        let Some(document) = &self.json.document else { return };
        let language = crate::gpui_shell::config::ui_language(cx);
        self.json.rows =
            document.rows(&self.json.expanded, &self.filter.read(cx).value(), |count| {
                language.format(Message::NativeResultJsonItems, &[("count", &count.to_string())])
            });
        self.json.text.clear();
        self.json.natural_width = px(0.0);
        self.json.language = Some(language);
        cx.notify();
    }

    pub(super) fn json_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        if self.json.document.is_none() {
            return div()
                .px_2()
                .child(language.text(Message::NativeResultPreparing))
                .into_any_element();
        }
        if self.json.language != Some(language) {
            self.rebuild_json(cx);
        }
        let font_key = (self.font_family.clone(), self.font_size);
        if self.json.font.as_ref() != Some(&font_key) {
            self.json.text.clear();
            self.json.natural_width = px(0.0);
            self.json.font = Some(font_key);
        }
        let row_height = (self.font_size * 1.6).max(px(22.0));
        let indent = self.font_size * 1.2;
        let prefix = if self.filter_open { 38.0 } else { 0.0 };
        let first = (((self.visible_top - prefix).max(0.0) / row_height.as_f32()) as usize)
            .min(self.json.rows.len());
        let last =
            ((self.visible_top + self.visible_height) / row_height.as_f32()).ceil() as usize + 2;
        let visible = first.saturating_sub(2)..last.min(self.json.rows.len());
        let foreground = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;
        let guide = foreground.opacity(0.10);
        let mut rows = Vec::new();
        for index in visible {
            let row = &self.json.rows[index];
            let id = row.node * 2 + usize::from(row.closing);
            let left = indent * row.depth as f32;
            let text = if let Some(text) = self.json.text.get(&id) {
                text.clone()
            } else {
                let run = gpui::TextRun {
                    len: row.text.len(),
                    font: crate::font_install::gpui_font_with_fallbacks(&self.font_family),
                    color: foreground,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let measured = window.text_system().shape_line(
                    row.text.clone().into(),
                    self.font_size,
                    &[run],
                    None,
                );
                self.json.natural_width =
                    self.json.natural_width.max(left + px(32.0) + measured.width);
                let escaped =
                    row.text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                // pre 保留字符串中的连续空格；转义后不把 JSON 当成 HTML 或远程资源。
                let html = format!("<pre>{escaped}</pre>");
                let text = cx.new(|cx| TextViewState::html(&html, cx));
                self.json.text.insert(id, text.clone());
                text
            };
            let node = row.node;
            let expanded = row.expanded;
            let mut line = div()
                .id(("native-json-row", id))
                .debug_selector(move || format!("native-json-row-{id}"))
                .absolute()
                .top(row_height * index as f32)
                .left_0()
                .right_0()
                .h(row_height)
                .flex()
                .items_center()
                .pl(left)
                .whitespace_nowrap()
                .hover(|row| row.bg(foreground.opacity(0.035)));
            for level in 0..row.depth {
                line = line.child(
                    div()
                        .absolute()
                        .left(indent * level as f32 + px(9.0))
                        .top_0()
                        .bottom_0()
                        .w(px(1.0))
                        .bg(guide),
                );
            }
            line = line
                .child(div().w(px(24.0)).h(row_height).flex_shrink_0().when(
                    row.expandable,
                    |slot| {
                        slot.child(
                            Button::new(("json-toggle", node))
                                .debug_selector(move || format!("native-json-toggle-{node}"))
                                .ghost()
                                .small()
                                .w(px(22.0))
                                .h(row_height)
                                .px_0()
                                .icon(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .tooltip(language.text(if expanded {
                                    Message::NativeResultJsonCollapse
                                } else {
                                    Message::NativeResultJsonExpand
                                }))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if !this.json.expanded.remove(&node) {
                                        this.json.expanded.insert(node);
                                    }
                                    window.focus(&this.focus, cx);
                                    this.rebuild_json(cx);
                                })),
                        )
                    },
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .line_height(row_height)
                        .debug_selector(move || format!("native-json-text-{id}"))
                        .text_color(match row.kind {
                            Kind::String => cx.theme().success,
                            Kind::Number => cx.theme().primary,
                            Kind::Bool | Kind::Null => muted,
                            _ => foreground,
                        })
                        .child(TextView::new(&text).selectable(true).scrollable(false).style(
                            TextViewStyle { paragraph_gap: rems(0.0), ..Default::default() },
                        )),
                );
            rows.push(line);
        }
        // 结果有原文预算；滚动浏览不能无限累积离屏 TextView 的解析与选区实体。
        if self.json.text.len() > 512 {
            let active: HashSet<_> = self.json.rows
                [first.saturating_sub(2)..last.min(self.json.rows.len())]
                .iter()
                .map(|row| row.node * 2 + usize::from(row.closing))
                .collect();
            self.json.text.retain(|id, _| active.contains(id));
        }
        let width = self.json.natural_width.max((self.available_width - px(34.0)).min(px(866.0)));
        div()
            .w_full()
            .min_w_0()
            .pl(px(18.0))
            .pr(px(16.0))
            .when(self.filter_open, |body| {
                body.child(
                    div()
                        .h(px(38.0))
                        .debug_selector(|| "native-result-filter-input".into())
                        .child(Input::new(&self.filter).small()),
                )
            })
            .child(
                div()
                    .id("native-json-horizontal")
                    .w_full()
                    .min_w_0()
                    .overflow_x_scroll()
                    .restrict_scroll_to_axis()
                    .track_scroll(&self.table_scroll)
                    .child(
                        div()
                            .relative()
                            .w(width)
                            .h(row_height * self.json.rows.len() as f32)
                            .children(rows),
                    ),
            )
            .when(self.json.rows.is_empty(), |body| {
                body.child(language.text(Message::NativeResultNoRows))
            })
            .into_any_element()
    }
}
