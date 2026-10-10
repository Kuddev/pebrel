use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ObjectFit, ParentElement as _, Render,
    RenderImage, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    WeakEntity, Window, div, img, px,
};
use gpui_component::text::{
    MarkdownExtensions, MarkdownNode, TextView, TextViewState, TextViewStyle, markdown_ast,
};

use crate::assistant_answer::AnswerSnapshot;
use crate::assistant_answer::document::{self, AnswerImage, IMAGE_LANGUAGE, ReaderDocument};
use crate::gpui_shell::prelude::*;
use crate::i18n::{Message, UiLanguage};

mod messages;
use messages::ImageFailure;
#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;

const IMAGE_BUDGET: usize = 64 * 1024 * 1024;
const LITERAL_BLOCK: &str = "nebula-answer-literal";

pub(super) enum ReaderEvent {
    Close,
}

enum ImageStatus {
    Waiting,
    Loading,
    Ready(Arc<RenderImage>),
    Failed(ImageFailure),
}

struct ReaderImage {
    spec: AnswerImage,
    selected_path: Option<PathBuf>,
    status: ImageStatus,
    revision: u64,
    bytes: usize,
}

pub(super) struct AnswerReader {
    pub focus_handle: FocusHandle,
    snapshot: AnswerSnapshot,
    text: Entity<TextViewState>,
    raw_text: Entity<TextViewState>,
    raw_mode: bool,
    raw_initialized: bool,
    pending_markdown: Option<String>,
    extensions: MarkdownExtensions,
    images_omitted: usize,
    images: Vec<ReaderImage>,
    image_queue: VecDeque<usize>,
    decoding: bool,
    image_bytes: usize,
    preview: Option<Arc<RenderImage>>,
    preparing: bool,
    attention: bool,
    newer_answer: bool,
}

impl EventEmitter<ReaderEvent> for AnswerReader {}

impl AnswerReader {
    pub fn new(snapshot: AnswerSnapshot, cx: &mut Context<Self>) -> Self {
        let source = snapshot.content.source().cloned();
        let reader = Self {
            focus_handle: cx.focus_handle(),
            snapshot,
            text: cx.new(|cx| TextViewState::markdown("", cx)),
            raw_text: cx.new(|cx| TextViewState::markdown("", cx)),
            raw_mode: false,
            raw_initialized: false,
            pending_markdown: None,
            extensions: MarkdownExtensions::default(),
            images_omitted: 0,
            images: Vec::new(),
            image_queue: VecDeque::new(),
            decoding: false,
            image_bytes: 0,
            preview: None,
            preparing: source.is_some(),
            attention: false,
            newer_answer: false,
        };
        if let Some(source) = source {
            cx.spawn(async move |reader, cx| {
                let document = cx.background_spawn(async move { document::prepare(&source) }).await;
                let _ = reader.update(cx, |reader, cx| reader.prepared(document, cx));
            })
            .detach();
        }
        reader
    }

    pub fn needs_attention(&mut self, cx: &mut Context<Self>) {
        self.attention = true;
        cx.notify();
    }

    pub fn answer_arrived(&mut self, cx: &mut Context<Self>) {
        self.newer_answer = true;
        cx.notify();
    }

    fn prepared(&mut self, document: ReaderDocument, cx: &mut Context<Self>) {
        let starts = document.image_placeholder_starts;
        self.images = document
            .images
            .into_iter()
            .map(|spec| ReaderImage {
                spec,
                selected_path: None,
                status: ImageStatus::Waiting,
                revision: 0,
                bytes: 0,
            })
            .collect();
        self.images_omitted = document.images_omitted;
        let image_marker = super::super::config::ui_language(cx).text(Message::ReaderImageMarker);
        let weak = cx.entity().downgrade();
        self.extensions = MarkdownExtensions::default()
            .block_parser(move |node, context| {
                if contains_uncontrolled_media(node) {
                    let source = context.node_source(node)?.to_owned();
                    return Some(
                        MarkdownNode::new(LITERAL_BLOCK, ()).text(source.clone()).markdown(source),
                    );
                }
                let index = image_placeholder_index(node, context.offset(), &starts)?;
                let source = context.node_source(node)?.to_owned();
                Some(MarkdownNode::new(IMAGE_LANGUAGE, index).text(image_marker).markdown(source))
            })
            .block_renderer(IMAGE_LANGUAGE, move |node, _, cx| match node.data::<usize>() {
                Some(index) => render_image(&weak, *index, cx),
                None => div().into_any_element(),
            })
            .block_renderer(LITERAL_BLOCK, |node, _, _| div().child(node.as_text().to_owned()));
        self.pending_markdown = Some(document.markdown);
        self.preparing = false;
        self.image_queue.extend(0..self.images.len());
        self.decode_next(cx);
        cx.notify();
    }

    fn decode_next(&mut self, cx: &mut Context<Self>) {
        if self.decoding {
            return;
        }
        let Some(index) = self.image_queue.pop_front() else { return };
        let image = &mut self.images[index];
        image.status = ImageStatus::Loading;
        let revision = image.revision;
        let selected_path = image.selected_path.clone();
        let target = image.spec.target.clone();
        let base = self.snapshot.cwd.clone();
        self.decoding = true;
        cx.spawn(async move |reader, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = match selected_path {
                        Some(path) => path,
                        None => document::local_image_path(
                            &target,
                            base.as_deref().ok_or(ImageFailure::MissingDirectory)?,
                        )
                        .map_err(ImageFailure::Read)?,
                    };
                    let bytes = document::read_image(&path).map_err(ImageFailure::Read)?;
                    super::inline_image::decode_bytes(&bytes).map_err(ImageFailure::Decode)
                })
                .await;
            let _ = reader.update(cx, |reader, cx| {
                reader.decoding = false;
                if reader.images[index].revision == revision {
                    match result {
                        Ok((image, bytes))
                            if reader.image_bytes.saturating_add(bytes) <= IMAGE_BUDGET =>
                        {
                            reader.image_bytes += bytes;
                            reader.images[index].bytes = bytes;
                            reader.images[index].status = ImageStatus::Ready(image);
                        },
                        Ok(_) => {
                            reader.images[index].status =
                                ImageStatus::Failed(ImageFailure::TotalBudget)
                        },
                        Err(error) => reader.images[index].status = ImageStatus::Failed(error),
                    }
                }
                reader.decode_next(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn choose_image(&mut self, index: usize, cx: &mut Context<Self>) {
        let language = super::super::config::ui_language(cx);
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(language.text(Message::ReaderChooseImagePrompt).into()),
        });
        cx.spawn(async move |reader, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = reader.update(cx, |reader, cx| {
                let image = &mut reader.images[index];
                reader.image_bytes = reader.image_bytes.saturating_sub(image.bytes);
                image.bytes = 0;
                image.revision += 1;
                image.selected_path = Some(path);
                image.status = ImageStatus::Waiting;
                reader.image_queue.retain(|queued| *queued != index);
                reader.image_queue.push_back(index);
                reader.decode_next(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.preview.take().is_none() {
                cx.emit(ReaderEvent::Close);
            }
            cx.notify();
            cx.stop_propagation();
        } else if event.keystroke.modifiers.control
            && event.keystroke.modifiers.shift
            && event.keystroke.key == "c"
        {
            let text = if self.raw_mode { &self.raw_text } else { &self.text };
            let selected = text.read(cx).selected_text();
            if !selected.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(selected));
            }
            cx.stop_propagation();
        }
    }

    fn notice(&self, language: UiLanguage) -> Option<String> {
        if self.images_omitted > 0 {
            Some(language.format(
                Message::ReaderImagesOmitted,
                &[("count", &self.images_omitted.to_string())],
            ))
        } else {
            self.snapshot.content.notice(language)
        }
    }
}

fn contains_uncontrolled_media(node: &markdown_ast::Node) -> bool {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if matches!(
            node,
            markdown_ast::Node::Image(_)
                | markdown_ast::Node::ImageReference(_)
                | markdown_ast::Node::Html(_)
        ) {
            return true;
        }
        if let Some(children) = node.children() {
            pending.extend(children);
        }
    }
    false
}

fn image_placeholder_index(
    node: &markdown_ast::Node,
    offset: usize,
    starts: &[usize],
) -> Option<usize> {
    let markdown_ast::Node::Code(code) = node else { return None };
    if code.lang.as_deref() != Some(IMAGE_LANGUAGE) {
        return None;
    }
    let index: usize = code.value.parse().ok()?;
    let position = node.position()?;
    (starts.get(index).copied()? == offset.checked_add(position.start.offset)?).then_some(index)
}

fn render_image(reader: &WeakEntity<AnswerReader>, index: usize, cx: &mut App) -> AnyElement {
    let language = super::super::config::ui_language(cx);
    let Some(entity) = reader.upgrade() else { return div().into_any_element() };
    let state = entity.read(cx);
    let Some(image) = state.images.get(index) else {
        return div()
            .py_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(language.text(Message::ReaderImagesOmittedInline))
            .into_any_element();
    };
    let target: SharedString = image.spec.target.clone().into();
    let alt: SharedString = if image.spec.alt.is_empty() {
        language.format(Message::ReaderImageNumber, &[("number", &(index + 1).to_string())]).into()
    } else {
        image.spec.alt.clone().into()
    };
    let mut block = v_flex().w_full().gap_2().py_2();
    match &image.status {
        ImageStatus::Ready(image) => {
            let image = image.clone();
            let enlarged = image.clone();
            let weak = reader.clone();
            block = block
                .child(
                    div()
                        .id(("answer-image", index))
                        .w_full()
                        .h(px(320.0))
                        .cursor_pointer()
                        .child(img(image).size_full().object_fit(ObjectFit::Contain))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |reader, cx| {
                                reader.preview = Some(enlarged.clone());
                                cx.notify();
                            });
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(language.text(Message::ReaderImageEnlarge)),
                );
        },
        ImageStatus::Waiting | ImageStatus::Loading => {
            block = block.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::ReaderImageLoading)),
            );
        },
        ImageStatus::Failed(error) => {
            let weak = reader.clone();
            block = block
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(error.localized(language)),
                )
                .child(
                    Button::new(("answer-pick-image", index))
                        .label(language.text(Message::ReaderChooseImage))
                        .ghost()
                        .small()
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |reader, cx| reader.choose_image(index, cx));
                        }),
                );
        },
    }
    block
        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(alt))
        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(target))
        .into_any_element()
}

impl Render for AnswerReader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = super::super::config::ui_language(cx);
        if let Some(markdown) = self.pending_markdown.take() {
            cx.on_next_frame(window, move |reader, _, cx| {
                reader.text.update(cx, |text, cx| text.push_str(&markdown, cx));
            });
        }
        if self.raw_mode && !self.raw_initialized {
            self.raw_initialized = true;
            if let Some(source) = self.snapshot.content.source() {
                let markdown = document::literal_markdown(source);
                self.raw_text.update(cx, |text, cx| text.push_str(&markdown, cx));
            }
        }
        let provider = match self.snapshot.provider.as_str() {
            "claude" => "Claude Code",
            "codex" => "Codex",
            _ => "Agent",
        };
        let muted = cx.theme().muted_foreground;
        let has_source = self.snapshot.content.source().is_some();
        let text = if self.raw_mode { &self.raw_text } else { &self.text };
        let extensions =
            if self.raw_mode { MarkdownExtensions::default() } else { self.extensions.clone() };
        let mut root =
            v_flex()
                .size_full()
                .min_w_0()
                .relative()
                .overflow_hidden()
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::on_key_down))
                .child(
                    h_flex()
                        .h(px(32.0))
                        .flex_shrink_0()
                        .px_2()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new("reader-return")
                                .label(language.text(Message::ReaderTerminal))
                                .ghost()
                                .small()
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(ReaderEvent::Close))),
                        )
                        .child(div().text_sm().flex_1().min_w_0().truncate().child(
                            language.format(Message::ReaderTitle, &[("provider", provider)]),
                        ))
                        .child(
                            Button::new("reader-source")
                                .debug_selector(|| "reader-source".into())
                                .label(language.text(if self.raw_mode {
                                    Message::ReaderRead
                                } else {
                                    Message::ReaderSource
                                }))
                                .ghost()
                                .small()
                                .disabled(!has_source)
                                .on_click(cx.listener(|reader, _, _, cx| {
                                    reader.raw_mode = !reader.raw_mode;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("reader-copy")
                                .debug_selector(|| "reader-copy".into())
                                .label(language.text(Message::ReaderCopySource))
                                .ghost()
                                .small()
                                .disabled(!has_source)
                                .on_click(cx.listener(|reader, _, _, cx| {
                                    if let Some(source) = reader.snapshot.content.source() {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            source.to_string(),
                                        ));
                                    }
                                })),
                        ),
                )
                .when(self.attention, |root| {
                    root.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_sm()
                            .text_color(cx.theme().warning)
                            .child(language.text(Message::ReaderAttention)),
                    )
                })
                .when(self.newer_answer, |root| {
                    root.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_sm()
                            .text_color(muted)
                            .child(language.text(Message::ReaderNewerAnswer)),
                    )
                })
                .when_some(self.notice(language), |root, notice| {
                    root.child(div().px_3().py_2().text_sm().text_color(muted).child(notice))
                })
                .when(self.preparing, |root| {
                    root.child(
                        div()
                            .p_3()
                            .text_sm()
                            .text_color(muted)
                            .child(language.text(Message::ReaderPreparing)),
                    )
                });
        let cwd = self.snapshot.cwd.clone();
        let markdown = div().flex_1().min_h_0().min_w_0().px_3().py_2().child(
            TextView::new(text)
                .selectable(true)
                .scrollable(true)
                .markdown_extensions(extensions)
                .on_link_click(move |url, _, window, cx| {
                    if url.starts_with('#') {
                        return;
                    }
                    let language = super::super::config::ui_language(cx);
                    let url = url.to_owned();
                    let cwd = cwd.clone();
                    // Path metadata and system openers can block on mounted/network
                    // filesystems. Keep both outside the window's render thread.
                    let task = cx.background_executor().spawn(async move {
                        let result =
                            crate::file_uri::try_open_local_link_with_cwd(&url, cwd.as_deref());
                        (url, result)
                    });
                    window
                        .spawn(cx, async move |cx| {
                            let (url, result) = task.await;
                            let _ = cx.update(|window, cx| {
                                let message = match result {
                                    Some(Ok(())) => return,
                                    Some(Err(error)) => error.localized_message(language),
                                    None if crate::file_uri::is_web_or_protocol_uri(&url) => {
                                        cx.open_url(&url);
                                        return;
                                    },
                                    None => language.format(
                                        crate::i18n::Message::CommonLinkUnrecognized,
                                        &[("target", &url)],
                                    ),
                                };
                                crate::gpui_shell::toast::toast(
                                    window,
                                    cx,
                                    crate::display::ToastKind::Warning,
                                    message,
                                );
                            });
                        })
                        .detach();
                })
                .style(TextViewStyle {
                    highlight_theme: cx.theme().highlight_theme.clone(),
                    is_dark: cx.theme().is_dark(),
                    ..TextViewStyle::default()
                }),
        );
        root = root.child(markdown);
        if let Some(image) = self.preview.clone() {
            root = root.child(
                v_flex()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(cx.theme().background)
                    .p_3()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(language.text(Message::ReaderImagePreview))
                            .child(
                                Button::new("reader-close-preview")
                                    .label(language.text(Message::ReaderClosePreview))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|reader, _, _, cx| {
                                        reader.preview = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(img(image).size_full().object_fit(ObjectFit::Contain)),
                    ),
            );
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn switching_language_and_modes_preserves_the_source_copied_by_the_button(
        cx: &mut gpui::TestAppContext,
    ) {
        let source = "# 原文 {error}\n\n$$\nx^2 + y^2\n$$\n";
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::gpui_shell::math_view::register(cx);
            cx.set_reduce_motion(true);
        });
        let mut reader = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                AnswerReader::new(
                    AnswerSnapshot {
                        provider: "codex".into(),
                        session_id: "fixture".into(),
                        received_sequence: 1,
                        content: crate::assistant_answer::AssistantAnswer::Complete(Arc::from(
                            source,
                        )),
                        cwd: None,
                    },
                    cx,
                )
            });
            reader = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let reader = reader.unwrap();
        cx.simulate_resize(gpui::size(px(420.0), px(600.0)));
        cx.run_until_parked();
        for language in [UiLanguage::EnUs, UiLanguage::ZhCn, UiLanguage::EnUs] {
            cx.update(|window, cx| {
                let mut settings =
                    crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
                settings.ui_language = language;
                cx.set_global(settings);
                reader.update(cx, |_, cx| cx.notify());
                window.refresh();
                window.draw(cx).clear(cx);
            });
            let copy = cx.debug_bounds("reader-copy").unwrap();
            assert!(copy.left() >= px(0.0) && copy.right() <= px(420.0));
            cx.update(|_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string("before copy".into()))
            });
            cx.simulate_click(copy.center(), gpui::Modifiers::default());
            cx.update(|_, cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(source))
            });
            let previous_mode = reader.read_with(cx, |reader, _| reader.raw_mode);
            let toggle = cx.debug_bounds("reader-source").unwrap();
            cx.simulate_click(toggle.center(), gpui::Modifiers::default());
            assert_ne!(reader.read_with(cx, |reader, _| reader.raw_mode), previous_mode);
            cx.update(|window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string("before copy in new mode".into()));
                window.refresh();
                window.draw(cx).clear(cx);
            });
            let copy = cx.debug_bounds("reader-copy").unwrap();
            cx.simulate_click(copy.center(), gpui::Modifiers::default());
            cx.update(|_, cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(source))
            });
            assert_eq!(
                reader.read_with(cx, |reader, _| reader
                    .snapshot
                    .content
                    .source()
                    .unwrap()
                    .to_string()),
                source
            );
        }
    }

    #[test]
    fn only_generated_image_placeholders_can_load_files() {
        let document = document::prepare("```nebula-answer-image\n0\n```\n\n![图](plot.png)");
        let starts = document.image_placeholder_starts.clone();
        let root = markdown::to_mdast(&document.markdown, &markdown::ParseOptions::gfm()).unwrap();
        let nodes = root.children().unwrap();
        assert_eq!(image_placeholder_index(&nodes[0], 0, &starts), None);
        assert!(nodes.iter().any(|node| image_placeholder_index(node, 0, &starts) == Some(0)));
        assert!(!contains_uncontrolled_media(&root));
    }

    #[test]
    fn uncontrolled_media_falls_back_before_component_can_fetch_it() {
        for source in [
            "![x](https://example.invalid/a)",
            "<img src=\"https://example.invalid/a\">",
            "|图|\n|-|\n|![x](file:///private.png)|",
        ] {
            let root = markdown::to_mdast(source, &markdown::ParseOptions::gfm()).unwrap();
            assert!(contains_uncontrolled_media(&root));
        }
    }

    #[test]
    fn omitted_image_markers_are_validated_without_expanding_the_load_budget() {
        let source = format!(
            "```nebula-answer-image\n8\n```\n\n{}",
            "![](plot.png)\n\n".repeat(document::MAX_IMAGES + 1)
        );
        let document = document::prepare(&source);
        assert_eq!(document.images.len(), document::MAX_IMAGES);
        let root = markdown::to_mdast(&document.markdown, &markdown::ParseOptions::gfm()).unwrap();
        let nodes = root.children().unwrap();
        assert_eq!(image_placeholder_index(&nodes[0], 0, &document.image_placeholder_starts), None);
        assert!(nodes.iter().any(|node| image_placeholder_index(
            node,
            0,
            &document.image_placeholder_starts
        ) == Some(document::MAX_IMAGES)));
    }
}
