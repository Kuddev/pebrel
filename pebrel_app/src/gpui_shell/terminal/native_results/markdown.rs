//! 保留 Markdown 语义与选择能力，只替换原型要求不同的块级呈现。
use super::*;
use gpui::{App, FontWeight, SharedString, TextRun};
use gpui_component::text::markdown_ast::{AlignKind, Node};

#[derive(Clone)]
struct Heading {
    source: SharedString,
    depth: u8,
    offset: usize,
}

struct Cell {
    source: SharedString,
    text: SharedString,
}

struct Table {
    rows: Vec<Vec<Cell>>,
    align: Vec<AlignKind>,
    offset: usize,
}

#[derive(Default)]
struct TableLayout {
    widths: Vec<Pixels>,
    fonts: Option<(gpui::Font, gpui::Font)>,
    size: Pixels,
    scale: f32,
    source: String,
    scroll: gpui::ScrollHandle,
}

fn needs_document_context(node: &Node) -> bool {
    // 引用和公式继续交给拥有全文定义及科学渲染上下文的现有 TextView。
    matches!(node, Node::LinkReference(_) | Node::FootnoteReference(_) | Node::InlineMath(_))
        || node.children().is_some_and(|children| children.iter().any(needs_document_context))
}

fn table_html_cells(html: &str) -> Vec<&str> {
    // 只拆固定 markdown-rs 生成的 table 包装；用户文本已经由编译器转义。
    // 先在原表格上下文解析，避免把单元格的 # / - / --- 再解释成块级结构。
    let mut cells = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<t") {
        rest = &rest[start + 2..];
        let Some(kind @ (b'h' | b'd')) = rest.as_bytes().first().copied() else { continue };
        if !matches!(rest.as_bytes().get(1), Some(b'>' | b' ')) {
            continue;
        }
        let Some((_, body)) = rest.split_once('>') else { break };
        let close = if kind == b'h' { "</th>" } else { "</td>" };
        let Some((cell, remaining)) = body.split_once(close) else { break };
        cells.push(cell);
        rest = remaining;
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_result_inline_content_is_compiled_in_its_original_context() {
        let heading = ::markdown::to_html("# ---");
        assert_eq!(heading, "<h1>---</h1>");
        let source = "| value |\n| --- |\n| # title |\n| - item |\n| **strong** and `code` |";
        let html = ::markdown::to_html_with_options(source, &::markdown::Options::gfm()).unwrap();
        assert_eq!(
            table_html_cells(&html),
            ["value", "# title", "- item", "<strong>strong</strong> and <code>code</code>"]
        );
    }
}

pub(super) fn extensions(owner: gpui::WeakEntity<NativeResult>) -> MarkdownExtensions {
    let code_owner = owner.clone();
    let table_owner = owner.clone();
    MarkdownExtensions::default()
        .block_parser(|node, context| {
            let raw = context.node_source(node)?;
            if super::super::answer_reader::contains_uncontrolled_media(node) {
                return Some(
                    MarkdownNode::new("native-literal", ())
                        .text(raw.to_owned())
                        .markdown(raw.to_owned()),
                );
            }
            let offset = node.position()?.start.offset + context.offset();
            let custom = match node {
                Node::Heading(heading) => {
                    if needs_document_context(node) {
                        return None;
                    }
                    let html =
                        ::markdown::to_html_with_options(raw, &::markdown::Options::gfm()).ok()?;
                    let source = html.split_once('>')?.1.rsplit_once("</h")?.0;
                    MarkdownNode::new(
                        "native-heading",
                        Heading {
                            source: format!("<p>{source}</p>").into(),
                            depth: heading.depth,
                            offset,
                        },
                    )
                },
                Node::Code(code)
                    if !matches!(code.lang.as_deref(), Some("math" | "latex" | "tex")) =>
                {
                    MarkdownNode::new(
                        "native-code",
                        super::code::Spec {
                            source: code.value.clone().into(),
                            language: code.lang.clone().unwrap_or_default().into(),
                            offset,
                            lines: code.value.lines().count().max(1),
                            diff_rows: None,
                        },
                    )
                },
                Node::Table(table) => {
                    if needs_document_context(node) {
                        return None;
                    }
                    let html =
                        ::markdown::to_html_with_options(raw, &::markdown::Options::gfm()).ok()?;
                    let mut content = table_html_cells(&html).into_iter();
                    let rows = table
                        .children
                        .iter()
                        .filter_map(|row| row.children())
                        .map(|cells| {
                            cells
                                .iter()
                                .map(|cell| {
                                    Some(Cell {
                                        source: format!("<p>{}</p>", content.next()?).into(),
                                        text: cell.to_string().into(),
                                    })
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .collect::<Option<Vec<_>>>()?;
                    MarkdownNode::new(
                        "native-markdown-table",
                        Table { rows, align: table.align.clone(), offset },
                    )
                },
                _ => return None,
            };
            Some(custom.text(node.to_string()).markdown(raw.to_owned()))
        })
        .block_renderer("native-literal", |node, _, _| div().child(node.as_text().to_owned()))
        .block_renderer("native-heading", move |node, _, cx| {
            let heading = node.data::<Heading>().expect("heading parser data");
            let size = owner.upgrade().map_or(px(14.0), |owner| owner.read(cx).font_size);
            let heading_size = size
                * if heading.depth == 1 {
                    1.3
                } else if heading.depth == 2 {
                    1.05
                } else {
                    1.0
                };
            div()
                .debug_selector(|| "native-markdown-heading".into())
                .font_family(cx.theme().font_family.clone())
                .text_size(heading_size)
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(heading_size * if heading.depth == 1 { 1.4 } else { 1.5 })
                .when(heading.offset > 0, |heading| heading.mt(px(10.0)))
                .pb(px(if heading.depth == 1 { 9.0 } else { 7.0 }))
                .child(
                    TextView::html(("native-heading", heading.offset), heading.source.clone())
                        .selectable(true)
                        .scrollable(false),
                )
        })
        .block_renderer("native-code", move |node, window, cx| {
            super::code::render(
                node.data::<super::code::Spec>().expect("code parser data"),
                &code_owner,
                window,
                cx,
            )
        })
        .block_renderer("native-markdown-table", move |node, window, cx| {
            render_table(node, &table_owner, window, cx)
        })
}

fn render_table(
    node: &MarkdownNode,
    owner: &gpui::WeakEntity<NativeResult>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let table = node.data::<Table>().expect("table parser data");
    let Some(owner) = owner.upgrade() else { return div().into_any_element() };
    let result = owner.read(cx);
    let size = result.font_size;
    let mut body_font = window.text_style().font();
    body_font.family = result.font_family.clone();
    let mut heading_font = body_font.clone();
    heading_font.family = cx.theme().font_family.clone();
    heading_font.weight = FontWeight::MEDIUM;
    let fonts = (body_font, heading_font);
    let scale = window.scale_factor();
    let state =
        window.use_keyed_state(("native-markdown-table-layout", table.offset), cx, |_, _| {
            TableLayout::default()
        });
    state.update(cx, |layout, cx| {
        if layout.source == node.as_markdown()
            && layout.fonts.as_ref() == Some(&fonts)
            && layout.size == size
            && layout.scale == scale
        {
            return;
        }
        layout.source = node.as_markdown().to_owned();
        layout.fonts = Some(fonts.clone());
        layout.size = size;
        layout.scale = scale;
        layout.widths = vec![px(0.0); table.align.len()];
        for (row, cells) in table.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate().take(layout.widths.len()) {
                let text = field_preview(&cell.text);
                let font = if row == 0 { fonts.1.clone() } else { fonts.0.clone() };
                let width = window
                    .text_system()
                    .shape_line(
                        text.clone().into(),
                        if row == 0 { px(12.0) } else { size },
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
                    .width;
                // 布局会按设备像素取整；向上保留内容宽度，避免刚好容纳的中文末字换行。
                let width = px((width.min(px(320.0)).as_f32() * scale).ceil() / scale);
                layout.widths[column] = layout.widths[column]
                    .max(width + px(if column + 1 == table.align.len() { 8.0 } else { 38.0 }));
            }
        }
    });
    let layout = state.read(cx);
    let total = layout.widths.iter().copied().fold(px(0.0), |sum, width| sum + width);
    div()
        .id(("native-markdown-table", table.offset))
        .debug_selector(|| "native-markdown-table".into())
        .max_w_full()
        .min_w_0()
        .mt(px(10.0))
        .mb(px(16.0))
        .overflow_x_scroll()
        .restrict_scroll_to_axis()
        .track_scroll(&layout.scroll)
        .child(div().w(total).children(table.rows.iter().enumerate().map(|(row, cells)| {
            div()
                .flex()
                .w_full()
                .when(row + 1 < table.rows.len(), |row| {
                    row.border_b_1().border_color(cx.theme().foreground.opacity(0.09))
                })
                .when(row == 0, |row| {
                    row.font_family(cx.theme().font_family.clone())
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                })
                .children(cells.iter().enumerate().take(layout.widths.len()).map(
                    |(column, cell)| {
                        div()
                            .w(layout.widths[column])
                            .flex_shrink_0()
                            .py(px(6.0))
                            .pr(px(if column + 1 == cells.len() { 8.0 } else { 38.0 }))
                            .when(table.align[column] == AlignKind::Right, |cell| cell.text_right())
                            .when(table.align[column] == AlignKind::Center, |cell| {
                                cell.text_center()
                            })
                            .child(
                                TextView::html(
                                    ("native-markdown-cell", row * table.align.len() + column),
                                    cell.source.clone(),
                                )
                                .selectable(true)
                                .scrollable(false),
                            )
                    },
                ))
        })))
        .into_any_element()
}
