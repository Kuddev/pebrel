//! Git patch 的文件边界和折叠归结果拥有；正文沿用同一套可选择代码组件。
use super::*;
use gpui::SharedString;
use gpui_component::button::ButtonCustomVariant;

impl NativeResult {
    pub(super) fn parse_diff(&mut self, from_grid: bool, cx: &mut Context<Self>) {
        if self.diff.is_some() || self.parsing || self.notice.is_some() {
            return;
        }
        self.parsing = true;
        let raw = self.raw.text.clone();
        let revision = self.revision;
        self.task = Some(cx.spawn(async move |this, cx| {
            let patch = cx
                .background_spawn(async move { model::diff::Patch::parse(&raw, from_grid) })
                .await;
            let _ = this.update(cx, |this, cx| {
                // 清空、驱逐或替换后到达的旧解析结果不能重新占据终端输出。
                if this.revision != revision || this.status == Status::Evicted {
                    return;
                }
                this.parsing = false;
                match patch {
                    Ok(patch) => this.diff = Some(Arc::new(patch)),
                    Err(error) => {
                        this.notice = Some(if error == "limit" {
                            Message::NativeResultDiffLimit
                        } else {
                            Message::NativeResultDiffInvalid
                        });
                        this.raw_mode = true;
                    },
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn diff_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(patch) = self.diff.clone() else { return self.raw_element(cx) };
        let language = crate::gpui_shell::config::ui_language(cx);
        let background =
            cx.theme().background.darken(if cx.theme().is_dark() { 0.18 } else { 0.035 });
        let mut output = div().w_full().min_w_0().pl(px(18.0)).pr(px(16.0));
        if !patch.preamble.is_empty() {
            output = output.child(self.code_element(
                &code::Spec {
                    source: SharedString::from(&patch.preamble),
                    language: "text".into(),
                    offset: usize::MAX,
                    lines: patch.preamble.split('\n').count(),
                    diff_rows: None,
                },
                window,
                cx,
            ));
        }
        for (index, file) in patch.files.iter().enumerate() {
            let key = file.source.start;
            let collapsed = self.collapsed_files.contains(&key);
            let toggle_label = language.text(if collapsed {
                Message::NativeResultDiffExpand
            } else {
                Message::NativeResultDiffCollapse
            });
            let header = div()
                .flex()
                .items_center()
                .gap(px(9.0))
                .px(px(12.0))
                .py(px(6.0))
                .font_family(cx.theme().font_family.clone())
                .text_size(px(12.0))
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::File).size(px(14.0)))
                .child(div().flex_1().min_w_0().text_ellipsis().child(field_preview(&file.path)))
                .child(
                    div()
                        .flex()
                        .gap(px(9.0))
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .child(
                            div().text_color(cx.theme().success).child(format!("+{}", file.added)),
                        )
                        .child(
                            div().text_color(cx.theme().danger).child(format!("−{}", file.deleted)),
                        ),
                )
                .child(
                    Button::new(("native-diff-collapse", key))
                        .debug_selector(move || format!("native-diff-collapse-{index}"))
                        .small()
                        .h(px(28.0))
                        .w(px(28.0))
                        .px_0()
                        .rounded(px(5.0))
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .foreground(cx.theme().muted_foreground)
                                .hover(cx.theme().list_hover)
                                .active(cx.theme().list_hover),
                        )
                        .aria_label(toggle_label)
                        .tooltip(toggle_label)
                        .child(
                            Icon::new(if collapsed {
                                IconName::ChevronRight
                            } else {
                                IconName::ChevronDown
                            })
                            .size(px(14.0)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // Button 不因鼠标点击自动取焦点；先离开 PTY，后续 Tab 才能进入文件控件。
                            // 键盘激活时保留按钮焦点，允许连续展开/折叠。
                            if !window.last_input_was_keyboard() {
                                window.focus(&this.focus, cx);
                            }
                            if !this.collapsed_files.remove(&key) {
                                this.collapsed_files.insert(key);
                            }
                            cx.notify();
                            cx.stop_propagation();
                        })),
                );
            let mut item = div()
                .id(("native-diff-file", key))
                .debug_selector(move || format!("native-diff-file-{index}"))
                .w_full()
                .min_w_0()
                .mb(px(8.0))
                .rounded(px(9.0))
                .overflow_hidden()
                .bg(background)
                .border_1()
                .border_color(cx.theme().foreground.opacity(0.09))
                .child(header);
            if !collapsed {
                // 纯重命名、权限或二进制摘要没有 hunk，仍显示 Git 提供的元数据。
                if file.rows.is_empty() {
                    item = item.child(
                        div()
                            .px(px(12.0))
                            .pb(px(9.0))
                            .text_size(px(12.0))
                            .text_color(cx.theme().muted_foreground)
                            .children(
                                file.details.iter().map(|line| div().child(field_preview(line))),
                            ),
                    );
                } else {
                    item = item.child(
                        div()
                            .debug_selector(move || format!("native-diff-body-{index}"))
                            .w_full()
                            .min_w_0()
                            .child(self.code_element(
                                &code::Spec {
                                    source: SharedString::from(&file.body),
                                    language: "diff".into(),
                                    offset: key,
                                    lines: file.max_line as usize,
                                    diff_rows: Some(file.rows.clone()),
                                },
                                window,
                                cx,
                            )),
                    );
                }
            }
            output = output.child(item);
        }
        output.into_any_element()
    }
}
