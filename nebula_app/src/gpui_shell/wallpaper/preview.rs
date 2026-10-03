//! One bounded, draft-owned image preview. Drawing reads prepared state only.

use super::*;
use gpui::{Context, InteractiveElement as _};
use nebula_settings::{RuntimeSettings, ThemeEffects};

struct PreparedImage {
    image: Arc<RenderImage>,
    width: u32,
    height: u32,
    owned: bool,
}

pub(crate) struct ImagePreview {
    path: Option<PathBuf>,
    generation: Arc<AtomicU64>,
    loading: bool,
    image: Option<PreparedImage>,
    error: Option<image_loader::LoadError>,
    opacity: f32,
    fit: BackgroundImageFit,
    alignment: BackgroundImageAlignment,
    cover_chrome: bool,
}

impl ImagePreview {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|preview, cx| {
            preview.generation.fetch_add(1, Ordering::Release);
            preview.release_image(cx);
        })
        .detach();
        Self {
            path: None,
            generation: Arc::new(AtomicU64::new(0)),
            loading: false,
            image: None,
            error: None,
            opacity: 0.38,
            fit: BackgroundImageFit::default(),
            alignment: BackgroundImageAlignment::default(),
            cover_chrome: false,
        }
    }

    pub(crate) fn configure(
        &mut self,
        effects: &ThemeEffects,
        runtime: &RuntimeSettings,
        cx: &mut Context<Self>,
    ) {
        self.opacity = effects.background_image_opacity.unwrap_or(runtime.background_image_opacity);
        self.fit = effects
            .background_image_fit
            .as_deref()
            .or(runtime.background_image_fit.as_deref())
            .and_then(BackgroundImageFit::parse)
            .unwrap_or_default();
        self.alignment = effects
            .background_image_alignment
            .as_deref()
            .or(runtime.background_image_alignment.as_deref())
            .and_then(BackgroundImageAlignment::parse)
            .unwrap_or_default();
        self.cover_chrome =
            effects.background_image_cover_chrome.unwrap_or(runtime.background_image_cover_chrome);
        let path = effects
            .background_image
            .as_deref()
            .or(runtime.background_image.as_deref())
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from);
        if self.path != path {
            self.path = path;
            self.generation.fetch_add(1, Ordering::Release);
            self.release_image(cx);
            self.error = None;
            // Reuse an already loaded runtime wallpaper without decoding or
            // allocating another image. Its owner retains cache responsibility.
            self.image = cx
                .try_global::<VisualEffects>()
                .and_then(|effects| effects.wallpaper.as_ref())
                .filter(|wallpaper| Some(&wallpaper.path) == self.path.as_ref())
                .and_then(|wallpaper| {
                    wallpaper.image.as_ref().map(|image| PreparedImage {
                        image: image.clone(),
                        width: wallpaper.width,
                        height: wallpaper.height,
                        owned: false,
                    })
                });
            if self.image.is_none() {
                self.start_load(cx);
            }
        }
        cx.notify();
    }

    fn release_image(&mut self, cx: &mut App) {
        if let Some(image) = self.image.take().filter(|image| image.owned) {
            super::retire_image(Some(image.image), cx);
        }
    }

    fn start_load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let Some(path) = self.path.clone() else { return };
        let version = self.generation.load(Ordering::Acquire);
        let request = image_loader::Request {
            path,
            cached: None,
            generation: self.generation.clone(),
            version,
        };
        self.loading = true;
        let task =
            cx.background_executor().spawn(async move { image_loader::load_preview(request) });
        cx.spawn(async move |preview, cx| {
            let result = task.await;
            let _ = preview.update(cx, |preview, cx| {
                preview.loading = false;
                if preview.generation.load(Ordering::Acquire) != version {
                    if preview.image.is_none() {
                        preview.start_load(cx);
                    }
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Some(loaded)) => {
                        preview.image = Some(PreparedImage {
                            image: Arc::new(RenderImage::new([Frame::new(loaded.pixels)])),
                            width: loaded.layout_width,
                            height: loaded.layout_height,
                            owned: true,
                        });
                    },
                    Ok(None) => {},
                    Err(error) => preview.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn status_message(&self) -> Option<crate::i18n::Message> {
        use crate::i18n::Message;
        if self.loading && self.path.is_some() && self.image.is_none() {
            Some(Message::ThemeEditorImageLoading)
        } else {
            self.error.as_ref().map(|error| match error {
                image_loader::LoadError::TooLarge => Message::WallpaperTooLarge,
                _ => Message::WallpaperLoadFailed,
            })
        }
    }

    pub(crate) fn layer(&self, chrome_height: Pixels) -> gpui::Div {
        let Some(prepared) = self.image.as_ref() else { return div() };
        let image = prepared.image.clone();
        let width = prepared.width;
        let height = prepared.height;
        let fit = self.fit;
        let alignment = self.alignment;
        let inset = if self.cover_chrome { px(0.0) } else { chrome_height };
        div()
            .debug_selector(|| "theme-editor-wallpaper-preview".to_owned())
            .absolute()
            .inset_0()
            .opacity(self.opacity)
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let anchor = Bounds::new(
                            bounds.origin + point(px(0.0), inset),
                            size(bounds.size.width, (bounds.size.height - inset).max(px(0.0))),
                        );
                        let scale = window.scale_factor().max(0.5);
                        let (x, y, w, h) = wallpaper_rect(
                            f32::from(anchor.size.width) * scale,
                            f32::from(anchor.size.height) * scale,
                            width as f32,
                            height as f32,
                            fit,
                            alignment,
                        );
                        let image_bounds = Bounds::new(
                            anchor.origin + point(px(x / scale), px(y / scale)),
                            size(px(w / scale), px(h / scale)),
                        );
                        window.with_content_mask(Some(ContentMask { bounds: anchor }), |window| {
                            if let Err(error) = window.paint_image(
                                anchor,
                                image_bounds,
                                Corners::all(px(9.0)),
                                image.clone(),
                                0,
                                false,
                            ) {
                                log::warn!("theme preview image paint failed: {error}");
                            }
                        });
                    },
                )
                .size_full(),
            )
    }

    #[cfg(all(test, feature = "gpui-test-support"))]
    pub(crate) fn ready(&self) -> bool {
        self.image.is_some() && !self.loading
    }
}
