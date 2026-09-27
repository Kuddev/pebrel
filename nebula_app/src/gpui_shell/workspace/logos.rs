//! DPI-specific sidebar textures, shared by tabs and pane headers.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Context, RenderImage, Task};
use image::Frame;

use super::NebulaWorkspace;

type LogoImages = HashMap<(crate::display::AiLogo, bool), Arc<RenderImage>>;

#[derive(Default)]
pub(super) struct LogoLoad {
    target: u32,
    pending: Option<PendingLoad>,
    ready: Option<LogoImages>,
}

struct PendingLoad {
    cancelled: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for PendingLoad {
    fn drop(&mut self) {
        // A synchronous decode already running on a worker cannot be preempted;
        // stop between images, and drop the foreground task that would publish it.
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl LogoLoad {
    fn reset(&mut self, target: u32) {
        // Reset and publication run on the foreground thread. Dropping the sole
        // publishing task prevents an old worker result from reaching this load.
        self.pending = None;
        self.ready = None;
        self.target = target;
    }
}

/// Render consumes already prepared pixels; it never decodes or waits for a worker.
pub(super) fn poll_sidebar_logo_images(
    workspace: &mut NebulaWorkspace,
    target: u32,
    cx: &mut Context<NebulaWorkspace>,
) -> Option<LogoImages> {
    let load = &mut workspace.sidebar_logo_load;
    if target == workspace.sidebar_logo_target_px {
        if load.target != target {
            load.reset(target);
        }
        return None;
    }
    if load.target != target {
        load.reset(target);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let worker = cx
            .background_executor()
            .spawn(async move { sidebar_logo_images(target, &worker_cancelled) });
        let task = cx.spawn(async move |workspace, cx| {
            let Some(images) = worker.await else { return };
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.sidebar_logo_load.ready = Some(images);
                cx.notify();
            });
        });
        load.pending = Some(PendingLoad { cancelled, _task: task });
    }
    load.ready.take()
}

fn decode_sidebar_logo(
    logo: crate::display::AiLogo,
    dark: bool,
    target_size: u32,
) -> Option<Arc<RenderImage>> {
    let mut rgba = image::load_from_memory(logo.png(dark)).ok()?.into_rgba8();
    logo.tint_pixels(&mut rgba, if dark { [236, 239, 245] } else { [35, 40, 50] });
    // 直接复用旧壳的 Lanczos3 物理像素预缩放与 alpha 质量中心校正。
    // 先 tint 再缩放，避免 1024px 原图在 GPUI paint 阶段临时压到十几个
    // 逻辑像素时产生灰边、锯齿与非整数 DPI 采样。
    let (prepared, width, height) = crate::display::prepare_ai_logo_texture(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
        target_size,
    );
    let mut rgba = image::RgbaImage::from_raw(width, height, prepared)?;
    // GPUI 的原始帧使用 BGRA；与壁纸解码走同一通道转换。
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([Frame::new(rgba)])))
}

fn sidebar_logo_images(target_size: u32, cancelled: &AtomicBool) -> Option<LogoImages> {
    use crate::display::AiLogo;

    let mut images = HashMap::new();
    for logo in AiLogo::ALL {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if logo.shares_theme_texture() {
            if let Some(image) = decode_sidebar_logo(logo, false, target_size) {
                images.insert((logo, false), image.clone());
                images.insert((logo, true), image);
            }
            continue;
        }
        for dark in [false, true] {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(image) = decode_sidebar_logo(logo, dark, target_size) {
                images.insert((logo, dark), image);
            }
        }
    }
    Some(images)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::AiLogo;

    #[test]
    fn sidebar_logos_share_only_identical_theme_pixels_at_each_dpi() {
        // 100% and 150% DPI keep separate physical textures. There is no global cache.
        for target in [15, 23] {
            let images = sidebar_logo_images(target, &AtomicBool::new(false)).unwrap();
            assert_eq!(images.len(), AiLogo::ALL.len() * 2);
            let unique = images.values().map(Arc::as_ptr).collect::<std::collections::HashSet<_>>();
            assert_eq!(unique.len(), 15, "five color assets must not be prepared twice");
            for logo in AiLogo::ALL {
                let light = &images[&(logo, false)];
                let dark = &images[&(logo, true)];
                assert_eq!(u32::from(light.size(0).width), target);
                assert_eq!(u32::from(dark.size(0).height), target);
                if Arc::ptr_eq(light, dark) {
                    // Compare the omitted dark preparation against the actual output,
                    // so a later asset/tint change cannot silently change its pixels.
                    let reference = decode_sidebar_logo(logo, true, target).unwrap();
                    assert_eq!(dark.as_bytes(0), reference.as_bytes(0), "{logo:?}");
                } else {
                    assert_ne!(light.as_bytes(0), dark.as_bytes(0), "{logo:?}");
                }
            }
        }
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn replacing_load_cancels_already_queued_publication(cx: &mut gpui::TestAppContext) {
        use std::cell::Cell;
        use std::rc::Rc;

        for replace in [true, false] {
            let published = Rc::new(Cell::new(false));
            let result = published.clone();
            let (reply, received) = futures::channel::oneshot::channel::<()>();
            let worker = cx.executor().spawn(async move { received.await.unwrap() });
            let task = cx.foreground_executor().spawn(async move {
                worker.await;
                result.set(true);
            });
            let mut load = LogoLoad::default();
            load.reset(15);
            load.pending =
                Some(PendingLoad { cancelled: Arc::new(AtomicBool::new(false)), _task: task });
            cx.run_until_parked();
            reply.send(()).unwrap();
            // Finish the worker, leaving its foreground publisher queued.
            assert!(cx.dispatcher.tick(true));
            if replace {
                load.reset(23);
                load.reset(15); // Returning to the old DPI must not revive its task.
            }
            cx.run_until_parked();
            assert_eq!(published.get(), !replace, "only the retained request may publish");
        }
    }

    #[test]
    fn releasing_or_replacing_a_load_cancels_further_image_work() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let pending = PendingLoad { cancelled: cancelled.clone(), _task: Task::ready(()) };
        drop(pending);
        assert!(cancelled.load(Ordering::Relaxed));
        assert!(sidebar_logo_images(23, &cancelled).is_none());
    }
}
