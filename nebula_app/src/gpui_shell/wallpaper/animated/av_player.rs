//! macOS 视频背景：系统 AVPlayer 解码，画到 GPUI 视图正下方的原生图层。
//!
//! 不走 StreamImage：固定 GPUI 版本的 Metal 后端没有流式纹理，而 AVPlayerLayer
//! 由系统硬件解码并直接合成，无需逐帧 CPU 拷贝和上传。GPUI 窗口本身透明清屏，
//! 卡片底色按 [`super::super::underlay_composition`] 降低 alpha，使合成结果与
//! 图片背景的 `w·V + a·(1-w)·B` 一致。所有 AppKit/AVFoundation 调用都在主线程。
use super::super::{
    VisualEffects, WallpaperLayout, refresh_surface_opacity, show_media_error, show_shader_error,
    underlay_layer_opacity,
};
use crate::renderer::image::wallpaper_rect;
use gpui::{App, AsyncApp, Bounds, Pixels, Task, Window, WindowId, point, px};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSView, NSWindowOcclusionState, NSWindowOrderingMode};
use objc2_av_foundation::{
    AVLayerVideoGravityResize, AVPlayerItem, AVPlayerItemStatus, AVPlayerLayer, AVPlayerLooper,
    AVPlayerLooperStatus, AVQueuePlayer,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSURL};
use objc2_quartz_core::CATransaction;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{collections::HashMap, path::PathBuf, time::Duration};

/// 可见性与解码状态由系统异步给出；低频轮询足以响应遮挡、最小化和失败。
const MONITOR_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(in crate::gpui_shell::wallpaper) struct Animated {
    effect_config: nebula_settings::BackgroundEffects,
    source: Option<Source>,
}

struct Source {
    player: Retained<AVQueuePlayer>,
    // 循环由 looper 持有的模板项驱动；释放 looper 即停止循环。
    looper: Retained<AVPlayerLooper>,
    placements: HashMap<WindowId, Placement>,
    enabled: bool,
    ready: bool,
    failed: bool,
    _monitor: Task<()>,
}

/// 每个窗口一层：裁剪视图承担卡片圆角，播放图层按共享布局放置。
struct Placement {
    clip: Retained<NSView>,
    layer: Retained<AVPlayerLayer>,
}

impl Drop for Placement {
    fn drop(&mut self) {
        self.clip.removeFromSuperview();
    }
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
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("video background must start on the main thread"))?;
    // Missing or undecodable files surface asynchronously through the item status.
    let url = NSURL::from_file_path(&path)
        .ok_or_else(|| anyhow::anyhow!("video background path is not a file URL"))?;
    let (player, looper) = unsafe {
        let template = AVPlayerItem::playerItemWithURL(&url, mtm);
        let player = AVQueuePlayer::queuePlayerWithItems(&NSArray::new(), mtm);
        let looper = AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &template);
        player.setMuted(true);
        player.setAllowsExternalPlayback(false);
        player.setPreventsDisplaySleepDuringVideoPlayback(false);
        (player, looper)
    };
    // Dropping the source drops this task, which ends the loop.
    let monitor = cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            cx.background_executor().timer(MONITOR_INTERVAL).await;
            cx.update(tick);
        }
    });
    Ok(Source {
        player,
        looper,
        placements: HashMap::new(),
        enabled: true,
        ready: false,
        failed: false,
        _monitor: monitor,
    })
}

/// 同步解码状态、窗口存活与可见性。只在可见时播放，遮挡和最小化即暂停。
fn tick(cx: &mut App) {
    let open_windows: Vec<WindowId> =
        cx.windows().iter().map(|handle| handle.window_id()).collect();
    let reduce_motion = cx.reduce_motion();
    if !cx.has_global::<VisualEffects>() {
        return;
    }
    let Some(source) = cx.global_mut::<VisualEffects>().animated.source.as_mut() else { return };
    if source.failed {
        return;
    }
    source.placements.retain(|id, _| open_windows.contains(id));
    let item = unsafe { source.player.currentItem() };
    let failed = unsafe { source.looper.status() } == AVPlayerLooperStatus::Failed
        || item.as_ref().is_some_and(|item| unsafe { item.status() } == AVPlayerItemStatus::Failed);
    if failed {
        let error = item
            .and_then(|item| unsafe { item.error() })
            .map(|error| error.localizedDescription().to_string())
            .unwrap_or_default();
        log::warn!("video background decode failed: {error}");
        source.failed = true;
        source.placements.clear();
        unsafe { source.player.pause() };
        show_media_error(nebula_settings::BackgroundMediaKind::Video, cx);
        cx.defer(refresh_surface_opacity);
        return;
    }
    let became_ready = !source.ready
        && item.as_ref().is_some_and(|item| unsafe {
            item.status() == AVPlayerItemStatus::ReadyToPlay && item.presentationSize().width > 0.0
        });
    if became_ready {
        source.ready = true;
    }
    let visible = source.enabled
        && source.placements.values().any(|placement| {
            placement.clip.window().is_some_and(|window| {
                window.occlusionState().contains(NSWindowOcclusionState::Visible)
                    && !window.isMiniaturized()
            })
        });
    for placement in source.placements.values() {
        placement.clip.setHidden(!source.enabled);
    }
    let playing = unsafe { source.player.rate() } != 0.0;
    if visible && !reduce_motion && !playing {
        unsafe { source.player.play() };
    } else if (!visible || reduce_motion) && playing {
        unsafe { source.player.pause() };
    }
    if became_ready {
        cx.defer(refresh_surface_opacity);
        cx.refresh_windows();
    }
}

pub(in crate::gpui_shell::wallpaper) fn source_failed(cx: &App) -> bool {
    cx.global::<VisualEffects>().animated.source.as_ref().is_some_and(|source| source.failed)
}
pub(in crate::gpui_shell::wallpaper) fn shader_ready(_: &App) -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn media_ready(cx: &App) -> bool {
    underlay_ready(cx)
}
/// The video is composited below GPUI, so surfaces above it must stay translucent.
pub(in crate::gpui_shell::wallpaper) fn underlay_ready(cx: &App) -> bool {
    cx.try_global::<VisualEffects>()
        .and_then(|effects| effects.animated.source.as_ref())
        .is_some_and(|source| source.ready && !source.failed && source.enabled)
}
pub(in crate::gpui_shell::wallpaper) fn video_available() -> bool {
    true
}
pub(in crate::gpui_shell::wallpaper) fn shader_available() -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn media_available(
    kind: nebula_settings::BackgroundMediaKind,
) -> bool {
    matches!(
        kind,
        nebula_settings::BackgroundMediaKind::Image | nebula_settings::BackgroundMediaKind::Video
    )
}

/// Places this window's layer under the card for the current frame. Nothing is
/// painted into the GPUI scene; returning `true` keeps the still-image path idle.
pub(in crate::gpui_shell::wallpaper) fn paint(
    bounds: Bounds<Pixels>,
    under_chrome: bool,
    layout: WallpaperLayout,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let radius = if under_chrome { px(0.0) } else { crate::gpui_shell::theme::card_radius(cx) };
    let layer_opacity = underlay_layer_opacity(cx);
    let Some(source) = cx.global_mut::<VisualEffects>().animated.source.as_mut() else {
        return false;
    };
    if source.failed {
        return false;
    }
    let id = Window::window_handle(window).window_id();
    if !source.placements.contains_key(&id) {
        match place(&source.player, window) {
            Ok(placement) => {
                source.placements.insert(id, placement);
            },
            Err(error) => {
                log::warn!("video background placement failed: {error:#}");
                return false;
            },
        }
    }
    let Some(placement) = source.placements.get(&id) else { return false };
    let anchor = if layout.cover_chrome {
        Bounds::new(point(px(0.0), px(0.0)), window.viewport_size())
    } else {
        bounds
    };
    let video = unsafe { source.player.currentItem().map(|item| item.presentationSize()) }
        .filter(|size| size.width > 0.0 && size.height > 0.0);
    arrange(placement, bounds, anchor, video, layout, radius, layer_opacity, window);
    true
}

fn place(player: &AVQueuePlayer, window: &Window) -> anyhow::Result<Placement> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("video background must be placed on the main thread"))?;
    let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)?.as_raw() else {
        anyhow::bail!("video background requires an AppKit window");
    };
    // GPUI 的绘制视图在其窗口存活期间有效；这里只取强引用，不转移所有权。
    let gpui_view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
        .ok_or_else(|| anyhow::anyhow!("missing GPUI view"))?;
    let content = unsafe { gpui_view.superview() }
        .ok_or_else(|| anyhow::anyhow!("GPUI view has no content view"))?;
    let clip = NSView::new(mtm);
    clip.setWantsLayer(true);
    let clip_layer = clip.layer().ok_or_else(|| anyhow::anyhow!("clip view has no layer"))?;
    clip_layer.setMasksToBounds(true);
    clip_layer.setGeometryFlipped(true);
    let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(player)) };
    if let Some(gravity) = unsafe { AVLayerVideoGravityResize } {
        unsafe { layer.setVideoGravity(gravity) };
    }
    clip_layer.addSublayer(&layer);
    // 紧贴 GPUI 视图之下：位于可选的模糊材质视图之上，不遮挡任何 GPUI 内容。
    content.addSubview_positioned_relativeTo(&clip, NSWindowOrderingMode::Below, Some(&gpui_view));
    Ok(Placement { clip, layer })
}

#[allow(clippy::too_many_arguments)]
fn arrange(
    placement: &Placement,
    bounds: Bounds<Pixels>,
    anchor: Bounds<Pixels>,
    video: Option<CGSize>,
    layout: WallpaperLayout,
    radius: Pixels,
    opacity: f32,
    window: &Window,
) {
    let Some(content) = (unsafe { placement.clip.superview() }) else { return };
    let height = content.bounds().size.height;
    // GPUI 坐标原点在左上；内容视图默认原点在左下。
    let clip_frame = CGRect::new(
        CGPoint::new(
            f64::from(f32::from(bounds.origin.x)),
            height - f64::from(f32::from(bounds.origin.y + bounds.size.height)),
        ),
        CGSize::new(
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        ),
    );
    let video_frame = video.map(|video| {
        let scale = window.scale_factor().max(0.5);
        let (x, y, width, height) = wallpaper_rect(
            f32::from(anchor.size.width) * scale,
            f32::from(anchor.size.height) * scale,
            video.width as f32,
            video.height as f32,
            layout.fit,
            layout.alignment,
        );
        let origin = anchor.origin - bounds.origin;
        CGRect::new(
            CGPoint::new(
                f64::from(f32::from(origin.x) + x / scale),
                f64::from(f32::from(origin.y) + y / scale),
            ),
            CGSize::new(f64::from(width / scale), f64::from(height / scale)),
        )
    });
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    placement.clip.setFrame(clip_frame);
    if let Some(clip_layer) = placement.clip.layer() {
        clip_layer.setCornerRadius(f64::from(f32::from(radius)));
    }
    placement.layer.setHidden(video_frame.is_none());
    if let Some(frame) = video_frame {
        placement.layer.setFrame(frame);
    }
    placement.layer.setOpacity(opacity);
    CATransaction::commit();
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
