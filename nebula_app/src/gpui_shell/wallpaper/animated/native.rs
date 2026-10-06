//! 有原生播放后端时的动画状态和替换生命周期。
use super::super::{
    VisualEffects, WallpaperLayout, image_corners, refresh_surface_opacity, show_media_error,
    show_shader_error,
};
use crate::renderer::image::wallpaper_rect;
use gpui::{App, AppContext, Bounds, Pixels, Window, point, px, size};
#[path = "../playback.rs"]
mod playback;
#[cfg(feature = "shader-background")]
#[path = "../shader.rs"]
mod shader;
use crate::platform::background_media as video;

#[derive(Default)]
pub(in crate::gpui_shell::wallpaper) struct Animated {
    effect_config: nebula_settings::BackgroundEffects,
    #[cfg(feature = "shader-background")]
    shader: Option<gpui::Entity<shader::Shader>>,
    video: Option<gpui::Entity<playback::Playback>>,
    retired_video: Option<gpui::Entity<playback::Playback>>,
}
pub(in crate::gpui_shell::wallpaper) fn source_failed(cx: &App) -> bool {
    cx.global::<VisualEffects>()
        .animated
        .video
        .as_ref()
        .is_some_and(|actor| actor.read(cx).failed())
}
pub(in crate::gpui_shell::wallpaper) fn configure(
    rt: &nebula_settings::RuntimeSettings,
    source_changed: bool,
    cx: &mut App,
) {
    refresh_shader(rt, cx);
    if rt.background_media_kind.is_animated() {
        if source_changed {
            restart_media(rt.background_media_kind, cx);
        }
        if let Some(actor) = cx.global::<VisualEffects>().animated.video.clone() {
            let enabled = rt.background_image_opacity > 0.0 && !shader_ready(cx);
            actor.update(cx, |state, cx| state.set_enabled(enabled, cx));
        }
    } else {
        let effects = cx.global_mut::<VisualEffects>();
        effects.animated.video.take();
        effects.animated.retired_video.take();
    }
}

fn restart_media(kind: nebula_settings::BackgroundMediaKind, cx: &mut App) {
    let old = cx.global_mut::<VisualEffects>().animated.video.take();
    if let Some(old) = old {
        let ready = old.read(cx).has_front();
        old.update(cx, |state, _| state.freeze());
        if ready {
            cx.global_mut::<VisualEffects>().animated.retired_video = Some(old);
        }
    }
    let path = cx.global::<VisualEffects>().wallpaper.as_ref().map(|wp| wp.path.clone());
    if path.is_none() {
        cx.global_mut::<VisualEffects>().animated.retired_video.take();
    }
    let next = path.map(|path| cx.new(|cx| playback::Playback::new(path, kind, cx)));
    cx.global_mut::<VisualEffects>().animated.video = next;
}

pub(in crate::gpui_shell::wallpaper) fn reload_media(cx: &mut App) {
    #[cfg(feature = "video-background")]
    if let Some(kind) = cx.try_global::<VisualEffects>().map(|effects| effects.kind)
        && kind.is_animated()
        && media_available(kind)
    {
        restart_media(kind, cx);
        cx.refresh_windows();
    }
}

pub(in crate::gpui_shell::wallpaper) fn reload_shader(cx: &mut App) {
    #[cfg(feature = "shader-background")]
    if let Some(actor) =
        cx.try_global::<VisualEffects>().and_then(|effects| effects.animated.shader.clone())
    {
        actor.update(cx, |state, cx| state.reload(cx));
    }
    #[cfg(not(feature = "shader-background"))]
    show_shader_error(cx);
}

fn refresh_shader(rt: &nebula_settings::RuntimeSettings, cx: &mut App) {
    #[cfg(not(feature = "shader-background"))]
    let changed = cx.global::<VisualEffects>().animated.effect_config != rt.background_effects;
    cx.global_mut::<VisualEffects>().animated.effect_config = rt.background_effects.clone();
    #[cfg(feature = "shader-background")]
    {
        if rt.background_effects.preset() == "off" {
            cx.global_mut::<VisualEffects>().animated.shader.take();
        } else {
            let actor = cx.global::<VisualEffects>().animated.shader.clone().unwrap_or_else(|| {
                cx.new(|cx| shader::Shader::new(rt.background_effects.clone(), cx))
            });
            actor.update(cx, |state, cx| {
                state.configure(
                    rt.background_effects.clone(),
                    rt.background_image_opacity > 0.0,
                    cx,
                )
            });
            cx.global_mut::<VisualEffects>().animated.shader = Some(actor);
        }
    }
    #[cfg(not(feature = "shader-background"))]
    if changed && rt.background_effects.preset() != "off" {
        show_shader_error(cx);
    }
}

pub(in crate::gpui_shell::wallpaper) fn shader_ready(cx: &App) -> bool {
    #[cfg(feature = "shader-background")]
    {
        return cx
            .try_global::<VisualEffects>()
            .and_then(|effects| effects.animated.shader.as_ref())
            .is_some_and(|actor| actor.read(cx).has_front());
    }
    #[cfg(not(feature = "shader-background"))]
    {
        let _ = cx;
        false
    }
}

#[cfg(feature = "shader-background")]
pub(in crate::gpui_shell::wallpaper) fn refresh_video_visibility(cx: &mut App) {
    let Some(effects) = cx.try_global::<VisualEffects>() else { return };
    let enabled = effects.layout.opacity > 0.0 && !shader_ready(cx);
    if let Some(actor) = effects.animated.video.clone() {
        actor.update(cx, |state, cx| state.set_enabled(enabled, cx));
    }
}

#[cfg(feature = "shader-background")]
fn paint_shader(
    bounds: Bounds<Pixels>,
    under_chrome: bool,
    layout: WallpaperLayout,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(actor) = cx.global::<VisualEffects>().animated.shader.clone() else { return false };
    let Some(frame) = actor.update(cx, |state, cx| state.touch(window, cx)) else { return false };
    let native = frame.native();
    let anchor = if layout.cover_chrome {
        Bounds::new(point(px(0.0), px(0.0)), window.viewport_size())
    } else {
        bounds
    };
    let scale = window.scale_factor().max(0.5);
    let (x, y, width, height) = wallpaper_rect(
        f32::from(anchor.size.width) * scale,
        f32::from(anchor.size.height) * scale,
        native.size.width.0 as f32,
        native.size.height.0 as f32,
        layout.fit,
        layout.alignment,
    );
    let target = Bounds::new(
        anchor.origin + point(px(x / scale), px(y / scale)),
        size(px(width / scale), px(height / scale)),
    );
    let radius = if under_chrome { px(0.0) } else { crate::gpui_shell::theme::card_radius(cx) };
    match window.paint_background_shader_image(
        bounds,
        target,
        image_corners(bounds, target, radius),
        &frame.owner,
        &native,
    ) {
        Ok(update) => {
            let ready = update.tile.is_some();
            if ready {
                actor.update(cx, |state, cx| state.mark_presentable(cx));
            }
            if let Some(completion) = update.completion {
                let id = window.window_handle().window_id();
                actor.update(cx, |state, cx| state.wait_for_gpu(id, completion, cx));
            }
            ready
        },
        Err(error) => {
            actor.update(cx, |state, cx| state.fail(&error, cx));
            false
        },
    }
}

pub(in crate::gpui_shell::wallpaper) fn paint(
    bounds: Bounds<Pixels>,
    under_chrome: bool,
    layout: WallpaperLayout,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    #[cfg(feature = "shader-background")]
    if paint_shader(bounds, under_chrome, layout, window, cx) {
        return true;
    }
    let effects = cx.global::<VisualEffects>();
    if effects.wallpaper.is_none() {
        return false;
    }
    if effects.kind.is_animated() {
        let current = effects.animated.video.clone();
        let fallback = effects.animated.retired_video.clone();
        let fit = layout.fit;
        let alignment = layout.alignment;
        let extended = layout.cover_chrome;
        let anchor = if extended {
            Bounds::new(point(px(0.0), px(0.0)), window.viewport_size())
        } else {
            bounds
        };
        let radius = if under_chrome { px(0.0) } else { crate::gpui_shell::theme::card_radius(cx) };
        let ready = current
            .and_then(|actor| {
                actor.update(cx, |state, cx| state.touch(window, cx)).map(|data| (actor, data))
            })
            .or_else(|| {
                fallback.and_then(|actor| {
                    actor.update(cx, |state, cx| state.touch(window, cx)).map(|data| (actor, data))
                })
            });
        if let Some((actor, (front, owner))) = ready {
            let scale = window.scale_factor().max(0.5);
            let (x, y, w, h) = wallpaper_rect(
                f32::from(anchor.size.width) * scale,
                f32::from(anchor.size.height) * scale,
                front.frame.width as f32,
                front.frame.height as f32,
                fit,
                alignment,
            );
            let image_bounds = Bounds::new(
                anchor.origin + point(px(x / scale), px(y / scale)),
                size(px(w / scale), px(h / scale)),
            );
            let frame = gpui::StreamImageFrame {
                sequence: front.frame.sequence,
                size: size(
                    gpui::DevicePixels(front.frame.width as i32),
                    gpui::DevicePixels(front.frame.height as i32),
                ),
                row_stride: front.frame.width as usize * 4,
                pixels: &front.frame.pixels,
            };
            match window.paint_stream_image(
                bounds,
                image_bounds,
                image_corners(bounds, image_bounds, radius),
                &owner,
                &frame,
                false,
            ) {
                Ok(update) => {
                    if update.tile.is_some() {
                        actor.update(cx, |state, cx| state.mark_presentable(cx));
                    }
                    if let Some(completion) = update.completion {
                        let id = window.window_handle().window_id();
                        actor.update(cx, |state, cx| state.wait_for_gpu(id, completion, cx));
                    }
                },
                Err(error) => {
                    log::warn!("video background paint failed: {error:#}");
                    actor.update(cx, |state, cx| state.fail(cx));
                },
            }
        }
        return true;
    }
    false
}

pub(in crate::gpui_shell::wallpaper) fn media_ready(cx: &App) -> bool {
    let effects = cx.global::<VisualEffects>();
    effects.animated.video.as_ref().is_some_and(|v| v.read(cx).has_front())
        || effects.animated.retired_video.as_ref().is_some_and(|v| v.read(cx).has_front())
}

pub(in crate::gpui_shell::wallpaper) fn video_available() -> bool {
    true
}

pub(in crate::gpui_shell::wallpaper) fn shader_available() -> bool {
    cfg!(feature = "shader-background")
}

pub(in crate::gpui_shell::wallpaper) fn media_available(
    kind: nebula_settings::BackgroundMediaKind,
) -> bool {
    match kind {
        nebula_settings::BackgroundMediaKind::Image
        | nebula_settings::BackgroundMediaKind::Video => true,
        nebula_settings::BackgroundMediaKind::Gif => cfg!(feature = "gif-background"),
    }
}
