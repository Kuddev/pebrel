//! 原生下衬视频的 UI 生命周期；播放器与系统视图由 platform 持有。
//!
//! 不走 StreamImage：固定 GPUI 版本的 Metal 后端没有流式纹理，而 AVPlayerLayer
//! 由系统硬件解码并直接合成，无需逐帧 CPU 拷贝和上传。GPUI 窗口本身透明清屏，
//! 卡片底色按 [`super::super::underlay_composition`] 降低 alpha，使合成结果与
//! 图片背景的 `w·V + a·(1-w)·B` 一致。所有 AppKit/AVFoundation 调用都在主线程。
use super::super::{
    VisualEffects, WallpaperLayout, refresh_surface_opacity, show_media_error, show_shader_error,
    underlay_layer_opacity,
};
use crate::platform::video_underlay::{Layout, NativeVideo};
use gpui::{App, AsyncApp, Bounds, Pixels, Task, Window, point, px};
use std::{path::PathBuf, time::Duration};

const MONITOR_INTERVAL: Duration = Duration::from_millis(500);
#[derive(Default)]
pub(in crate::gpui_shell::wallpaper) struct Animated {
    effect_config: nebula_settings::BackgroundEffects,
    source: Option<Source>,
}
struct Source {
    video: NativeVideo,
    enabled: bool,
    _monitor: Task<()>,
}

pub(in crate::gpui_shell::wallpaper) fn configure(
    rt: &nebula_settings::RuntimeSettings,
    source_changed: bool,
    cx: &mut App,
) {
    let state = &mut cx.global_mut::<VisualEffects>().animated;
    let effects_changed = state.effect_config != rt.background_effects;
    state.effect_config = rt.background_effects.clone();
    if effects_changed && rt.background_effects.preset() != "off" {
        show_shader_error(cx);
    }
    if rt.background_media_kind != nebula_settings::BackgroundMediaKind::Video {
        if cx.global_mut::<VisualEffects>().animated.source.take().is_some() {
            cx.defer(refresh_surface_opacity);
        }
        if source_changed && rt.background_media_kind.is_animated() {
            show_media_error(rt.background_media_kind, cx);
        }
        return;
    }
    if source_changed {
        restart(cx);
    }
    let enabled = rt.background_image_opacity > 0.0;
    if let Some(source) = cx.global_mut::<VisualEffects>().animated.source.as_mut() {
        source.enabled = enabled;
    }
    tick(cx);
}

fn restart(cx: &mut App) {
    let path = cx.global::<VisualEffects>().wallpaper.as_ref().map(|wp| wp.path.clone());
    let had_source = cx.global_mut::<VisualEffects>().animated.source.take().is_some();
    let source = path.and_then(|path| match open(path, cx) {
        Ok(source) => Some(source),
        Err(error) => {
            log::warn!("video background open failed: {error:#}");
            show_media_error(nebula_settings::BackgroundMediaKind::Video, cx);
            None
        },
    });
    cx.global_mut::<VisualEffects>().animated.source = source;
    if had_source {
        cx.defer(refresh_surface_opacity);
    }
}

fn open(path: PathBuf, cx: &mut App) -> anyhow::Result<Source> {
    let video = NativeVideo::open(path)?;
    let monitor = cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            cx.background_executor().timer(MONITOR_INTERVAL).await;
            let finished = cx.update(|cx| {
                tick(cx);
                cx.try_global::<VisualEffects>()
                    .and_then(|effects| effects.animated.source.as_ref())
                    .is_none_or(|source| source.video.failed())
            });
            if finished {
                break;
            }
        }
    });
    Ok(Source { video, enabled: true, _monitor: monitor })
}

fn tick(cx: &mut App) {
    let windows = cx.windows().iter().map(|handle| handle.window_id()).collect::<Vec<_>>();
    let reduce_motion = cx.reduce_motion();
    if !cx.has_global::<VisualEffects>() {
        return;
    }
    let Some(source) = cx.global_mut::<VisualEffects>().animated.source.as_mut() else { return };
    match source.video.tick(&windows, source.enabled, reduce_motion) {
        Ok(true) => {
            cx.defer(refresh_surface_opacity);
            cx.refresh_windows();
        },
        Ok(false) => {},
        Err(error) => {
            log::warn!("video background failed: {error:#}");
            source.video.fail();
            show_media_error(nebula_settings::BackgroundMediaKind::Video, cx);
            cx.defer(refresh_surface_opacity);
        },
    }
}

pub(in crate::gpui_shell::wallpaper) fn source_failed(cx: &App) -> bool {
    cx.global::<VisualEffects>().animated.source.as_ref().is_some_and(|s| s.video.failed())
}
pub(in crate::gpui_shell::wallpaper) fn shader_ready(_: &App) -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn media_ready(cx: &App) -> bool {
    underlay_ready(cx)
}
pub(in crate::gpui_shell::wallpaper) fn underlay_ready(cx: &App) -> bool {
    cx.try_global::<VisualEffects>()
        .and_then(|e| e.animated.source.as_ref())
        .is_some_and(|s| s.enabled && s.video.ready())
}
pub(in crate::gpui_shell::wallpaper) fn video_available() -> bool {
    NativeVideo::available()
}
pub(in crate::gpui_shell::wallpaper) fn shader_available() -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn media_available(
    kind: nebula_settings::BackgroundMediaKind,
) -> bool {
    kind == nebula_settings::BackgroundMediaKind::Image
        || (kind == nebula_settings::BackgroundMediaKind::Video && video_available())
}
pub(in crate::gpui_shell::wallpaper) fn paint(
    bounds: Bounds<Pixels>,
    under_chrome: bool,
    layout: WallpaperLayout,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let radius = if under_chrome { px(0.0) } else { crate::gpui_shell::theme::card_radius(cx) };
    let opacity = underlay_layer_opacity(cx);
    let Some(source) = cx.global_mut::<VisualEffects>().animated.source.as_mut() else {
        return false;
    };
    if source.video.failed() {
        return false;
    }
    let anchor = if layout.cover_chrome {
        Bounds::new(point(px(0.0), px(0.0)), window.viewport_size())
    } else {
        bounds
    };
    let layout =
        Layout { bounds, anchor, radius, opacity, fit: layout.fit, alignment: layout.alignment };
    if let Err(error) = source.video.paint(&layout, window) {
        log::warn!("video background placement failed: {error:#}");
        source.video.fail();
        cx.defer(|cx| {
            show_media_error(nebula_settings::BackgroundMediaKind::Video, cx);
            refresh_surface_opacity(cx);
            cx.refresh_windows();
        });
        return false;
    }
    true
}
pub(in crate::gpui_shell::wallpaper) fn reload_media(cx: &mut App) {
    if cx
        .try_global::<VisualEffects>()
        .is_some_and(|effects| effects.kind == nebula_settings::BackgroundMediaKind::Video)
    {
        restart(cx);
        cx.refresh_windows();
    }
}
pub(in crate::gpui_shell::wallpaper) fn reload_shader(cx: &mut App) {
    show_shader_error(cx);
}
