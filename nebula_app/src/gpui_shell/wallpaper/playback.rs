//! App-owned video clock, one decoder source and independently retired window textures.
use super::video;
use crate::gpui_shell::wallpaper::budgets::GlobalBudgets;
pub(super) use crate::gpui_shell::wallpaper::budgets::gpu_budget;
use gpui::{
    App, AppContext, Context, StreamImageBudget, StreamImageBudgets, StreamImageCompletion,
    StreamImageHandle, Subscription, Task, Window, WindowId,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) struct Front {
    pub frame: video::Frame,
}
struct Placement {
    native: isize,
    owner: StreamImageHandle,
    blocked: bool,
    subscriptions: Vec<Subscription>,
}

pub(super) struct Playback {
    path: PathBuf,
    kind: nebula_settings::BackgroundMediaKind,
    front: Option<Arc<Front>>,
    pending: Option<Arc<Front>>,
    cursor: Option<video::Cursor>,
    cancellation: Arc<AtomicBool>,
    producing: bool,
    timer: Option<Task<()>>,
    media_time: Duration,
    active_since: Option<Instant>,
    next_at: Duration,
    generation: u64,
    failed: bool,
    presentable: bool,
    frozen: bool,
    enabled: bool,
    ended: bool,
    placements: HashMap<WindowId, Placement>,
    cpu: StreamImageBudgets,
    jobs: StreamImageBudgets,
    gpu: StreamImageBudgets,
    ui_thread: std::thread::ThreadId,
    closed: Subscription,
}

pub(super) fn allowed(native: isize) -> bool {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, IsWindowVisible},
    };
    let hwnd = HWND(native as *mut _);
    unsafe {
        IsWindowVisible(hwnd).as_bool()
            && !IsIconic(hwnd).as_bool()
            && GetForegroundWindow() == hwnd
    }
}

impl Playback {
    pub fn new(
        path: PathBuf,
        kind: nebula_settings::BackgroundMediaKind,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.weak_entity();
        let closed = cx.on_window_closed(move |cx, id| {
            let _ = weak.update(cx, |this, cx| {
                this.placements.remove(&id);
                this.reconcile(cx);
                if this.placements.is_empty() {
                    this.cancel();
                }
            });
        });
        cx.on_release(|this, _| this.cancel()).detach();
        let gpu_global = gpu_budget(cx);
        let cpu_global = cx.global::<GlobalBudgets>().cpu.clone();
        let decoder_global = cx.global::<GlobalBudgets>().decoder.clone();
        Self {
            path,
            kind,
            front: None,
            pending: None,
            cursor: None,
            cancellation: Arc::default(),
            producing: false,
            timer: None,
            media_time: Duration::ZERO,
            active_since: None,
            next_at: Duration::ZERO,
            generation: 0,
            failed: false,
            presentable: false,
            frozen: false,
            enabled: true,
            ended: false,
            placements: HashMap::new(),
            cpu: StreamImageBudgets::new(
                StreamImageBudget::with_allocation_limit(32 * 1024 * 1024, 8),
                cpu_global.clone(),
            ),
            jobs: StreamImageBudgets::new(
                StreamImageBudget::with_allocation_limit(1, 1),
                decoder_global,
            ),
            gpu: StreamImageBudgets::new(StreamImageBudget::new(16 * 1024 * 1024), gpu_global),
            ui_thread: std::thread::current().id(),
            closed,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.enabled = enabled;
        self.reconcile(cx);
    }

    pub fn has_front(&self) -> bool {
        self.presentable && self.front.is_some()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn touch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Arc<Front>, StreamImageHandle)> {
        if self.failed && self.front.is_none() {
            return None;
        }
        let id = Window::window_handle(window).window_id();
        if !self.placements.contains_key(&id) {
            let RawWindowHandle::Win32(handle) =
                HasWindowHandle::window_handle(window).ok()?.as_raw()
            else {
                return None;
            };
            let native = handle.hwnd.get();
            let owner = window.create_stream_image(self.gpu.clone(), cx);
            let activation = cx.observe_window_activation(window, |this, _, cx| this.reconcile(cx));
            let bounds = cx.observe_window_bounds(window, |this, _, cx| this.reconcile(cx));
            self.placements.insert(
                id,
                Placement {
                    native,
                    owner,
                    blocked: false,
                    subscriptions: vec![activation, bounds],
                },
            );
        }
        self.reconcile(cx);
        self.front.as_ref().map(|front| (front.clone(), self.placements[&id].owner.clone()))
    }

    fn permitted(&self, cx: &App) -> bool {
        self.enabled
            && !self.ended
            && !self.failed
            && !self.frozen
            && !cx.reduce_motion()
            && self.placements.values().any(|p| allowed(p.native))
    }
    fn capacity(&self) -> bool {
        self.placements.values().any(|p| allowed(p.native) && !p.blocked)
    }
    fn elapsed(&self) -> Duration {
        self.media_time + self.active_since.map_or(Duration::ZERO, |s| s.elapsed())
    }
    fn reconcile(&mut self, cx: &mut Context<Self>) {
        // Closing the last placement cancels the owning worker. Reopening waits
        // for its pending result to be refused before starting a fresh source.
        if !self.producing
            && !self.ended
            && !self.frozen
            && !self.failed
            && !self.placements.is_empty()
            && self.cancellation.load(Ordering::Acquire)
        {
            self.cancellation = Arc::default();
        }
        let permitted = self.permitted(cx);
        if permitted && self.active_since.is_none() {
            self.active_since = Some(Instant::now());
        }
        if !permitted && self.active_since.is_some() {
            self.media_time += self.active_since.take().unwrap().elapsed();
            self.generation = self.generation.wrapping_add(1);
            self.timer.take();
        }
        if self.failed || self.frozen || self.ended || !self.enabled || self.placements.is_empty() {
            return;
        }
        if self.front.is_none() && !self.producing {
            if let Some(front) = self.pending.take() {
                self.publish(front, cx);
            } else {
                self.prepare(cx);
            }
        } else if permitted {
            if self.pending.is_none() && !self.producing && self.capacity() {
                self.prepare(cx);
            }
            self.arm(cx);
        }
    }
    fn arm(&mut self, cx: &mut Context<Self>) {
        if !self.permitted(cx)
            || !self.capacity()
            || self.timer.is_some()
            || self.producing
            || self.pending.is_none()
        {
            return;
        }
        let wait = self.next_at.saturating_sub(self.elapsed());
        let generation = self.generation;
        self.timer = Some(cx.spawn(async move |entity, cx| {
            cx.background_executor().timer(wait).await;
            let _ = entity.update(cx, |this, cx| {
                this.timer.take();
                if generation != this.generation {
                    return;
                }
                if !this.permitted(cx) {
                    this.reconcile(cx);
                    return;
                }
                if let Some(front) = this.pending.take() {
                    this.publish(front, cx);
                }
            });
        }));
    }
    fn prepare(&mut self, cx: &mut Context<Self>) {
        if self.producing || self.failed || self.frozen || self.cancellation.load(Ordering::Acquire)
        {
            return;
        }
        self.producing = true;
        let mut cursor = self.cursor.take();
        let path = self.path.clone();
        let kind = self.kind;
        let cpu = self.cpu.clone();
        let jobs = self.jobs.clone();
        let ui_thread = self.ui_thread;
        let cancellation = self.cancellation.clone();
        let receipt_cancellation = cancellation.clone();
        let decoder = cx.global::<GlobalBudgets>().decoder.clone();
        let executor = cx.background_executor().clone();
        let task = cx.background_executor().spawn(async move {
            // A replacement must not overlap native teardown of the old source.
            // Admission belongs to the native worker until its actual exit.
            let deadline = Instant::now() + Duration::from_secs(5);
            while cursor.is_none()
                && decoder.preparations() != 0
                && !cancellation.load(Ordering::Acquire)
                && Instant::now() < deadline
            {
                executor.timer(Duration::from_millis(10)).await;
            }
            let result = (|| {
                anyhow::ensure!(!cancellation.load(Ordering::Acquire), "video request cancelled");
                if cursor.is_none() {
                    cursor = Some(match kind {
                        nebula_settings::BackgroundMediaKind::Video => {
                            video::Cursor::start(path, cpu, jobs, ui_thread, cancellation.clone())?
                        },
                        #[cfg(feature = "gif-background")]
                        nebula_settings::BackgroundMediaKind::Gif => video::Cursor::start_gif(
                            path,
                            cpu,
                            jobs,
                            ui_thread,
                            cancellation.clone(),
                        )?,
                        _ => anyhow::bail!("animated background kind is unavailable"),
                    });
                }
                cursor
                    .as_mut()
                    .unwrap()
                    .next_frame()
                    .map(|frame| frame.map(|frame| Arc::new(Front { frame })))
            })();
            (cursor, result)
        });
        cx.spawn(async move |entity, cx| {
            let (cursor, result) = task.await;
            let _ = entity.update(cx, |this, cx| {
                this.producing = false;
                if receipt_cancellation.load(Ordering::Acquire) {
                    drop(cursor);
                    this.reconcile(cx);
                    return;
                }
                this.cursor = cursor;
                match result {
                    Ok(Some(front)) => {
                        this.pending = Some(front);
                        this.reconcile(cx);
                    },
                    Ok(None) => {
                        // 有限循环 GIF 停在末帧，不把正常结束当错误，也不重新启动解码器。
                        this.ended = true;
                        this.cursor.take();
                        this.reconcile(cx);
                    },
                    Err(error) => {
                        log::warn!("video background decode failed: {error:#}");
                        this.failed = true;
                        this.timer.take();
                        this.cursor.take();
                        super::show_media_error(this.kind, cx);
                    },
                }
            });
        })
        .detach();
    }
    fn publish(&mut self, front: Arc<Front>, cx: &mut Context<Self>) {
        let first = self.front.is_none();
        self.next_at = if first {
            self.elapsed() + front.frame.delay
        } else {
            (self.next_at + front.frame.delay).max(self.elapsed())
        };
        self.front = Some(front);
        self.refresh_placements(cx);
        if self.permitted(cx) && self.capacity() {
            self.prepare(cx);
        }
    }

    pub fn mark_presentable(&mut self, cx: &mut Context<Self>) {
        if !self.presentable {
            self.presentable = true;
            if !self.frozen && cx.has_global::<super::VisualEffects>() {
                cx.global_mut::<super::VisualEffects>().animated.retired_video.take();
            }
            cx.defer(super::refresh_surface_opacity);
            self.refresh_placements(cx);
        }
    }

    pub fn wait_for_gpu(
        &mut self,
        id: WindowId,
        completion: StreamImageCompletion,
        cx: &mut Context<Self>,
    ) {
        let Some(placement) = self.placements.get_mut(&id) else {
            return;
        };
        if placement.blocked {
            return;
        }
        placement.blocked = true;
        if !self.capacity() {
            self.timer.take();
        }
        let task = cx.background_executor().spawn(async move { completion.wait() });
        cx.spawn(async move |entity, cx| {
            let result = task.await;
            let _ = entity.update(cx, |this, cx| {
                let Some(p) = this.placements.get_mut(&id) else { return };
                p.blocked = false;
                if let Err(error) = result {
                    log::warn!("video background completion failed: {error:#}");
                    this.failed = true;
                    this.cancel();
                    super::show_media_error(this.kind, cx);
                } else {
                    this.reconcile(cx);
                }
            });
        })
        .detach();
    }
    pub fn freeze(&mut self) {
        self.frozen = true;
        self.cancellation.store(true, Ordering::Release);
        self.timer.take();
        self.pending.take();
        if let Some(mut cursor) = self.cursor.take() {
            cursor.close();
        }
    }
    pub fn fail(&mut self, cx: &mut Context<Self>) {
        if !self.failed {
            self.failed = true;
            self.cancel();
            super::show_media_error(self.kind, cx);
            cx.defer(super::refresh_surface_opacity);
        }
    }
    fn cancel(&mut self) {
        self.cancellation.store(true, Ordering::Release);
        self.generation = self.generation.wrapping_add(1);
        self.active_since = None;
        self.media_time = Duration::ZERO;
        self.next_at = Duration::ZERO;
        self.timer.take();
        self.pending.take();
        self.front.take();
        self.presentable = false;
        self.ended = false;
        if let Some(mut cursor) = self.cursor.take() {
            cursor.close();
        }
        self.placements.clear();
    }
    fn refresh_placements(&self, cx: &mut Context<Self>) {
        let ids: Vec<_> = self.placements.keys().copied().collect();
        cx.defer(move |cx| {
            for window in
                cx.windows().into_iter().filter(|window| ids.contains(&window.window_id()))
            {
                let _ = window.update(cx, |_, window, _| window.refresh());
            }
        });
    }
}
