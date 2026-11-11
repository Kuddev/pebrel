//! AVFoundation 播放器与 AppKit 图层句柄，所有方法由主线程调用。
use crate::renderer::image::wallpaper_rect;
use gpui::{Bounds, Pixels, Window, WindowId};
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
use std::{collections::HashMap, path::PathBuf};

pub(crate) struct NativeVideo {
    player: Retained<AVQueuePlayer>,
    // 循环由 looper 持有的模板项驱动；释放 looper 即停止循环。
    looper: Retained<AVPlayerLooper>,
    placements: HashMap<WindowId, Placement>,
    ready: bool,
    failed: bool,
}

impl Drop for NativeVideo {
    fn drop(&mut self) {
        // 配置切换和最后一个窗口释放时立即停解码，不等待系统引用回收。
        unsafe { self.player.pause() };
        self.placements.clear();
    }
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

impl NativeVideo {
    pub(crate) fn open(path: PathBuf) -> anyhow::Result<Self> {
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
        Ok(Self { player, looper, placements: HashMap::new(), ready: false, failed: false })
    }
    pub(crate) fn available() -> bool {
        true
    }
    pub(crate) fn ready(&self) -> bool {
        self.ready && !self.failed
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed
    }
    pub(crate) fn fail(&mut self) {
        self.failed = true;
        self.placements.clear();
        unsafe { self.player.pause() };
    }
    pub(crate) fn tick(
        &mut self,
        open_windows: &[WindowId],
        enabled: bool,
        reduce_motion: bool,
    ) -> anyhow::Result<bool> {
        if self.failed {
            return Ok(false);
        }
        self.placements.retain(|id, _| open_windows.contains(id));
        let item = unsafe { self.player.currentItem() };
        let failed = unsafe { self.looper.status() } == AVPlayerLooperStatus::Failed
            || item
                .as_ref()
                .is_some_and(|item| unsafe { item.status() } == AVPlayerItemStatus::Failed);
        if failed {
            let error = item
                .and_then(|item| unsafe { item.error() })
                .map(|error| error.localizedDescription().to_string())
                .unwrap_or_default();
            log::warn!("video background decode failed: {error}");
            self.failed = true;
            self.placements.clear();
            unsafe { self.player.pause() };
            return Err(anyhow::anyhow!("video background decode failed: {error}"));
        }
        let became_ready = !self.ready
            && item.as_ref().is_some_and(|item| unsafe {
                item.status() == AVPlayerItemStatus::ReadyToPlay
                    && item.presentationSize().width > 0.0
            });
        if became_ready {
            self.ready = true;
        }
        let visible = enabled
            && self.placements.values().any(|placement| {
                placement.clip.window().is_some_and(|window| {
                    window.occlusionState().contains(NSWindowOcclusionState::Visible)
                        && !window.isMiniaturized()
                })
            });
        for placement in self.placements.values() {
            placement.clip.setHidden(!enabled);
        }
        let playing = unsafe { self.player.rate() } != 0.0;
        if visible && !reduce_motion && !playing {
            unsafe { self.player.play() };
        } else if (!visible || reduce_motion) && playing {
            unsafe { self.player.pause() };
        }
        Ok(became_ready)
    }
    pub(crate) fn paint(
        &mut self,
        layout: &super::Layout,
        window: &mut Window,
    ) -> anyhow::Result<()> {
        let id = Window::window_handle(window).window_id();
        if !self.placements.contains_key(&id) {
            self.placements.insert(id, place(&self.player, window)?);
        }
        let placement = &self.placements[&id];
        let video = unsafe { self.player.currentItem().map(|item| item.presentationSize()) }
            .filter(|size| size.width > 0.0 && size.height > 0.0);
        arrange(
            placement,
            layout.bounds,
            layout.anchor,
            video,
            layout,
            layout.radius,
            layout.opacity,
            window,
        );
        Ok(())
    }
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
    layout: &super::Layout,
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
