//! Image tab rendering, plus the shared editable Markdown tab entry point.
//! Image zoom/pan continues to use the existing tested ImageView geometry.

mod image_source;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::ssh_sftp::document::{DocumentOperation, RemoteLocation};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Bounds, ContentMask, Context, Corners, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Render, RenderImage, ScrollWheelEvent, Styled as _, Task, Window, div, px,
};
use image_source::ImageSource;

use crate::display::image_viewer::ImageView;
use crate::gpui_shell::prelude::*;
use crate::i18n::Message;

/// 双击路由：应用内能读的开 tab（图片/文档/源码），其余交系统处理器。
/// 源码查看是 GPUI 壳新增能力，旧壳合同（`input/chrome.rs`）之上的超集。
pub fn openable_in_app(path: &Path) -> bool {
    crate::display::image_viewer::viewable_file(path)
        || crate::display::markdown_view::viewable_file(path)
        || crate::gpui_shell::code_tab::viewable_file(path)
}

pub struct ImageTabView {
    pub path: PathBuf,
    pub title: String,
    source: ImageSource,
    encoded: Option<Arc<[u8]>>,
    operation: Option<DocumentOperation>,
    generation: u64,
    loading: bool,
    load_received: u64,
    load_total: Option<u64>,
    load_task: Option<Task<()>>,
    progress_task: Option<Task<()>>,
    focus: FocusHandle,
    /// 共享几何状态机（zoom/pan/锚点/钳制）。
    geometry: ImageView,
    /// 后台解码的像素（BGRA 帧）；None = 解码中或失败。
    image: Option<Arc<RenderImage>>,
    error: Option<(Message, String)>,
    /// 上一帧查看区矩形（窗口坐标）；事件换算用。绘制不依赖它——canvas
    /// paint 拿的是当帧 bounds。
    area: Rc<RefCell<Bounds<Pixels>>>,
}

impl ImageTabView {
    pub fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        Self::new_with_source(ImageSource::Local(path), cx)
    }

    pub(crate) fn new_remote(location: RemoteLocation, cx: &mut Context<Self>) -> Self {
        Self::new_with_source(ImageSource::Remote(location), cx)
    }

    fn new_with_source(source: ImageSource, cx: &mut Context<Self>) -> Self {
        let path = source.path().to_owned();
        let geometry = ImageView::pending(path.clone());
        let title = match &source {
            ImageSource::Local(_) => geometry.title.clone(),
            ImageSource::Remote(location) => {
                format!("{} · {}", geometry.title, location.destination)
            },
        };
        let mut this = Self {
            path,
            title,
            source,
            encoded: None,
            operation: None,
            generation: 0,
            loading: false,
            load_received: 0,
            load_total: None,
            load_task: None,
            progress_task: None,
            focus: cx.focus_handle(),
            geometry,
            image: None,
            error: None,
            area: Rc::new(RefCell::new(Bounds::default())),
        };
        this.spawn_decode(cx);
        this
    }

    /// Reopening a tab reuses its source and preserves zoom/pan.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if !self.loading {
            self.spawn_decode(cx);
        }
    }

    #[cfg(all(test, feature = "gpui-test-support"))]
    pub(super) fn geometry_snapshot_for_test(&self) -> (Option<(u32, u32)>, f32, (f32, f32)) {
        self.geometry.geometry_snapshot_for_test()
    }

    fn area_tuple(&self) -> (f32, f32, f32, f32) {
        let bounds = *self.area.borrow();
        (
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let steps = event.delta.pixel_delta(px(40.0)).y.as_f32() / 40.0;
        let anchor = (f32::from(event.position.x), f32::from(event.position.y));
        if self.geometry.zoom_by(steps, anchor, self.area_tuple()) {
            cx.notify();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        let point = (f32::from(event.position.x), f32::from(event.position.y));
        if self.geometry.begin_drag(point, self.area_tuple()) {
            cx.notify();
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.geometry.dragging() {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            let _ = self.geometry.end_drag();
            return;
        }
        let point = (f32::from(event.position.x), f32::from(event.position.y));
        if self.geometry.drag_to(point, self.area_tuple()) {
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.geometry.end_drag() {
            cx.notify();
        }
    }
}

impl Focusable for ImageTabView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ImageTabView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = super::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let store = self.area.clone();
        // paint 闭包吃当帧几何：矩形按当帧 bounds 现算，没有首帧空窗。
        let geometry_snapshot = self.geometry.clone();
        let image = self.image.clone();
        let painter = gpui::canvas(
            move |bounds, _, _| {
                *store.borrow_mut() = bounds;
                bounds
            },
            move |_, bounds: Bounds<Pixels>, window, _| {
                let Some(image) = image else { return };
                let area = (
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                );
                let target = geometry_snapshot.render_rect(area);
                let target_bounds = Bounds::new(
                    gpui::point(px(target.0), px(target.1)),
                    gpui::size(px(target.2.max(1.0)), px(target.3.max(1.0))),
                );
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let _ = window.paint_image(
                        target_bounds,
                        target_bounds,
                        Corners::all(px(0.0)),
                        image,
                        0,
                        false,
                    );
                });
            },
        )
        .absolute()
        .inset_0();

        let cancelling = self.operation.as_ref().is_some_and(DocumentOperation::is_cancelled);
        let status = if cancelling {
            Some(language.text(Message::TransferCancelling).to_owned())
        } else if let Some((message, error)) = &self.error {
            Some(language.format(*message, &[("path", &self.source.display()), ("error", error)]))
        } else if self.loading {
            Some(super::widgets::file_read_progress(language, self.load_received, self.load_total))
        } else {
            None
        };

        div()
            .id("nebula-image-tab")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(painter)
            .when_some(status, |root, text| {
                root.child(
                    h_flex()
                        .absolute()
                        .top_2()
                        .left_2()
                        .right_2()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(cx.theme().background)
                        .text_sm()
                        .text_color(muted)
                        .child(div().flex_1().min_w_0().child(text))
                        .when(self.loading, |bar| {
                            bar.child(
                                Button::new("image-cancel")
                                    .ghost()
                                    .xsmall()
                                    .label(language.text(Message::TransferCancel))
                                    .disabled(cancelling)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(operation) = &this.operation {
                                            operation.cancel();
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                        .when(!self.loading, |bar| {
                            bar.child(
                                Button::new("image-retry")
                                    .ghost()
                                    .xsmall()
                                    .label(language.text(Message::EditorReload))
                                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                            )
                        }),
                )
            })
    }
}

pub use super::file_editor::{TextFileEvent as DocTabViewEvent, TextFileView as DocTabView};

#[cfg(all(test, feature = "gpui-test-support"))]
mod image_geometry_tests;
