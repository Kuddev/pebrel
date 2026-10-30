//! 表格的自然列宽、横向视口与控件；纵向位置仍由终端投影拥有。
use super::*;
use gpui::{App, FontWeight, SharedString, TextRun};
use gpui_component::button::ButtonCustomVariant;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub(super) struct Layout {
    samples: Vec<String>,
    numeric: Vec<bool>,
    order: Vec<usize>,
    widths: Vec<Pixels>,
    labels: Vec<String>,
    font_size: Pixels,
    font_family: SharedString,
    ui_family: SharedString,
    icon_column: Option<usize>,
}

impl Layout {
    pub(super) fn prepare(table: &model::Table) -> Self {
        let mut result = Self {
            samples: vec![String::new(); table.columns.len()],
            numeric: vec![true; table.columns.len()],
            order: (0..table.columns.len()).collect(),
            ..Default::default()
        };
        let mut widths = vec![0; table.columns.len()];
        let mut seen = vec![false; table.columns.len()];
        // 与解析一起在后台挑选有界测量样本；滚动不会重新扫描整张表。
        for row in &table.rows {
            for (column, value) in row.iter().enumerate() {
                let sample = field_preview(value);
                let width = sample.width();
                if width > widths[column] {
                    result.samples[column] = sample;
                    widths[column] = width;
                }
                if !value.trim().is_empty() {
                    seen[column] = true;
                    result.numeric[column] &= model::numeric_cell(value).is_some();
                }
            }
        }
        for (numeric, seen) in result.numeric.iter_mut().zip(seen) {
            *numeric &= seen;
        }
        if let Some(files) = &table.files {
            result.icon_column = files
                .kinds
                .iter()
                .any(|kind| *kind != model::FileKind::Unknown)
                .then_some(files.name_column);
            result.order.retain(|column| *column != files.name_column);
            result.order.insert(0, files.name_column);
            for (column, name) in table.columns.iter().enumerate() {
                result.numeric[column] = matches!(name.as_str(), "Size" | "Length" | "Links");
            }
        }
        result
    }

    fn measure(
        &mut self,
        labels: Vec<String>,
        font_size: Pixels,
        family: SharedString,
        window: &mut Window,
        cx: &App,
    ) {
        let ui_family = cx.theme().font_family.clone();
        if self.labels == labels
            && self.font_size == font_size
            && self.font_family == family
            && self.ui_family == ui_family
            && self.widths.len() == labels.len()
        {
            return;
        }
        self.widths = labels
            .iter()
            .enumerate()
            .map(|(column, label)| {
                let mut heading_font = gpui::font(ui_family.clone());
                heading_font.weight = FontWeight::MEDIUM;
                let measure = |text: String, font, size| {
                    window
                        .text_system()
                        .shape_line(
                            text.clone().into(),
                            size,
                            &[TextRun {
                                len: text.len(),
                                font,
                                color: cx.theme().foreground,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                        )
                        .width
                };
                let header = measure(field_preview(label), heading_font, px(12.0)) + px(16.0);
                let mut sample = measure(
                    self.samples[column].clone(),
                    gpui::font(family.clone()),
                    font_size * 0.94,
                )
                .min(px(280.0));
                if self.icon_column == Some(column) {
                    sample += px(20.0);
                }
                if label == "CPUPerc" {
                    sample = sample.max(px(48.0)) + px(84.0);
                }
                header.max(sample)
                    + px(if self.order.last() == Some(&column) { 20.0 } else { 30.0 })
            })
            .collect();
        self.labels = labels;
        self.font_size = font_size;
        self.font_family = family;
        self.ui_family = ui_family;
    }
}

fn column_label(table: &model::Table, column: usize, cx: &App) -> String {
    let name = &table.columns[column];
    let message = table.files.as_ref().and_then(|_| match name.as_str() {
        "Name" => Some(Message::NativeResultFileName),
        "Mode" => Some(Message::NativeResultFileMode),
        "Links" => Some(Message::NativeResultFileLinks),
        "Owner" => Some(Message::NativeResultFileOwner),
        "Group" => Some(Message::NativeResultFileGroup),
        "Size" | "Length" => Some(Message::NativeResultFileSize),
        "Modified" | "LastWriteTime" => Some(Message::NativeResultFileModified),
        _ => None,
    });
    message.map_or_else(
        || name.clone(),
        |id| crate::gpui_shell::config::ui_language(cx).text(id).to_owned(),
    )
}

impl NativeResult {
    pub(super) fn table_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let Some(table) = self.table.clone() else {
            return div()
                .py_2()
                .text_color(cx.theme().muted_foreground)
                .child(language.text(Message::NativeResultPreparing))
                .into_any_element();
        };
        let labels =
            (0..table.columns.len()).map(|column| column_label(&table, column, cx)).collect();
        self.table_layout.measure(labels, self.font_size, self.font_family.clone(), window, cx);
        let layout = &self.table_layout;
        let natural: Pixels = layout.widths.iter().copied().fold(px(0.0), |sum, w| sum + w);
        let viewport_width = (self.available_width - px(34.0)).max(px(0.0)).min(px(820.0));
        let table_width = natural.max(viewport_width);
        let ratio = (table_width.as_f32() / natural.as_f32().max(1.0)).max(1.0);
        let body_size = self.font_size * 0.94;
        let row_height = body_size * 1.65 + px(17.0);
        let header_height = px(43.0);
        let line = cx.theme().foreground.opacity(0.09);
        let muted = cx.theme().muted_foreground;
        let headings = table.files.as_ref().is_none_or(|files| files.headers);
        let preamble = table.files.as_ref().map_or(&[][..], |files| files.preamble.as_slice());
        let prefix_height = 28.0
            + if self.filter_open { 44.0 } else { 0.0 }
            + if headings { header_height.as_f32() } else { 0.0 }
            + preamble.len() as f32 * 22.0
            + 16.0;
        let first =
            ((self.visible_top - prefix_height).max(0.0) / row_height.as_f32()).floor() as usize;
        let last = ((self.visible_top + self.visible_height - prefix_height).max(0.0)
            / row_height.as_f32())
        .ceil() as usize
            + 2;
        let mut header = div()
            .flex()
            .w(table_width)
            .h(header_height)
            .bg(cx.theme().foreground.opacity(0.035))
            .rounded(px(6.0))
            .border_b_1()
            .border_color(line);
        for &column in &layout.order {
            let selected = self.sort.filter(|(selected, _)| *selected == column);
            let arrow = selected.map_or("", |(_, desc)| if desc { "↓" } else { "↑" });
            let right = layout.numeric[column];
            header = header.child(
                Button::new(("native-column", column))
                    .debug_selector(move || format!("native-table-column-{column}"))
                    .small()
                    .h(header_height)
                    .w(layout.widths[column] * ratio)
                    .flex_shrink_0()
                    .px_0()
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .foreground(muted)
                            .hover(cx.theme().foreground.opacity(0.035))
                            .active(cx.theme().foreground.opacity(0.07)),
                    )
                    .tooltip(layout.labels[column].clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .min_w_0()
                            .pl(px(10.0))
                            .pr(px(if layout.order.last() == Some(&column) { 10.0 } else { 20.0 }))
                            .gap(px(7.0))
                            .font_family(cx.theme().font_family.clone())
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(12.0))
                            .line_height(px(18.0))
                            .when(right, |cell| cell.justify_end())
                            .text_color(if selected.is_some() { cx.theme().primary } else { muted })
                            .child(field_preview(&layout.labels[column]))
                            .child(div().w(px(9.0)).child(arrow)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.focus, cx);
                        this.sort =
                            Some((column, !this.sort.is_some_and(|(c, desc)| c == column && desc)));
                        this.order_table(cx);
                    })),
            );
        }
        let rows = (first.saturating_sub(2)..last.min(self.order.len()))
            .map(|index| {
                let source_row = self.order[index];
                div()
                    // GPUI 依靠稳定元素状态发出 hover 通知；只有 selector 会让背景等到其他事件才重绘。
                    .id(("native-table-row", source_row))
                    .debug_selector(move || format!("native-table-row-{index}"))
                    .absolute()
                    .top(row_height * index as f32)
                    .h(row_height)
                    .w(table_width)
                    .flex()
                    .text_size(body_size)
                    .line_height(body_size * 1.65)
                    .when(index + 1 < self.order.len(), |row| row.border_b_1().border_color(line))
                    .hover(|style| style.bg(cx.theme().foreground.opacity(0.035)))
                    .children(layout.order.iter().map(|&column| {
                        let value = &table.rows[source_row][column];
                        let kind = table
                            .files
                            .as_ref()
                            .filter(|files| files.name_column == column)
                            .map_or(model::FileKind::Unknown, |files| files.kinds[source_row]);
                        let icon = match kind {
                            model::FileKind::Directory => Some(IconName::FolderClosed),
                            model::FileKind::File | model::FileKind::Link => Some(IconName::File),
                            _ => None,
                        };
                        let percent = (table.columns[column] == "CPUPerc"
                            && value.trim().ends_with('%'))
                        .then(|| model::numeric_cell(value))
                        .flatten();
                        div()
                            .w(layout.widths[column] * ratio)
                            .flex_shrink_0()
                            .min_w_0()
                            .pl(px(10.0))
                            .pr(px(if layout.order.last() == Some(&column) { 10.0 } else { 20.0 }))
                            .py(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .when(layout.numeric[column] && percent.is_none(), |cell| {
                                cell.justify_end()
                            })
                            .when(
                                table
                                    .files
                                    .as_ref()
                                    .is_some_and(|files| column != files.name_column),
                                |cell| cell.text_color(muted),
                            )
                            .when_some(icon, |cell, icon| {
                                cell.child(Icon::new(icon).size(px(14.0)).text_color(muted))
                            })
                            .child(div().min_w_0().text_ellipsis().child(field_preview(value)))
                            .when_some(percent, |cell, value| {
                                cell.child(
                                    div()
                                        .flex()
                                        .gap(px(3.0))
                                        .ml(px(6.0))
                                        .w(px(72.0))
                                        .flex_shrink_0()
                                        .children((0..10).map(|segment| {
                                            let fill =
                                                (value / 10.0 - f64::from(segment)).clamp(0.0, 1.0);
                                            div()
                                                .w(px(4.0))
                                                .h(px(4.0))
                                                .bg(cx.theme().foreground.opacity(0.12))
                                                .child(
                                                    div()
                                                        .h_full()
                                                        .w(px(4.0 * fill as f32))
                                                        .bg(cx.theme().primary),
                                                )
                                        })),
                                )
                            })
                    }))
            })
            .collect::<Vec<_>>();
        let total = table.rows.len().to_string();
        let shown = self.order.len().to_string();
        let count = if self.filter.read(cx).value().is_empty() {
            language.format(Message::NativeResultRowCount, &[("count", &total)])
        } else {
            language.format(
                Message::NativeResultFilteredCount,
                &[("visible", &shown), ("total", &total)],
            )
        };
        div()
            .w_full()
            .min_w_0()
            .pl(px(18.0))
            .pr(px(16.0))
            .flex()
            .flex_col()
            .children(preamble.iter().map(|line| {
                div().h(px(22.0)).text_color(muted).text_size(px(12.0)).child(field_preview(line))
            }))
            .child(
                div()
                    .h(px(28.0))
                    .font_family(cx.theme().font_family.clone())
                    .text_size(px(12.0))
                    .line_height(px(18.0))
                    .text_color(muted)
                    .child(count),
            )
            .when(self.filter_open, |body| {
                body.child(
                    div()
                        .debug_selector(|| "native-result-filter-input".into())
                        .w(px(340.0))
                        .max_w_full()
                        .mb(px(10.0))
                        .border_b_1()
                        .border_color(line)
                        .child(
                            Input::new(&self.filter)
                                .appearance(false)
                                .focus_bordered(false)
                                .cleanable(true)
                                .h(px(34.0))
                                .prefix(
                                    Icon::new(IconName::Search).size(px(14.0)).text_color(muted),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .relative()
                    .w(viewport_width)
                    .max_w_full()
                    .min_w_0()
                    .child(
                        div()
                            .id("native-table-horizontal")
                            .w_full()
                            .min_w_0()
                            .overflow_x_scroll()
                            .restrict_scroll_to_axis()
                            .track_scroll(&self.table_scroll)
                            .child(
                                div()
                                    .w(table_width)
                                    .when(headings, |body| body.child(header))
                                    .child(
                                        div()
                                            .relative()
                                            .w(table_width)
                                            .h(row_height * self.order.len() as f32)
                                            .children(rows),
                                    ),
                            ),
                    )
                    .when(table_width > viewport_width, |body| {
                        body.child(gpui_component::scroll::Scrollbar::horizontal(
                            &self.table_scroll,
                        ))
                    }),
            )
            .when(self.order.is_empty(), |body| {
                body.child(
                    div()
                        .py(px(14.0))
                        .text_size(px(12.0))
                        .text_color(muted)
                        .child(language.text(Message::NativeResultNoRows)),
                )
            })
            .into_any_element()
    }
}
