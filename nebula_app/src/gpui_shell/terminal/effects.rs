mod compiler;
mod frame;

use super::{colors::Palette, cursor_painter::CursorPaint, view::TerminalView};
use crate::platform::effect_activity::EffectActivity;
use gpui::{
    App, AppContext, BackgroundShaderCancellation, Bounds, Context, DevicePixels, Entity, Pixels,
    PostprocessFeedback, StreamImageBudget, StreamImageBudgets, StreamImageHandle, Subscription,
    Task, WeakEntity, WgslPostprocessDescriptor, Window, size,
};
use nebula_settings::{EffectAnimation, TerminalEffects};
use nebula_terminal::term::color::Colors;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const GPU_LIMIT: u64 = 64 * 1024 * 1024;

pub(super) struct TerminalEffect {
    view: WeakEntity<TerminalView>,
    config: TerminalEffects,
    revision: u64,
    epoch: u64,
    extent: [i32; 2],
    activity: EffectActivity,
    window: gpui::AnyWindowHandle,
    owner: Option<StreamImageHandle>,
    ready: bool,
    program: Option<Arc<compiler::Program>>,
    compiling: Option<Task<()>>,
    preparing: Option<Task<()>>,
    timer: Option<Task<()>>,
    cancelled: Arc<AtomicBool>,
    preparation_cancelled: BackgroundShaderCancellation,
    failed: bool,
    feedback: PostprocessFeedback,
    history: frame::History,
    gpu: StreamImageBudgets,
    jobs: StreamImageBudgets,
    _subscriptions: Vec<Subscription>,
}

impl TerminalEffect {
    fn new(view: WeakEntity<TerminalView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.activity = EffectActivity::capture(window);
            this.reconcile(cx);
            this.notify_view(cx);
        });
        let bounds = cx.observe_window_bounds(window, |this, window, cx| {
            this.activity = EffectActivity::capture(window);
            this.reconcile(cx);
            this.notify_view(cx);
        });
        cx.on_release(|this, _| this.cancel()).detach();
        Self {
            view,
            config: TerminalEffects::default(),
            revision: 0,
            epoch: 0,
            extent: [0; 2],
            activity: EffectActivity::capture(window),
            window: Window::window_handle(window),
            owner: None,
            ready: false,
            program: None,
            compiling: None,
            preparing: None,
            timer: None,
            cancelled: Arc::default(),
            preparation_cancelled: BackgroundShaderCancellation::default(),
            failed: false,
            feedback: PostprocessFeedback::default(),
            history: frame::History::default(),
            gpu: StreamImageBudgets::new(
                StreamImageBudget::new(GPU_LIMIT),
                crate::gpui_shell::wallpaper::effect_gpu_budget(cx),
            ),
            jobs: crate::gpui_shell::wallpaper::effect_compiler_budget(cx),
            _subscriptions: vec![activation, bounds],
        }
    }

    fn cancel(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.cancelled.store(true, Ordering::Release);
        self.preparation_cancelled.cancel();
        self.compiling.take();
        self.preparing.take();
        self.timer.take();
        self.owner.take();
        self.ready = false;
    }

    fn configure(&mut self, config: TerminalEffects, revision: u64) {
        if self.config.paths != config.paths
            || self.config.enabled != config.enabled
            || self.revision != revision
        {
            self.cancel();
            self.program = None;
            self.failed = false;
            self.cancelled = Arc::default();
            self.preparation_cancelled = BackgroundShaderCancellation::default();
            self.history = frame::History::default();
            self.feedback = PostprocessFeedback::default();
        }
        self.config = config;
        self.revision = revision;
    }

    fn fail(&mut self, error: &dyn std::fmt::Display, cx: &mut Context<Self>) {
        if self.failed {
            return;
        }
        log::warn!("terminal effect failed: {error}");
        self.cancel();
        self.failed = true;
        crate::gpui_shell::wallpaper::show_terminal_effect_error(self.window, cx);
        self.notify_view(cx);
    }

    fn notify_view(&self, cx: &mut Context<Self>) {
        if let Err(error) = self.view.update(cx, |_, cx| cx.notify()) {
            log::debug!("effect view released: {error}");
        }
    }

    fn permitted(&self, cx: &App) -> bool {
        if !self.config.enabled
            || self.failed
            || !self.ready
            || cx.reduce_motion()
            || self.config.animation == EffectAnimation::Off
        {
            return false;
        }
        let Some(view) = self.view.upgrade() else {
            return false;
        };
        let view = view.read(cx);
        if !view.effect_output_visible() {
            return false;
        }
        !self.activity.hidden()
            && (self.config.animation == EffectAnimation::Always
                || (self.activity.focused() && view.effect_pane_focused()))
    }

    fn reconcile(&mut self, cx: &mut Context<Self>) {
        if !self.activity.focused() {
            self.history.unfocus();
        }
        let visible = self.view.upgrade().is_some_and(|view| view.read(cx).effect_output_visible())
            && !self.activity.hidden();
        if !visible {
            self.preparation_cancelled.cancel();
            self.preparing.take();
            self.owner.take();
            self.ready = false;
            self.history.unfocus();
            self.preparation_cancelled = BackgroundShaderCancellation::default();
            self.timer.take();
            return;
        }
        if !self.permitted(cx) {
            self.timer.take();
        }
    }

    fn schedule_after_paint(&mut self, cx: &mut Context<Self>) {
        if !self.permitted(cx) || self.timer.is_some() {
            return;
        }
        let epoch = self.epoch;
        self.timer = Some(cx.spawn(async move |entity, cx| {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            let _ = entity.update(cx, |this, cx| {
                if this.epoch != epoch {
                    return;
                }
                if let Some(task) = this.timer.take() {
                    task.detach();
                }
                this.reconcile(cx);
                if this.permitted(cx) {
                    this.notify_view(cx);
                }
                // 只有下一次真实 paint 才续期；合成器不再绘制隐藏窗口时没有轮询链。
            });
        }));
    }

    pub(super) fn visibility_changed(&mut self, cx: &mut Context<Self>) {
        self.reconcile(cx);
    }

    fn compile(&mut self, cx: &mut Context<Self>) {
        if self.program.is_some() || self.compiling.is_some() || self.failed {
            return;
        }
        let paths: Vec<_> = match self.config.request() {
            Ok(Some(paths)) => {
                paths.iter().map(|path| nebula_settings::settings_dir().join(path)).collect()
            },
            Ok(None) => return,
            Err(error) => {
                self.fail(&error, cx);
                return;
            },
        };
        let jobs = self.jobs.clone();
        let cancelled = self.cancelled.clone();
        let epoch = self.epoch;
        let executor = cx.background_executor().clone();
        let work = executor.clone().spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                anyhow::ensure!(!cancelled.load(Ordering::Acquire), "effect compile cancelled");
                match jobs.reserve_preparation() {
                    Ok(_permit) => return compiler::load_chain(&paths).map(Arc::new),
                    Err(error) if Instant::now() >= deadline => return Err(error),
                    Err(_) => executor.timer(Duration::from_millis(10)).await,
                }
            }
        });
        self.compiling = Some(cx.spawn(async move |entity, cx| {
            let result = work.await;
            let _ = entity.update(cx, |this, cx| {
                if this.epoch != epoch {
                    return;
                }
                if let Some(task) = this.compiling.take() {
                    task.detach();
                }
                match result {
                    Ok(program) => this.program = Some(program),
                    Err(error) => this.fail(&error, cx),
                }
                this.notify_view(cx);
            });
        }));
    }

    fn prepare(&mut self, extent: [i32; 2], window: &mut Window, cx: &mut Context<Self>) {
        if extent != self.extent {
            self.preparation_cancelled.cancel();
            self.preparing.take();
            self.owner.take();
            self.ready = false;
            self.extent = extent;
            self.preparation_cancelled = BackgroundShaderCancellation::default();
        }
        let Some(program) = self.program.clone() else {
            return;
        };
        if self.ready || self.preparing.is_some() || self.failed || extent.iter().any(|v| *v <= 0) {
            return;
        }
        let descriptor = WgslPostprocessDescriptor {
            size: size(DevicePixels(extent[0]), DevicePixels(extent[1])),
            uniform_size: compiler::UNIFORM_BYTES,
            passes: program.passes.clone(),
        };
        let bytes = match descriptor.texture_bytes() {
            Ok(bytes) => bytes + compiler::UNIFORM_BYTES as u64,
            Err(error) => {
                self.fail(&error, cx);
                return;
            },
        };
        if bytes > GPU_LIMIT {
            self.fail(&"terminal effect surface exceeds its GPU budget", cx);
            return;
        }
        let owner = window.create_stream_image(self.gpu.clone(), cx);
        let cancelled = self.preparation_cancelled.clone();
        let factory = match owner.prepare_postprocess_wgsl(descriptor, cancelled.clone()) {
            Ok(factory) => factory,
            Err(error) => {
                self.fail(&error, cx);
                return;
            },
        };
        let budgets = self.gpu.clone();
        let epoch = self.epoch;
        let executor = cx.background_executor().clone();
        let work = executor.clone().spawn(async move {
            // 旧场景可能仍持有缩放前的资源；先等真实释放，不在 resize 时叠加纹理。
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                anyhow::ensure!(!cancelled.is_cancelled(), "effect preparation cancelled");
                budgets.ensure_available()?;
                match budgets.reserve(bytes) {
                    Ok(permit) => {
                        drop(permit);
                        return factory.run();
                    },
                    Err(error) if Instant::now() >= deadline => return Err(error),
                    Err(_) => executor.timer(Duration::from_millis(10)).await,
                }
            }
        });
        self.owner = Some(owner.clone());
        self.preparing = Some(cx.spawn(async move |entity, cx| {
            let result = work.await;
            let _ = entity.update(cx, |this, cx| {
                if this.epoch != epoch
                    || this.owner.as_ref().map(StreamImageHandle::id) != Some(owner.id())
                {
                    return;
                }
                if let Some(task) = this.preparing.take() {
                    task.detach();
                }
                match result {
                    Ok(Some(prepared)) => match owner.adopt_postprocess(prepared) {
                        Ok(()) => this.ready = true,
                        Err(error) => this.fail(&error, cx),
                    },
                    Ok(None) => {},
                    Err(error) => this.fail(&error, cx),
                }
                this.notify_view(cx);
                this.reconcile(cx);
            });
        }));
    }
}

pub(super) fn paint(
    view: &Entity<TerminalView>,
    bounds: Bounds<Pixels>,
    palette: &Palette,
    colors: &Colors,
    cursor: Option<&CursorPaint>,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let (config, revision) = crate::gpui_shell::wallpaper::terminal_effect_configuration(cx);
    if !config.enabled || !crate::platform::effect_activity::supported(window) {
        view.update(cx, |view, _| {
            view.effect.take();
        });
        return;
    }
    let actor = view.read(cx).effect.clone().unwrap_or_else(|| {
        let weak = view.downgrade();
        let actor = cx.new(|cx| TerminalEffect::new(weak, window, cx));
        view.update(cx, |view, _| view.effect = Some(actor.clone()));
        actor
    });
    let scale = window.scale_factor();
    let physical = |value: Pixels| (f32::from(window.pixel_snap(value)) * scale).round() as i32;
    let extent = [
        (physical(bounds.right()) - physical(bounds.left())).max(0),
        (physical(bounds.bottom()) - physical(bounds.top())).max(0),
    ];
    actor.update(cx, |this, cx| {
        this.activity = EffectActivity::capture(window);
        this.configure(config, revision);
        // 旧几何的场景可能晚到一帧；它的错误不应停用替换后的所有者。
        if this.owner.is_none() || this.extent != extent {
            this.feedback = PostprocessFeedback::default();
        }
        if let Some(error) = this.feedback.take_error() {
            this.fail(&error, cx);
        }
        this.compile(cx);
        this.prepare(extent, window, cx);
        if this.ready {
            if let Some(owner) = this.owner.clone() {
                let uniforms = this.history.encode(
                    extent,
                    Bounds::new(window.pixel_snap_point(bounds.origin), bounds.size),
                    scale,
                    cursor,
                    // WM_ACTIVATE 状态在非输入桌面上也可能为真；与动画门控共用前台事实。
                    focused && this.activity.focused(),
                    palette,
                    colors,
                );
                if let Err(error) =
                    window.paint_postprocess(bounds, &owner, uniforms, this.feedback.clone())
                {
                    this.fail(&error, cx);
                }
            }
        }
        this.reconcile(cx);
        this.schedule_after_paint(cx);
    });
}

pub(super) fn visibility_changed(view: &TerminalView, cx: &mut Context<TerminalView>) {
    if let Some(effect) = view.effect.clone() {
        // 可见性由工作区裁定；延后读取 view，避免在其更新期间重入借用。
        cx.defer(move |cx| {
            effect.update(cx, |effect, cx| effect.visibility_changed(cx));
        });
    }
}

#[cfg(all(test, not(windows), feature = "gpui-test-support"))]
mod cadence_tests {
    use super::*;

    #[gpui::test]
    fn a_paint_schedules_one_wake_and_hidden_panes_cancel_it(cx: &mut gpui::TestAppContext) {
        // 复用既有终端会话夹具，终端不挂入绘制树，明确模拟合成器停止送帧的情形。
        let (view, window, _receiver) = super::super::view::startup_tests::open(cx);
        let actor = window
            .update(|window, cx| cx.new(|cx| TerminalEffect::new(view.downgrade(), window, cx)));
        view.update(window, |view, _| view.effect = Some(actor.clone()));
        actor.update(window, |effect, cx| {
            effect.config.enabled = true;
            effect.config.animation = EffectAnimation::Always;
            // 本用例只验证调度，不构造 GPU owner，也不把此状态当作后端验收。
            effect.ready = true;
            effect.schedule_after_paint(cx);
            assert!(effect.timer.is_some());
        });
        window.run_until_parked();
        window.executor().advance_clock(Duration::from_millis(50));
        window.run_until_parked();
        assert!(actor.read_with(window, |effect, _| effect.timer.is_none()));
        window.executor().advance_clock(Duration::from_secs(2));
        window.run_until_parked();
        assert!(
            actor.read_with(window, |effect, _| effect.timer.is_none()),
            "no paint, no recurring timer"
        );

        window.deactivate_window();
        actor.update(window, |effect, cx| {
            effect.config.animation = EffectAnimation::Focused;
            effect.schedule_after_paint(cx);
            assert!(effect.timer.is_none());
            effect.config.animation = EffectAnimation::Off;
            effect.schedule_after_paint(cx);
            assert!(effect.timer.is_none());
            effect.config.animation = EffectAnimation::Always;
            effect.schedule_after_paint(cx);
            assert!(effect.timer.is_some(), "always mode still works in an inactive visible pane");
        });
        view.update(window, |view, cx| view.set_output_visible(false, cx));
        window.run_until_parked();
        assert!(actor.read_with(window, |effect, _| effect.timer.is_none() && !effect.ready));
    }
}
