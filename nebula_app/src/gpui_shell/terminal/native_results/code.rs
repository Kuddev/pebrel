//! 结果中的代码块复用文本选择和语法着色，行号与操作浮层不进入复制内容。
use super::*;
use gpui::{App, Focusable as _, SharedString, TextRun};
use gpui_component::button::ButtonCustomVariant;
use gpui_component::select::{SearchableVec, SelectEvent};

#[derive(Clone)]
pub(super) struct Spec {
    pub source: SharedString,
    pub language: SharedString,
    pub offset: usize,
    pub lines: usize,
    pub diff_rows: Option<Arc<[model::diff::Row]>>,
}

pub(super) struct Code {
    spec: Spec,
    text: Entity<TextViewState>,
    language: Entity<SelectState<SearchableVec<SharedString>>>,
    plain: SharedString,
    feedback: Entity<CopyFeedback>,
    focus: FocusHandle,
    hovered: bool,
    family: SharedString,
    size: Pixels,
    width: Pixels,
    top: Pixels,
    line_tops: Vec<Pixels>,
    content_width: Pixels,
    scroll: gpui::ScrollHandle,
    metrics: Option<(SharedString, Pixels, Pixels, f32)>,
    _subscriptions: Vec<Subscription>,
}

fn fenced(source: &str, language: &str) -> String {
    // 波浪线围栏允许语言名含反引号；重建后正文必须仍是代码，不能重新成为图片/HTML。
    let fence = "~".repeat(source.split(|c| c != '~').map(str::len).max().unwrap_or(0).max(2) + 1);
    let language = if language.chars().any(char::is_control) { "" } else { language };
    format!("{fence}{language}\n{source}\n{fence}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_result_code_fences_keep_unusual_languages_and_markup_literal() {
        let source =
            "![probe](https://example.invalid/probe.png)\n~~~\n<img src='file:///private'>\n```";
        for language in ["x`y", "bash", "text\nmalformed"] {
            let parsed =
                ::markdown::to_mdast(&fenced(source, language), &::markdown::ParseOptions::gfm())
                    .unwrap();
            let children = parsed.children().unwrap();
            assert_eq!(children.len(), 1);
            let gpui_component::text::markdown_ast::Node::Code(code) = &children[0] else {
                panic!("source escaped code fence")
            };
            assert_eq!(code.value, source);
        }
    }
}

impl Code {
    fn new(spec: Spec, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let plain: SharedString =
            crate::gpui_shell::config::ui_language(cx).text(Message::MarkdownPlainText).into();
        let mut languages = gpui_component::highlighter::LanguageRegistry::singleton().languages();
        languages.sort();
        languages.insert(0, plain.clone());
        let current = if spec.language.is_empty() { plain.clone() } else { spec.language.clone() };
        if !languages.contains(&current) {
            languages.push(current.clone());
        }
        let index = languages.iter().position(|name| name == &current).unwrap_or(0);
        let language = cx.new(|cx| {
            SelectState::new(SearchableVec::new(languages), Some(IndexPath::new(index)), window, cx)
                .searchable(true)
        });
        let feedback = cx.new(|_| CopyFeedback::new());
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.observe(&feedback, |_, _, cx| cx.notify()),
            cx.observe(&language, |_, _, cx| cx.notify()),
            cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            cx.subscribe(&language, |this, _, event, cx| {
                if let SelectEvent::Confirm(Some(language)) = event {
                    let lang = if *language == this.plain { "" } else { language.as_ref() };
                    let source = fenced(&this.spec.source, lang);
                    this.text.update(cx, |text, cx| text.set_text(&source, cx));
                    cx.notify();
                }
            }),
        ];
        let text = cx.new(|cx| TextViewState::markdown(&fenced(&spec.source, &spec.language), cx));
        Self {
            spec,
            text,
            language,
            plain,
            feedback,
            focus,
            hovered: false,
            family: "monospace".into(),
            size: px(14.0),
            width: px(600.0),
            top: px(0.0),
            line_tops: Vec::new(),
            content_width: px(0.0),
            scroll: gpui::ScrollHandle::new(),
            metrics: None,
            _subscriptions: subscriptions,
        }
    }
}

pub(super) fn render(
    spec: &Spec,
    owner: &gpui::WeakEntity<NativeResult>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(owner) = owner.upgrade() else { return div().into_any_element() };
    owner.update(cx, |result, cx| result.code_element(spec, window, cx))
}

impl NativeResult {
    pub(super) fn code_element(
        &mut self,
        spec: &Spec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 代码语言选择和解析随结果保留，不能因离开视口一帧就被 keyed state 回收。
        let state = self
            .code_blocks
            .entry(spec.offset)
            .or_insert_with(|| cx.new(|cx| Code::new(spec.clone(), window, cx)))
            .clone();
        let family = self.font_family.clone();
        let size = self.font_size;
        state.update(cx, |code, cx| {
            if code.spec.source != spec.source || code.spec.language != spec.language {
                code.spec = spec.clone();
                code.metrics = None;
                let current = if spec.language.is_empty() {
                    code.plain.clone()
                } else {
                    spec.language.clone()
                };
                code.language
                    .update(cx, |language, cx| language.set_selected_value(&current, window, cx));
                code.text.update(cx, |text, cx| {
                    text.set_text(&fenced(&spec.source, &spec.language), cx)
                });
            }
            if code.family != family || code.size != size {
                code.family = family;
                code.size = size;
                cx.notify();
            }
        });
        state.into_any_element()
    }
}

impl Render for Code {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_diff = self.spec.diff_rows.is_some();
        let line_height = window.pixel_snap(self.size * 1.65);
        let digits = self.spec.lines.max(1).ilog10() + 1;
        let column = px(19.0).max(px(digits as f32 * 7.0)) + px(14.0);
        let gutter = column * if is_diff { 2.0 } else { 1.0 };
        let padding = px(if is_diff { 9.0 } else { 14.0 });
        let wrap_width = (self.width - gutter - px(32.0)).max(px(1.0));
        let metrics = (
            self.family.clone(),
            self.size,
            if is_diff { px(0.0) } else { wrap_width },
            window.scale_factor(),
        );
        if self.metrics.as_ref() != Some(&metrics) {
            // 使用与正文相同的字体换行器缓存物理行位置；窗口变宽后行号仍对应源代码行。
            let source = self.spec.source.clone();
            if let Ok(lines) = window.text_system().shape_text(
                source.clone(),
                self.size,
                &[TextRun {
                    len: source.len(),
                    font: gpui::font(self.family.clone()),
                    color: cx.theme().foreground,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                (!is_diff).then_some(wrap_width),
                None,
            ) {
                let mut top = px(0.0);
                self.content_width = lines
                    .iter()
                    .map(|line| line.size(line_height).width)
                    .fold(px(0.0), Pixels::max);
                self.line_tops = lines
                    .iter()
                    .map(|line| {
                        let current = top;
                        top += line.size(line_height).height;
                        current
                    })
                    .collect();
                self.metrics = Some(metrics);
            }
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let copied = self.feedback.read(cx).is_copied();
        let keyboard = self.focus.contains_focused(window, cx) && window.last_input_was_keyboard();
        let picker_focused = self.language.read(cx).focus_handle(cx).contains_focused(window, cx);
        let tools = self.hovered || copied || keyboard || picker_focused;
        // 终端原型使用轻微下沉的代码底色，不继承独立文档页的抬亮色面。
        let background =
            cx.theme().background.darken(if cx.theme().is_dark() { 0.18 } else { 0.035 });
        let ink = cx.theme().foreground;
        let mut code_style =
            gpui::StyleRefinement::default().p_0().rounded(px(0.0)).bg(gpui::transparent_black());
        code_style.text.font_family = Some(self.family.clone());
        code_style.text.font_size = Some(self.size.into());
        code_style.text.line_height = Some(line_height.into());
        if is_diff {
            code_style.text.white_space = Some(gpui::WhiteSpace::Nowrap);
        }
        let first = self.line_tops.partition_point(|top| self.top + *top + line_height < px(0.0));
        let last =
            self.line_tops.partition_point(|top| self.top + *top < window.viewport_size().height);
        let measured = cx.entity().downgrade();
        let block = div()
            .on_children_prepainted(move |bounds, _, cx| {
                if let Some(bounds) = bounds.get(usize::from(is_diff)) {
                    let _ = measured.update(cx, |this, cx| {
                        let width = bounds.size.width + px(32.0);
                        if (this.width - width).abs() > px(0.5)
                            || (this.top - bounds.top()).abs() > px(0.5)
                        {
                            this.width = width;
                            this.top = bounds.top();
                            cx.notify();
                        }
                    });
                }
            })
            .id("native-code-block")
            .debug_selector(|| "native-code-block".into())
            .relative()
            .w_full()
            .min_w_0()
            .when(is_diff, |block| block.min_w(self.content_width + gutter + px(34.0)))
            .when(!is_diff, |block| block.mt(px(3.0)).mb(px(8.0)))
            .px(px(16.0))
            .py(padding)
            .bg(background)
            .when(!is_diff, |block| {
                block.rounded(px(9.0)).border_1().border_color(cx.theme().foreground.opacity(0.09))
            })
            .font_family(self.family.clone())
            .text_size(self.size)
            .line_height(line_height)
            .text_color(ink)
            .track_focus(&self.focus)
            .when(!is_diff, |block| {
                block.on_hover(cx.listener(|this, hovered, _, cx| {
                    if this.hovered != *hovered {
                        this.hovered = *hovered;
                        cx.notify();
                    }
                }))
            })
            .when_some(self.spec.diff_rows.as_ref(), |block, rows| {
                // 背景和行号在选择文本下方绘制，不进入复制内容，也不接管选择手势。
                block.child(div().absolute().inset_0().children((first..last.min(rows.len())).map(
                    |index| {
                        let row = rows[index];
                        let color = match row.kind {
                            model::diff::Kind::Added => Some(cx.theme().success),
                            model::diff::Kind::Deleted => Some(cx.theme().danger),
                            _ => None,
                        };
                        div()
                            .id(("native-diff-row", index))
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(padding + self.line_tops[index])
                            .h(line_height)
                            .px(px(16.0))
                            .flex()
                            .items_center()
                            .when_some(color, |line, color| line.bg(color.opacity(0.09)))
                            .hover(|line| line.bg(cx.theme().list_hover))
                            .children([row.old, row.new].into_iter().enumerate().map(
                                |(side, number)| {
                                    let marked = matches!(
                                        (side, row.kind),
                                        (0, model::diff::Kind::Deleted)
                                            | (1, model::diff::Kind::Added)
                                    );
                                    div()
                                        .w(column)
                                        .flex_shrink_0()
                                        .pr(px(10.0))
                                        .text_right()
                                        .text_size(px(11.0))
                                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                                        .when(marked, |number| {
                                            number.border_r_2().border_color(color.unwrap())
                                        })
                                        .child(
                                            number.map_or_else(String::new, |number| {
                                                number.to_string()
                                            }),
                                        )
                                },
                            ))
                    },
                )))
            })
            .child(div().pl(gutter).w_full().min_w_0().child(
                TextView::new(&self.text).selectable(true).scrollable(false).style(TextViewStyle {
                    code_block: code_style,
                    highlight_theme: cx.theme().highlight_theme.clone(),
                    is_dark: cx.theme().is_dark(),
                    ..Default::default()
                }),
            ))
            .children((first..if is_diff { first } else { last }).map(|index| {
                div()
                    .absolute()
                    .left(px(16.0))
                    .top(padding + self.line_tops[index])
                    .w(gutter - px(14.0))
                    .text_right()
                    .text_size(px(11.0))
                    .line_height(line_height)
                    .text_color(cx.theme().muted_foreground.opacity(0.65))
                    .child((index + 1).to_string())
            }))
            .when(!is_diff, |block| {
                block.child(
                    div()
                        .absolute()
                        .top(px(5.0))
                        .right(px(5.0))
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .p(px(2.0))
                        .rounded(px(6.0))
                        .bg(background)
                        .font_family(cx.theme().font_family.clone())
                        .opacity(if tools { 1.0 } else { 0.0 })
                        .child(
                            div()
                                .debug_selector(|| "native-code-language".into())
                                .w(px(120.0))
                                .h(px(28.0))
                                .child(
                                    Select::new(&self.language)
                                        .appearance(false)
                                        .small()
                                        .h(px(28.0))
                                        .menu_width(px(210.0))
                                        .search_placeholder(
                                            language.text(Message::EditorSearchCodeLanguage),
                                        ),
                                ),
                        )
                        .child(
                            Button::new("native-code-copy")
                                .debug_selector(|| "native-code-copy".into())
                                .small()
                                .h(px(28.0))
                                .min_w(px(30.0))
                                .px(px(6.0))
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .foreground(cx.theme().muted_foreground)
                                        .hover(cx.theme().list_hover)
                                        .active(cx.theme().list_hover),
                                )
                                .tooltip(language.text(if copied {
                                    Message::NativeResultCopied
                                } else {
                                    Message::EditorCopyCode
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .text_size(px(11.0))
                                        .child(
                                            Icon::new(if copied {
                                                IconName::Check
                                            } else {
                                                IconName::Copy
                                            })
                                            .size(px(14.0)),
                                        )
                                        .when(copied, |button| {
                                            button.child(language.text(Message::NativeResultCopied))
                                        }),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        this.spec.source.to_string(),
                                    ));
                                    this.feedback
                                        .update(cx, |feedback, cx| feedback.mark_copied(cx));
                                    cx.stop_propagation();
                                })),
                        ),
                )
            });
        div()
            .id("native-code-scroll")
            .max_w_full()
            .min_w_0()
            .when(is_diff, |body| {
                body.overflow_x_scroll().restrict_scroll_to_axis().track_scroll(&self.scroll)
            })
            .child(block)
    }
}
