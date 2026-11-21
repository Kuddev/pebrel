//! Owns the fitted wallpaper strip for one window.

use super::{optics, *};
use gpui::Context;
use std::time::Duration;

#[derive(Clone, PartialEq)]
struct Request {
    source: Arc<RenderImage>,
    geometry: optics::StripGeometry,
    tint: [u8; 3],
}

#[derive(Default)]
pub(in crate::gpui_shell) struct WorkspaceStrip {
    request: Option<Request>,
    image: Option<Arc<RenderImage>>,
    generation: Arc<AtomicU64>,
    loading: bool,
}

impl WorkspaceStrip {
    pub(in crate::gpui_shell) fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|strip, cx| {
            strip.generation.fetch_add(1, Ordering::Release);
            strip.retire(cx);
        })
        .detach();
        Self::default()
    }

    pub(in crate::gpui_shell) fn configure(
        &mut self,
        sidebar: Option<f32>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(Pixels, Arc<RenderImage>)> {
        let request = sidebar.and_then(|width| Request::for_window(width, window, cx));
        if self.request != request {
            self.request = request;
            self.generation.fetch_add(1, Ordering::Release);
            self.retire(cx);
            self.start(cx);
        }
        self.image
            .as_ref()
            .zip(self.request.as_ref())
            .map(|(image, request)| (px(request.geometry.width), image.clone()))
    }

    fn retire(&mut self, cx: &mut App) {
        retire_image(self.image.take(), cx);
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let Some(request) = self.request.clone() else { return };
        self.loading = true;
        let generation = self.generation.clone();
        let version = generation.load(Ordering::Acquire);
        let settle = cx.background_executor().timer(Duration::from_millis(90));
        let task = cx.background_executor().spawn(async move {
            settle.await;
            let cancelled = || generation.load(Ordering::Acquire) != version;
            let dimensions = request.source.size(0);
            image_loader::prepare_strip(
                request.source.as_bytes(0)?,
                (dimensions.width.0 as u32, dimensions.height.0 as u32),
                request.geometry,
                request.tint,
                cancelled,
            )
        });
        cx.spawn(async move |this, cx| {
            let image = task.await;
            let _ = this.update(cx, |strip, cx| {
                strip.loading = false;
                if strip.generation.load(Ordering::Acquire) != version {
                    strip.start(cx);
                } else if let Some(pixels) = image {
                    strip.image = Some(Arc::new(RenderImage::new([Frame::new(pixels)])));
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl Request {
    fn for_window(sidebar: f32, window: &Window, cx: &App) -> Option<Self> {
        let skin = crate::gpui_shell::theme::resolved_skin(cx);
        let effects = cx.try_global::<VisualEffects>()?;
        let layout = effects.layout;
        let wallpaper = effects.wallpaper.as_ref()?;
        if skin.is_light
            || effects.kind.is_animated()
            || !layout.cover_chrome
            || layout.opacity <= 0.0
        {
            return None;
        }
        let source = wallpaper.image.as_ref()?.clone();
        let viewport = window.viewport_size();
        let scale = window.scale_factor().max(0.5);
        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let anchor = Bounds::new(point(px(0.0), px(0.0)), viewport);
        let rect = image_bounds(wallpaper, layout, anchor, scale);
        if rect.size.width <= px(0.0) || rect.size.height <= px(0.0) {
            return None;
        }
        let bg = skin.panel;
        let tint = [bg.b, bg.g, bg.r].map(|channel| (f32::from(channel) * 0.65).round() as u8);
        Some(Self {
            source,
            geometry: optics::StripGeometry {
                sidebar,
                width: (sidebar + optics::FEATHER).min(width),
                height,
                scale,
                image: [
                    rect.origin.x.into(),
                    rect.origin.y.into(),
                    rect.size.width.into(),
                    rect.size.height.into(),
                ],
            },
            tint,
        })
    }
}
