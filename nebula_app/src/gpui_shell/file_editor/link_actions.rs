//! Link actions retain the authored occurrence and the full resolved destination.

use super::*;
use gpui_component::text::TextLinkHover;
use std::ops::Range;

pub(super) struct LinkHover {
    block: usize,
    part: Option<usize>,
    url: SharedString,
    pub(super) range: Option<Range<usize>>,
    position: Point<Pixels>,
    revision: u64,
}

impl TextFileView {
    pub(super) fn update_link_hover(
        &mut self,
        block: usize,
        part: Option<usize>,
        rendered: &str,
        hover: Option<TextLinkHover>,
        cx: &mut Context<Self>,
    ) {
        let Some(hover) = hover else {
            if self.link_hover.as_ref().is_some_and(|link| link.block == block && link.part == part)
            {
                self.defer_link_close(cx);
            }
            return;
        };
        self.link_hide_task = None;
        let range = hover.source_range.and_then(|range| {
            if range.is_empty() {
                return None;
            }
            let raw = rendered.get(range.clone())?;
            let block_range = self.outline.source_ranges.get(block)?.clone();
            let original = self.source_slice(block_range.clone(), cx)?;
            let origin = part
                .and_then(|part| {
                    self.outline
                        .structures
                        .get(block)?
                        .as_ref()?
                        .parts
                        .get(part)
                        .map(|part| part.range.clone())
                })
                .unwrap_or(0..original.len());
            let authored = original.get(origin.clone())?;
            if range.end <= authored.len() && rendered.get(..authored.len()) == Some(authored) {
                return Some(
                    block_range.start + origin.start + range.start
                        ..block_range.start + origin.start + range.end,
                );
            }
            // 预览可能重写图片路径；用原链接语法及出现次序定位，不能按相同 URL 猜目标。
            let spans = |source: &str| {
                fn collect(node: &markdown::mdast::Node, output: &mut Vec<Range<usize>>) {
                    if matches!(
                        node,
                        markdown::mdast::Node::Link(_) | markdown::mdast::Node::LinkReference(_)
                    ) {
                        if let Some(position) = node.position() {
                            output.push(position.start.offset..position.end.offset);
                        }
                    }
                    if let Some(children) = node.children() {
                        for node in children {
                            collect(node, output);
                        }
                    }
                }
                let ast = markdown::to_mdast(source, &markdown::ParseOptions::gfm()).ok()?;
                let mut ranges = Vec::new();
                collect(&ast, &mut ranges);
                Some(ranges)
            };
            let occurrence = spans(rendered)?
                .into_iter()
                .filter(|span| rendered.get(span.clone()) == Some(raw))
                .position(|span| span == range)?;
            // 引用定义追加在预览末尾；原片段单独解析时引用可能尚未解析，保留预览定义参与识别。
            let original_preview = self.outline.part_source(block, authored);
            let start = spans(&original_preview)?
                .into_iter()
                .filter(|span| original_preview.get(span.clone()) == Some(raw))
                .nth(occurrence)?
                .start;
            if start + raw.len() > authored.len() {
                return None;
            }
            Some(
                block_range.start + origin.start + start
                    ..block_range.start + origin.start + start + raw.len(),
            )
        });
        self.link_hover = Some(LinkHover {
            block,
            part,
            url: hover.url,
            range,
            position: hover.position + gpui::point(px(0.0), hover.line_height + px(6.0)),
            revision: self.content_revision,
        });
        cx.notify();
    }

    fn defer_link_close(&mut self, cx: &mut Context<Self>) {
        self.link_hide_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let _ = view.update(cx, |view, cx| {
                view.link_hover = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn render_link_actions(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(link) = &self.link_hover else { return div().into_any_element() };
        let mounted = match link.part {
            Some(part) => self
                .inline_views
                .borrow()
                .get(&(link.block, part))
                .and_then(|view| view.upgrade())
                .is_some(),
            None => self.blocks.borrow().get(link.block).is_some_and(Option::is_some),
        };
        if !self.preview
            || self.content_revision != link.revision
            || !self.render_active
            || !mounted
        {
            return div().into_any_element();
        }
        let position = link.position;
        let url = link.url.clone();
        let open = url.clone();
        let copy = url.clone();
        let range = link.range.clone();
        let revision = link.revision;
        let editable = range.is_some() && self.object_editable();
        let panel = h_flex()
            .id("markdown-link-actions")
            .debug_selector(|| "markdown-link-actions".to_owned())
            .occlude()
            .gap(px(4.0))
            .p(px(4.0))
            .rounded(px(12.0))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|view, hovered, _, cx| {
                if *hovered {
                    view.link_hide_task = None;
                } else {
                    view.defer_link_close(cx);
                }
            }))
            .child(
                div()
                    .max_w(px(320.0))
                    .min_w_0()
                    .truncate()
                    .px(px(8.0))
                    .text_size(px(12.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(url.clone()),
            )
            .child(
                Button::new("link-edit")
                    .debug_selector(|| "markdown-link-edit".into())
                    .ghost()
                    .h(px(32.0))
                    .label("Edit")
                    .disabled(!editable)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        if let Some(range) = range.clone() {
                            view.edit_link(range, url.clone(), revision, window, cx);
                        }
                    })),
            )
            .child(
                Button::new("link-open")
                    .debug_selector(|| "markdown-link-open".into())
                    .ghost()
                    .h(px(32.0))
                    .label("Open")
                    .on_click(move |_, _, cx| cx.open_url(&open)),
            )
            .child(
                Button::new("link-copy")
                    .debug_selector(|| "markdown-link-copy".into())
                    .ghost()
                    .h(px(32.0))
                    .label("Copy")
                    .on_click(move |_, window, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.to_string()));
                        super::super::toast::toast(
                            window,
                            cx,
                            crate::display::ToastKind::Success,
                            super::super::config::ui_language(cx)
                                .text(Message::EditorClipboardCopied),
                        );
                    }),
            );
        gpui::deferred(
            gpui::anchored().position(position).snap_to_window_with_margin(px(8.0)).child(panel),
        )
        .with_priority(3)
        .into_any_element()
    }

    pub(super) fn edit_link(
        &mut self,
        range: Range<usize>,
        url: SharedString,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.content_revision != revision || !self.object_editable() {
            return;
        }
        let Some(original) = self.source_slice(range.clone(), cx) else { return };
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(url.clone(), window, cx);
            input
        });
        let owner = cx.weak_entity();
        let focus_input = input.clone();
        self.link_hover = None;
        self.link_hide_task = None;
        window.open_dialog(cx, move |dialog, window, cx| {
            let input = input.clone();
            let range = range.clone();
            let original = original.clone();
            let url = url.clone();
            let owner = owner.clone();
            let language = super::super::config::ui_language(cx);
            confirm_dialog(
                dialog,
                window,
                "Edit",
                "",
                language.text(Message::EditorFinishEditing),
                language.text(Message::EditorCancel),
                gpui_component::button::ButtonVariant::Primary,
            )
            .child(gpui_component::input::Input::new(&input))
            .on_ok(move |_, window, cx| {
                let next = input.read(cx).value().to_string();
                if next.trim().is_empty() || next.chars().any(char::is_control) {
                    return false;
                }
                owner
                    .update(cx, |view, cx| {
                        if view.content_revision != revision
                            || view.source_slice(range.clone(), cx).as_deref()
                                != Some(original.as_str())
                        {
                            return false;
                        }
                        if next != url.as_ref() {
                            let Some(replacement) = replace_destination(&original, &next) else {
                                return false;
                            };
                            view.commit_source_edit(range.clone(), &replacement, None, window, cx);
                            view.history.mark_object_edit();
                        }
                        true
                    })
                    .unwrap_or(false)
            })
        });
        focus_input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }
}

pub(super) fn replace_destination(source: &str, destination: &str) -> Option<String> {
    if source.starts_with('<') && source.ends_with('>') {
        return Some(format!("<{}>", destination.replace('>', "%3E").replace('<', "%3C")));
    }
    if !source.starts_with('[') {
        return Some(destination.to_owned());
    }
    // 标签可含嵌套括号、转义和代码；不能从 title 里的 ]( 反推标签边界。
    let close = label_end(source)?;
    let label = source.get(1..close)?;
    let destination = destination
        .replace('\\', "\\\\")
        .replace('&', "&amp;")
        .replace('>', "%3E")
        .replace('<', "%3C");
    let title = markdown::to_mdast(source, &markdown::ParseOptions::gfm()).ok().and_then(|node| {
        node.children().and_then(|nodes| nodes.first()).and_then(|node| node.children()).and_then(
            |nodes| {
                nodes.iter().find_map(|node| match node {
                    markdown::mdast::Node::Link(link) => link.title.clone(),
                    _ => None,
                })
            },
        )
    });
    Some(format!(
        "[{label}](<{destination}>{})",
        title.map_or(String::new(), |title| format!(
            " \"{}\"",
            title.replace('\\', "\\\\").replace('&', "&amp;").replace('"', "&quot;")
        ))
    ))
}

fn label_end(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = 1;
    let mut depth = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                index += 2;
                continue;
            },
            b'`' => {
                let count = bytes[index..].iter().take_while(|&&byte| byte == b'`').count();
                let mut end = index + count;
                while end < bytes.len() {
                    if bytes[end] == b'`' {
                        let run = bytes[end..].iter().take_while(|&&byte| byte == b'`').count();
                        if run == count {
                            index = end + run;
                            break;
                        }
                        end += run;
                    } else {
                        end += 1;
                    }
                }
                if end < bytes.len() {
                    continue;
                }
                index += count;
                continue;
            },
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            },
            _ => {},
        }
        index += 1;
    }
    None
}
