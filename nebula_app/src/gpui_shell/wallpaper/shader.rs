//! 后台编译、固定尺寸 GPU 目标和按窗口归属的播放时钟。
#[path = "shader/compile.rs"]
mod compile;
pub(super) use crate::gpui_shell::wallpaper::budgets::compiler_budget;

use gpui::{
    App, AppContext, BackgroundShaderCancellation, Context, DevicePixels, StreamImageBudget,
    StreamImageBudgets, StreamImageCompletion, StreamImageHandle, Subscription, Task, Window,
    WindowId, size,
};
use nebula_settings::BackgroundEffects;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const WIDTH: i32 = 960;
const HEIGHT: i32 = 540;
const FRAME_INTERVAL: Duration = Duration::from_millis(50);

struct Placement {
    native: isize,
    owner: StreamImageHandle,
    cancellation: BackgroundShaderCancellation,
    preparing: bool,
    ready: bool,
    blocked: bool,
    _subscriptions: Vec<Subscription>,
}

pub(super) struct Frame {
    pub owner: StreamImageHandle,
    program: Arc<compile::Program>,
    sequence: u64,
    seconds: f32,
}
impl Frame {
    pub fn native(&self) -> gpui::BackgroundShaderFrame<'_> {
        gpui::BackgroundShaderFrame {
            sequence: self.sequence,
            size: size(DevicePixels(WIDTH), DevicePixels(HEIGHT)),
            directx_bytecode: &self.program.bytecode,
            uniforms: self.program.animated.then_some([
                WIDTH as f32,
                HEIGHT as f32,
                self.seconds,
                0.0,
            ]),
        }
    }
}

pub(super) struct Shader {
    settings: BackgroundEffects,
    program: Option<Arc<compile::Program>>,
    compiling: bool,
    cancellation: Arc<AtomicBool>,
    generation: u64,
    sequence: u64,
    placements: HashMap<WindowId, Placement>,
    gpu: StreamImageBudgets,
    jobs: StreamImageBudgets,
    timer: Option<Task<()>>,
    media_time: Duration,
    active_since: Option<Instant>,
    enabled: bool,
    failed: bool,
    presentable: bool,
    _closed: Subscription,
}

impl Shader {
    pub fn new(settings: BackgroundEffects, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        let closed = cx.on_window_closed(move |cx, id| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(p) = this.placements.remove(&id) {
                    p.cancellation.cancel();
                }
                this.reconcile(cx);
            });
        });
        cx.on_release(|this, _| this.cancel()).detach();
        let jobs = compiler_budget(cx);
        let gpu = StreamImageBudgets::new(
            StreamImageBudget::new(16 * 1024 * 1024),
            super::playback::gpu_budget(cx),
        );
        Self {
            settings,
            program: None,
            compiling: false,
            cancellation: Arc::default(),
            generation: 0,
            sequence: 1,
            placements: HashMap::new(),
            gpu,
            jobs,
            timer: None,
            media_time: Duration::ZERO,
            active_since: None,
            enabled: true,
            failed: false,
            presentable: false,
            _closed: closed,
        }
    }

    pub fn configure(
        &mut self,
        settings: BackgroundEffects,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if self.settings != settings {
            self.cancel();
            self.settings = settings;
            self.program = None;
            self.cancellation = Arc::default();
            self.failed = false;
            self.presentable = false;
            self.media_time = Duration::ZERO;
            self.active_since = None;
            self.sequence = 1;
        }
        self.enabled = enabled;
        self.reconcile(cx);
    }

    fn cancel(&mut self) {
        self.cancellation.store(true, Ordering::Release);
        self.generation = self.generation.wrapping_add(1);
        self.timer.take();
        for p in self.placements.values() {
            p.cancellation.cancel();
        }
        self.placements.clear();
    }

    pub fn has_front(&self) -> bool {
        self.presentable && !self.failed
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.cancel();
        self.program = None;
        self.cancellation = Arc::default();
        self.failed = false;
        self.presentable = false;
        self.media_time = Duration::ZERO;
        self.active_since = None;
        self.sequence = 1;
        cx.defer(|cx| {
            super::refresh_video_visibility(cx);
            cx.refresh_windows();
        });
    }

    pub fn touch(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Frame> {
        if !self.enabled || self.failed {
            return None;
        }
        // 原生 HasWindowHandle 同名方法返回另一种句柄；这里必须使用 GPUI 窗口身份。
        let id = Window::window_handle(window).window_id();
        if !self.placements.contains_key(&id) {
            let RawWindowHandle::Win32(handle) =
                HasWindowHandle::window_handle(window).ok()?.as_raw()
            else {
                return None;
            };
            let activation = cx.observe_window_activation(window, |this, _, cx| this.reconcile(cx));
            let bounds = cx.observe_window_bounds(window, |this, _, cx| this.reconcile(cx));
            self.placements.insert(
                id,
                Placement {
                    native: handle.hwnd.get(),
                    owner: window.create_stream_image(self.gpu.clone(), cx),
                    cancellation: BackgroundShaderCancellation::default(),
                    preparing: false,
                    ready: false,
                    blocked: false,
                    _subscriptions: vec![activation, bounds],
                },
            );
        }
        self.compile_if_needed(cx);
        self.prepare_window(id, cx);
        self.reconcile(cx);
        let p = self.placements.get(&id)?;
        if !p.ready {
            return None;
        }
        let program = self.program.clone()?;
        let seconds = self.elapsed().as_secs_f64();
        Some(Frame {
            owner: p.owner.clone(),
            sequence: self.sequence,
            // 先对双精度时间取周期，避免长时间运行后 f32 精度造成内置动画卡顿。
            seconds: if program.periodic { (seconds % 6.0) as f32 } else { seconds as f32 },
            program,
        })
    }

    fn compile_if_needed(&mut self, cx: &mut Context<Self>) {
        if self.compiling
            || self.program.is_some()
            || self.failed
            || !self.enabled
            || self.placements.is_empty()
        {
            return;
        }
        self.compiling = true;
        let settings = self.settings.clone();
        let cancellation = self.cancellation.clone();
        let generation = self.generation;
        let jobs = self.jobs.clone();
        let executor = cx.background_executor().clone();
        let worker = cx.background_executor().spawn(async move {
            // 旧编译真实退出前持有许可证，快速切换只留下最新设置，不并行堆积编译。
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if cancellation.load(Ordering::Acquire) {
                    return Ok(None);
                }
                match jobs.reserve_preparation() {
                    Ok(_lease) => {
                        return compile::compile(&settings).map(|program| program.map(Arc::new));
                    },
                    Err(error) if Instant::now() >= deadline => return Err(error),
                    Err(_) => executor.timer(Duration::from_millis(10)).await,
                }
            }
        });
        cx.spawn(async move |entity, cx| {
            let result = worker.await;
            let _ = entity.update(cx, |this, cx| {
                this.compiling = false;
                if this.generation != generation {
                    this.compile_if_needed(cx);
                    return;
                }
                match result {
                    Ok(program) => this.program = program,
                    Err(error) => this.fail(&error, cx),
                }
                this.refresh_placements(cx);
            });
        })
        .detach();
    }

    fn prepare_window(&mut self, id: WindowId, cx: &mut Context<Self>) {
        let Some(program) = self.program.clone() else { return };
        let Some(p) = self.placements.get(&id) else { return };
        if p.ready || p.preparing {
            return;
        }
        let owner = p.owner.clone();
        let work = match owner.prepare_background_shader(
            size(DevicePixels(WIDTH), DevicePixels(HEIGHT)),
            program.bytecode.clone(),
            p.cancellation.clone(),
        ) {
            Ok(work) => work,
            Err(error) => {
                self.fail(&error, cx);
                return;
            },
        };
        self.placements.get_mut(&id).unwrap().preparing = true;
        let generation = self.generation;
        let worker = cx.background_executor().spawn(async move { work.run() });
        cx.spawn(async move |entity, cx| {
            let result = worker.await;
            let _ = entity.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                let Some(p) = this.placements.get_mut(&id) else { return };
                if p.owner.id() != owner.id() {
                    return;
                }
                p.preparing = false;
                match result {
                    Ok(Some(prepared)) => match owner.adopt_background_shader(prepared) {
                        Ok(()) => p.ready = true,
                        Err(error) => this.fail(&error, cx),
                    },
                    Ok(None) => {},
                    Err(error) => this.fail(&error, cx),
                }
                this.refresh_placements(cx);
                this.reconcile(cx);
            });
        })
        .detach();
    }

    fn elapsed(&self) -> Duration {
        self.media_time + self.active_since.map_or(Duration::ZERO, |start| start.elapsed())
    }

    fn permitted(&self, cx: &App) -> bool {
        self.enabled
            && !self.failed
            && !cx.reduce_motion()
            && self.program.as_ref().is_some_and(|program| program.animated)
            && self.placements.values().any(|p| p.ready && super::playback::allowed(p.native))
    }

    fn reconcile(&mut self, cx: &mut Context<Self>) {
        if self.permitted(cx) {
            self.active_since.get_or_insert_with(Instant::now);
        } else {
            if let Some(start) = self.active_since.take() {
                self.media_time += start.elapsed();
            }
            self.timer.take();
            return;
        }
        let capacity = self
            .placements
            .values()
            .any(|p| p.ready && !p.blocked && super::playback::allowed(p.native));
        if !capacity || self.timer.is_some() {
            return;
        }
        let generation = self.generation;
        self.timer = Some(cx.spawn(async move |entity, cx| {
            cx.background_executor().timer(FRAME_INTERVAL).await;
            let _ = entity.update(cx, |this, cx| {
                this.timer.take();
                if generation != this.generation {
                    return;
                }
                if this.permitted(cx) {
                    this.sequence = this.sequence.saturating_add(1);
                    this.refresh_placements(cx);
                }
                this.reconcile(cx);
            });
        }));
    }

    pub fn mark_presentable(&mut self, cx: &mut Context<Self>) {
        if !self.presentable {
            self.presentable = true;
            cx.defer(|cx| {
                super::refresh_video_visibility(cx);
                super::refresh_surface_opacity(cx);
            });
        }
    }

    pub fn wait_for_gpu(
        &mut self,
        id: WindowId,
        completion: StreamImageCompletion,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self.placements.get_mut(&id) else { return };
        if p.blocked {
            return;
        }
        p.blocked = true;
        self.timer.take();
        let generation = self.generation;
        let worker = cx.background_executor().spawn(async move { completion.wait() });
        cx.spawn(async move |entity, cx| {
            let result = worker.await;
            let _ = entity.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                let Some(p) = this.placements.get_mut(&id) else { return };
                p.blocked = false;
                if let Err(error) = result {
                    this.fail(&error, cx);
                } else {
                    this.refresh_placements(cx);
                    this.reconcile(cx);
                }
            });
        })
        .detach();
    }

    pub fn fail(&mut self, error: &anyhow::Error, cx: &mut Context<Self>) {
        if self.failed {
            return;
        }
        log::warn!("background shader failed: {error:#}");
        self.failed = true;
        self.presentable = false;
        self.cancel();
        let generation = self.generation;
        let weak = cx.weak_entity();
        cx.defer(move |cx| {
            let current = weak.upgrade().is_some_and(|entity| {
                let state = entity.read(cx);
                state.failed && state.generation == generation
            });
            if !current {
                return;
            }
            super::refresh_video_visibility(cx);
            super::show_shader_error(cx);
            super::refresh_surface_opacity(cx);
            cx.refresh_windows();
        });
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
