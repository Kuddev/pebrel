//! Native generated-frame qualification. No GIF/video/shader capability claim.
mod gif_stream;
mod metrics;
mod native_gate;

use gpui::{
    AppContext, Bounds, Context, Corners, Entity, IntoElement, ParentElement, Render, RenderImage,
    Styled, Subscription, Task, Window, WindowBounds, WindowOptions, div, point, px, rgb, size,
};
use image::{Frame, RgbaImage};
use metrics::Metrics;
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
struct Options {
    dynamic: bool,
    interactive: bool,
    seconds: u64,
    fps: u32,
    width: u32,
    height: u32,
    windows: usize,
    placements: usize,
    output: PathBuf,
    gif: Option<PathBuf>,
    worker_ms: u64,
    reduced_motion: bool,
}

impl Options {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let value = |key: &str, fallback: &str| {
            args.windows(2)
                .find(|pair| pair[0] == key)
                .map(|pair| pair[1].clone())
                .unwrap_or_else(|| fallback.to_owned())
        };
        let options = Self {
            dynamic: value("--mode", "static") == "dynamic",
            interactive: value("--interactive", "false").parse().expect("interactive"),
            seconds: value("--seconds", "10").parse().expect("seconds"),
            fps: value("--fps", "30").parse().expect("fps"),
            width: value("--width", "1280").parse().expect("width"),
            height: value("--height", "720").parse().expect("height"),
            windows: value("--windows", "1").parse().expect("windows"),
            placements: value("--placements", "1").parse().expect("placements"),
            output: value("--output", "media-probe.json").into(),
            gif: args.windows(2).find(|pair| pair[0] == "--gif").map(|pair| pair[1].clone().into()),
            worker_ms: value("--worker-ms", "0").parse().expect("worker-ms"),
            reduced_motion: value("--reduced-motion", "false").parse().expect("reduced-motion"),
        };
        assert!((1..=60).contains(&options.fps));
        assert!((1..=4).contains(&options.windows));
        assert!((1..=8).contains(&options.placements));
        assert!(options.worker_ms <= 1000);
        assert!(options.width > 0 && options.height > 0);
        assert!(
            u64::from(options.width) * u64::from(options.height) * 4 <= 4 * 1024 * 1024,
            "prototype output admission is at most 4 MiB; large inputs are a separate decoder test"
        );
        options
    }
}

struct Prepared {
    pixels: RgbaImage,
    delay: Duration,
    sequence: u64,
}

/// Source/session ownership is shared; each native window has its own atlas.
struct Session {
    options: Options,
    metrics: Rc<RefCell<Metrics>>,
    front: Option<Arc<RenderImage>>,
    placements: BTreeMap<usize, (bool, Option<native_gate::NativeWindow>)>,
    enabled: bool,
    closing: bool,
    generation: u64,
    producing: bool,
    timer: Option<Task<()>>,
    media_time: Duration,
    active_since: Option<Instant>,
    sequence: u64,
    started: Instant,
    source_epoch: u64,
    cursor: Option<gif_stream::Cursor>,
    pending: Option<Prepared>,
    next_frame_at: Duration,
    failure: Option<String>,
    at_eos: bool,
    reduced_motion: bool,
}

impl Session {
    fn new(options: Options, metrics: Rc<RefCell<Metrics>>, cx: &mut Context<Self>) -> Self {
        cx.on_release(|session, cx| {
            session.generation = session.generation.wrapping_add(1);
            session.timer.take();
            if let Some(front) = session.front.take() {
                cx.drop_image(front, None);
            }
        })
        .detach();
        Self {
            enabled: options.dynamic,
            closing: false,
            options,
            metrics,
            front: None,
            placements: BTreeMap::new(),
            generation: 0,
            producing: false,
            timer: None,
            media_time: Duration::ZERO,
            active_since: None,
            sequence: 0,
            started: Instant::now(),
            source_epoch: 0,
            cursor: None,
            pending: None,
            next_frame_at: Duration::ZERO,
            failure: None,
            at_eos: false,
            reduced_motion: cx.reduce_motion(),
        }
    }

    fn permitted(&self) -> bool {
        !self.closing
            && self.enabled
            && self.failure.is_none()
            && !self.at_eos
            && !self.reduced_motion
            && self
                .placements
                .values()
                .any(|(active, handle)| native_gate::allowed(*handle, *active))
    }

    fn elapsed_media(&self) -> Duration {
        self.media_time + self.active_since.map_or(Duration::ZERO, |start| start.elapsed())
    }

    fn update_gate(&mut self) -> bool {
        let allowed = self.permitted();
        if allowed && self.active_since.is_none() {
            self.active_since = Some(Instant::now());
            let mut metrics = self.metrics.borrow_mut();
            metrics.resume_events += 1;
            metrics.transition(true, self.started.elapsed(), self.elapsed_media(), self.generation);
        } else if !allowed && self.active_since.is_some() {
            self.media_time += self.active_since.take().unwrap().elapsed();
            self.generation = self.generation.wrapping_add(1);
            self.timer.take();
            let mut metrics = self.metrics.borrow_mut();
            metrics.pause_events += 1;
            metrics.transition(
                false,
                self.started.elapsed(),
                self.elapsed_media(),
                self.generation,
            );
        }
        allowed
    }

    fn reconcile(&mut self, cx: &mut Context<Self>) {
        self.reduced_motion = cx.reduce_motion();
        let was_playing = self.active_since.is_some();
        let allowed = self.update_gate();
        if !self.closing
            && self.failure.is_none()
            && self.front.is_none()
            && !self.producing
            && !self.placements.is_empty()
        {
            if let Some(frame) = self.pending.take() {
                self.publish(frame, cx);
            } else if !self.at_eos {
                self.prepare(cx);
            }
        } else if allowed {
            if self.options.gif.is_some() && self.pending.is_none() && !self.producing {
                self.prepare(cx);
            }
            self.arm(cx);
        }
        if was_playing != self.active_since.is_some() {
            cx.notify();
        }
    }

    fn arm(&mut self, cx: &mut Context<Self>) {
        if !self.permitted() || self.timer.is_some() || self.producing {
            return;
        }
        let version = self.generation;
        let period = Duration::from_nanos(1_000_000_000 / u64::from(self.options.fps));
        let now = self.elapsed_media();
        let wait = if self.options.gif.is_some() {
            if self.pending.is_none() {
                return;
            }
            self.next_frame_at.saturating_sub(now)
        } else {
            let next = period.as_nanos() * (now.as_nanos() / period.as_nanos() + 1);
            Duration::from_nanos((next - now.as_nanos()) as u64)
        };
        self.metrics.borrow_mut().timer_requests += 1;
        self.timer = Some(cx.spawn(async move |session, cx| {
            cx.background_executor().timer(wait).await;
            let _ = session.update(cx, |session, cx| {
                session.timer.take();
                session.reduced_motion = cx.reduce_motion();
                if version != session.generation {
                    return;
                }
                if !session.permitted() {
                    session.reconcile(cx);
                    return;
                }
                session.metrics.borrow_mut().timer_fires += 1;
                if session.options.gif.is_some() {
                    if let Some(frame) = session.pending.take() {
                        session.publish(frame, cx);
                    }
                } else {
                    session.prepare(cx);
                }
            });
        }));
    }

    fn prepare(&mut self, cx: &mut Context<Self>) {
        if self.producing {
            return;
        }
        self.producing = true;
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.producer_started += 1;
            metrics.producers_live += 1;
            metrics.producers_peak = metrics.producers_peak.max(metrics.producers_live);
        }
        let version = self.generation;
        let sequence = self.sequence + 1;
        let width = self.options.width;
        let height = self.options.height;
        let source_epoch = self.source_epoch;
        let path = self.options.gif.clone();
        let mut cursor = self.cursor.take();
        let worker_ms = self.options.worker_ms;
        let executor = cx.background_executor().clone();
        let task = cx.background_executor().spawn(async move {
            if worker_ms != 0 {
                executor.timer(Duration::from_millis(worker_ms)).await;
            }
            let start = Instant::now();
            let output = if let Some(path) = path {
                let opened = if cursor.is_none() {
                    gif_stream::Cursor::open(&path).map(|opened| cursor = Some(opened))
                } else {
                    Ok(())
                };
                opened.and_then(|()| cursor.as_mut().unwrap().next_looping()).map(|frame| {
                    frame.map(|frame| Prepared {
                        pixels: frame.pixels,
                        delay: frame.delay,
                        sequence: frame.sequence,
                    })
                })
            } else {
                Ok(Some(Prepared {
                    pixels: generate_frame(width, height, sequence + source_epoch * 1_000_000),
                    delay: Duration::ZERO,
                    sequence,
                }))
            };
            (cursor, output, start.elapsed())
        });
        let completion_metrics = self.metrics.clone();
        cx.spawn(async move |session, cx| {
            let (cursor, output, elapsed) = task.await;
            {
                let mut metrics = completion_metrics.borrow_mut();
                metrics.producer_completed += 1;
                metrics.producers_live -= 1;
            }
            let _ = session.update(cx, |session, cx| {
                // An invalid generation does not free the producer permit early.
                session.producing = false;
                session.reduced_motion = cx.reduce_motion();
                if session.active_since.is_some() && !session.permitted() {
                    session.update_gate();
                }
                if session.closing || session.source_epoch != source_epoch {
                    session.metrics.borrow_mut().stale_completions += 1;
                    session.reconcile(cx);
                    return;
                }
                // Composition progress belongs to this source, even after pause revoked its presentation ticket.
                session.cursor = cursor;
                let Some(frame) = (match output {
                    Ok(frame) => frame,
                    Err(error) => {
                        session.metrics.borrow_mut().decoder_error = Some(error.clone());
                        session.failure = Some(error);
                        session.reconcile(cx);
                        return;
                    },
                }) else {
                    session.at_eos = true;
                    session.reconcile(cx);
                    return;
                };
                session.metrics.borrow_mut().prepare.record(elapsed);
                if session.options.gif.is_some() {
                    let mut metrics = session.metrics.borrow_mut();
                    metrics.decoded_outputs += 1;
                    if let Some(cursor) = session.cursor.as_ref() {
                        metrics.peak_cursor_capacity =
                            metrics.peak_cursor_capacity.max(cursor.owned_capacity());
                        metrics.gif_loops = cursor.loops_done();
                    }
                    if version != session.generation {
                        metrics.revoked_presentations += 1;
                    }
                    drop(metrics);
                    session.pending = Some(frame);
                    session.reconcile(cx);
                } else if session.generation != version {
                    session.metrics.borrow_mut().stale_completions += 1;
                    session.reconcile(cx);
                } else {
                    session.publish(frame, cx);
                }
            });
        })
        .detach();
    }

    fn publish(&mut self, frame: Prepared, cx: &mut Context<Self>) {
        let capacity = frame.pixels.as_raw().capacity();
        let image = Arc::new(RenderImage::new([Frame::new(frame.pixels)]));
        let retired = self.front.replace(image);
        self.sequence = frame.sequence;
        let media_now = self.elapsed_media();
        self.next_frame_at = if self.options.gif.is_some() && frame.sequence > 1 {
            let next = self.next_frame_at + frame.delay;
            if next <= media_now {
                self.metrics.borrow_mut().missed_deadlines += 1;
                media_now + frame.delay
            } else {
                next
            }
        } else {
            media_now + frame.delay
        };
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.generated += 1;
            metrics.peak_generated_capacity = metrics.peak_generated_capacity.max(capacity);
        }
        if let Some(old) = retired {
            self.metrics.borrow_mut().retirement_requests += 1;
            cx.defer(move |cx| {
                cx.drop_image(old, None);
            });
        }
        cx.notify();
        if self.options.gif.is_some() && self.permitted() {
            self.prepare(cx);
        } else {
            self.arm(cx);
        }
    }

    fn switch_source(&mut self, cx: &mut Context<Self>) {
        self.source_epoch = self.source_epoch.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        self.timer.take();
        self.cursor.take();
        self.pending.take();
        self.failure = None;
        self.at_eos = false;
        self.media_time = Duration::ZERO;
        self.active_since = self.permitted().then(Instant::now);
        self.sequence = 0;
        self.next_frame_at = Duration::ZERO;
        if let Some(front) = self.front.take() {
            cx.defer(move |cx| {
                cx.drop_image(front, None);
            });
        }
        self.metrics.borrow_mut().source_changes += 1;
        self.reconcile(cx);
        cx.notify();
    }
}

fn generate_frame(width: u32, height: u32, sequence: u64) -> RgbaImage {
    let mut data = RgbaImage::new(width, height);
    for (x, y, pixel) in data.enumerate_pixels_mut() {
        let moving = ((x as u64 + sequence * 7) % u64::from(width)) < u64::from(width / 8);
        let bit = sequence & (1 << ((x / 16).min(31))) != 0;
        let marker = y < 32 && bit;
        // BGRA bytes; the single RenderImage retains the source dimensions.
        *pixel = image::Rgba(if marker {
            [255, 255, 255, 255]
        } else if moving {
            [40, 200, 100, 255]
        } else {
            [80, ((y * 128 / height) + 30) as u8, 35, 255]
        });
    }
    data
}

struct View {
    id: usize,
    session: Entity<Session>,
    metrics: Rc<RefCell<Metrics>>,
    subscriptions: Vec<Subscription>,
    scale: f32,
    key_received: Option<Instant>,
    focus: gpui::FocusHandle,
}

impl View {
    fn new(
        id: usize,
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let metrics = session.read(cx).metrics.clone();
        session.update(cx, |session, cx| {
            session
                .placements
                .insert(id, (window.is_window_active(), native_gate::capture(window)));
            session.reconcile(cx);
        });
        let focus = cx.focus_handle().tab_stop(true);
        if window.is_window_active() {
            focus.focus(window, cx);
        }
        let mut view = Self {
            id,
            session,
            metrics,
            subscriptions: Vec::new(),
            scale: window.scale_factor(),
            key_received: None,
            focus,
        };
        view.subscriptions.push(cx.observe(&view.session, |_, _, cx| cx.notify()));
        view.subscriptions.push(cx.observe_window_activation(window, |view, window, cx| {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "activation window={} active={}",
                view.id,
                window.is_window_active()
            );
            if window.is_window_active() {
                view.focus.focus(window, cx);
            }
            view.metrics.borrow_mut().activation_events += 1;
            view.session.update(cx, |session, cx| {
                session
                    .placements
                    .insert(view.id, (window.is_window_active(), native_gate::capture(window)));
                session.reconcile(cx);
            });
        }));
        view.subscriptions.push(cx.observe_window_bounds(window, |view, window, cx| {
            let scale = window.scale_factor();
            let mut metrics = view.metrics.borrow_mut();
            metrics.bounds_events += 1;
            if (view.scale - scale).abs() > f32::EPSILON {
                metrics.scale_changes += 1;
            }
            view.scale = scale;
            drop(metrics);
            view.session.update(cx, |session, cx| {
                session
                    .placements
                    .insert(view.id, (window.is_window_active(), native_gate::capture(window)));
                session.reconcile(cx);
            });
            cx.notify();
        }));
        cx.on_release(|view, cx| {
            view.session.update(cx, |session, cx| {
                session.placements.remove(&view.id);
                session.reconcile(cx);
            });
        })
        .detach();
        view
    }
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui::InteractiveElement;
        let session = self.session.read(cx);
        let image = session.front.clone();
        let count = session.options.placements;
        let sequence = session.sequence;
        let enabled = session.enabled;
        let metrics = self.metrics.clone();
        let key_received = self.key_received.take();
        div().id("media-probe").size_full().flex().flex_col().bg(rgb(0x2e3440))
            .track_focus(&self.focus)
            .font_family("Arial")
            .text_color(rgb(0xe5e9f0)).text_size(px(16.0))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|view, _, window, cx| {
                view.focus.focus(window, cx);
                view.session.update(cx, |session, cx| {
                    session.enabled = !session.enabled;
                    session.reconcile(cx);
                });
            }))
            .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| {
                view.metrics.borrow_mut().keys += 1;
                view.key_received = Some(Instant::now());
                if event.keystroke.key == "s" {
                    view.session.update(cx, |session, cx| session.switch_source(cx));
                } else if event.keystroke.key == "p" {
                    view.session.update(cx, |session, cx| { session.enabled = !session.enabled; session.reconcile(cx); });
                } else if event.keystroke.key == "r" {
                    let reduced_motion = !cx.reduce_motion();
                    cx.set_reduce_motion(reduced_motion);
                    view.session.update(cx, |session, cx| session.reconcile(cx));
                }
                cx.notify();
            }))
            .child(div().p_2().child(format!("Qualification only | frame {sequence} | play {enabled} | S switches | P pauses | click toggles")))
            .child(div().flex_1().min_h_0().relative().child(gpui::canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if let Some(received) = key_received {
                        metrics.borrow_mut().input_handler_to_paint.record(received.elapsed());
                    }
                    let Some(image) = &image else { return; };
                    let canvas_started = Instant::now();
                    for index in 0..count {
                        let area = Bounds::new(bounds.origin + point(px(0.0), bounds.size.height * index as f32 / count as f32),
                            size(bounds.size.width, bounds.size.height / count as f32));
                        let start = Instant::now();
                        let result = window.paint_image(area, area, Corners::all(px(0.0)), image.clone(), 0, false);
                        let mut metrics = metrics.borrow_mut();
                        metrics.paint_calls += 1;
                        metrics.paint_image_cpu.record(start.elapsed());
                        if result.is_err() { metrics.failed_paints += 1; }
                    }
                    metrics.borrow_mut().canvas_cpu.record(canvas_started.elapsed());
                },
            ).size_full()))
    }
}

fn main() {
    let options = Options::parse();
    if let Some(parent) = options.output.parent() {
        std::fs::create_dir_all(parent).expect("output directory");
    }
    let metrics = Rc::new(RefCell::new(Metrics::default()));
    let captured = metrics.clone();
    let saved_options = options.clone();
    let started = Instant::now();
    gpui_platform::application().with_quit_mode(gpui::QuitMode::LastWindowClosed).run(move |cx| {
        let options = options.clone();
        cx.set_reduce_motion(options.reduced_motion);
        let session = cx.new(|cx| Session::new(options.clone(), metrics.clone(), cx));
        let mut handles = Vec::new();
        for index in 0..options.windows {
            let session = session.clone();
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(50.0 + index as f32 * 70.0), px(70.0 + index as f32 * 50.0)),
                            size(px(800.0), px(560.0)),
                        ))),
                        focus: options.interactive && index == 0,
                        show: options.interactive,
                        ..Default::default()
                    },
                    |window, cx| {
                        window.set_window_title(&format!("Pebrel-Media-Probe-{index}"));
                        cx.new(|cx| View::new(index, session, window, cx))
                    },
                )
                .expect("native window");
            handles.push(handle);
        }
        if options.interactive {
            cx.activate(true);
        }
        cx.spawn(async move |cx| {
            // One qualification-control timeout, not a recurring playback tick.
            cx.background_executor().timer(Duration::from_secs(options.seconds)).await;
            let _ = session.update(cx, |session, cx| {
                session.enabled = false;
                session.closing = true;
                session.reconcile(cx);
                if let Some(front) = session.front.take() {
                    cx.drop_image(front, None);
                }
            });
            if session.read_with(cx, |session, _| session.producing) {
                // One bounded shutdown grace period; no media timer or hidden recurring poll.
                cx.background_executor()
                    .timer(Duration::from_millis(options.worker_ms + 200))
                    .await;
            }
            for handle in handles {
                let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let metrics = captured.borrow();
    let report = serde_json::json!({
        "options": saved_options, "pid": std::process::id(), "duration_ms": started.elapsed().as_millis(),
        "metrics": &*metrics,
        "prepare_p95_upper_us": metrics.prepare.percentile_upper_us(95),
        "paint_image_cpu_p95_upper_us": metrics.paint_image_cpu.percentile_upper_us(95),
        "canvas_cpu_p95_upper_us": metrics.canvas_cpu.percentile_upper_us(95),
        "input_handler_to_paint_p95_upper_us": metrics.input_handler_to_paint.percentile_upper_us(95),
        "measurement_limits": ["paint_image return is not native present completion", "native atlas contains defaults to false at this pin; upload/page/retirement-completion counts require separate private backend instrumentation", "GIF cursor is a lab prototype; no product media capability enabled, no video or GLSL execution in this executable", "focus/minimize are checked on Windows; other platforms and occlusion need native acceptance", "the qualification timeout is separate from media playback scheduling"]
    });
    std::fs::write(
        &saved_options.output,
        serde_json::to_vec_pretty(&report).expect("serialize report"),
    )
    .expect("write report");
    if metrics.decoder_error.is_some() {
        std::process::exit(1);
    }
}
