//! Small resource charts resolve theme colors on every render and never synthesize samples.
use crate::gpui_shell::prelude::*;
use gpui::{
    App, AppContext as _, Bounds, Context, Div, Entity, FocusHandle, Hsla, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseMoveEvent, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div, point, px,
};
use std::time::Instant;
use std::{cell::Cell, rc::Rc};

/// One point uses real elapsed seconds rather than a fabricated uniform sample cadence.
#[derive(Clone)]
pub(super) struct ChartPoint {
    /// Actual completion time; age is recalculated when pointer interaction repaints the graph.
    pub sampled: Instant,
    /// Measured value; absent points break the curve.
    pub value: Option<f32>,
}

/// Independently colored measurements sharing one quantitative chart scale.
#[derive(Clone)]
pub(super) struct ChartSeries {
    /// Real samples in chronological order.
    pub points: Vec<ChartPoint>,
    /// Color resolved from the current application's chart palette.
    pub color: Hsla,
    /// Localized metric label shown in the sample inspector.
    pub label: String,
    /// Whether values are byte rates rather than percentages.
    pub bytes_per_second: bool,
}

/// A persistent sample inspector retains pointer position while its parent refreshes telemetry.
pub(super) struct TrendChart {
    /// Measurements prepared by the parent, with current theme colors.
    series: Rc<Vec<ChartSeries>>,
    /// Shared vertical scale, always positive.
    maximum: f32,
    /// Last painted chart bounds in window coordinates, shared only on the UI thread.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Current pointer position; leaving the graph clears the inspector.
    pointer: Option<Point<Pixels>>,
    /// Keyboard focus allows sample inspection without a pointing device.
    focus: FocusHandle,
    /// Actual selected timestamp survives sample insertion and index changes.
    keyboard_sample: Option<Instant>,
}

impl TrendChart {
    /// Create an empty chart without collecting telemetry or installing timers.
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            series: Rc::new(Vec::new()),
            maximum: 1.0,
            bounds: Rc::new(Cell::new(Bounds::default())),
            pointer: None,
            focus: cx.focus_handle(),
            keyboard_sample: None,
        }
    }

    /// Release host-specific inspection state when the sidebar changes execution scope.
    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.series = Rc::new(Vec::new());
        self.pointer = None;
        self.keyboard_sample = None;
        cx.notify();
    }
}

/// Supply prepared samples to the existing chart entity, preserving its pointer state.
pub(super) fn trend(
    chart: &Entity<TrendChart>,
    series: Vec<ChartSeries>,
    maximum: f32,
    cx: &mut App,
) -> gpui::AnyElement {
    chart.update(cx, |view, cx| {
        view.series = Rc::new(series);
        view.maximum = maximum.max(1.0);
        cx.notify();
    });
    chart.clone().into_any_element()
}

impl Render for TrendChart {
    /// Paint measured curves and inspect the closest actual sample instead of interpolating values.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let series = self.series.clone();
        let maximum = self.maximum;
        let bounds_cell = self.bounds.clone();
        let bounds = self.bounds.get();
        let now = Instant::now();
        let points = self.series.first().map(|series| series.points.as_slice()).unwrap_or_default();
        let selected = self
            .pointer
            .filter(|position| bounds.contains(position))
            .and_then(|position| {
                let age = 60.0
                    * (1.0
                        - f32::from(position.x - bounds.origin.x)
                            / f32::from(bounds.size.width).max(1.0));
                points
                    .iter()
                    .filter(|sample| {
                        now.saturating_duration_since(sample.sampled).as_secs_f32() <= 60.0
                    })
                    .min_by(|a, b| {
                        let a = now.saturating_duration_since(a.sampled).as_secs_f32();
                        let b = now.saturating_duration_since(b.sampled).as_secs_f32();
                        (a - age).abs().total_cmp(&(b - age).abs())
                    })
            })
            .or_else(|| {
                if !self.focus.is_focused(window) {
                    return None;
                }
                let selected = self.keyboard_sample?;
                points.iter().find(|sample| {
                    sample.sampled == selected
                        && now.saturating_duration_since(sample.sampled).as_secs_f32() <= 60.0
                })
            });
        let selected_age =
            selected.map(|sample| now.saturating_duration_since(sample.sampled).as_secs_f32());
        let grid_color = cx.theme().border;
        let crosshair_color = cx.theme().muted_foreground;
        let canvas = gpui::canvas(
            move |bounds, _, _| {
                bounds_cell.set(bounds);
            },
            move |bounds, _, window, _| {
                let left = f32::from(bounds.origin.x);
                let top = f32::from(bounds.origin.y) + 1.0;
                let width = f32::from(bounds.size.width);
                let height = (f32::from(bounds.size.height) - 2.0).max(1.0);
                let mut grid = gpui::PathBuilder::stroke(px(1.0));
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    grid.move_to(point(px(left), px(top + fraction * height)));
                    grid.line_to(point(px(left + width), px(top + fraction * height)));
                }
                if let Ok(path) = grid.build() {
                    window.paint_path(path, grid_color);
                }
                if let Some(age) = selected_age {
                    let x = left + width * (1.0 - age / 60.0).clamp(0.0, 1.0);
                    let mut path = gpui::PathBuilder::stroke(px(1.0));
                    path.move_to(point(px(x), px(top)));
                    path.line_to(point(px(x), px(top + height)));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, crosshair_color);
                    }
                }
                for series in series.iter() {
                    let mut path = gpui::PathBuilder::stroke(px(1.5));
                    let mut connected = false;
                    for sample in &series.points {
                        let age = now.saturating_duration_since(sample.sampled).as_secs_f32();
                        if age > 60.0 {
                            connected = false;
                            continue;
                        }
                        if let Some(value) = sample.value {
                            let position = point(
                                px(left + width * (1.0 - age / 60.0).clamp(0.0, 1.0)),
                                px(top + height * (1.0 - value / maximum).clamp(0.0, 1.0)),
                            );
                            if connected {
                                path.line_to(position);
                            } else {
                                path.move_to(position);
                            }
                            connected = true;
                        } else {
                            connected = false;
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, series.color);
                    }
                }
            },
        )
        .w_full()
        .h(px(48.0));
        let mut root = div()
            .id("resource-trend")
            .relative()
            .track_focus(&self.focus)
            .tab_index(0)
            .w_full()
            .h(px(48.0))
            .focus(|style| style.bg(cx.theme().muted))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    if view.keyboard_sample.take().is_some() {
                        cx.stop_propagation();
                        cx.notify();
                    }
                    return;
                }
                if !matches!(key, "left" | "right" | "home" | "end") {
                    return;
                }
                let Some(series) = view.series.first() else { return };
                let samples = series
                    .points
                    .iter()
                    .filter(|sample| sample.sampled.elapsed().as_secs_f32() <= 60.0)
                    .collect::<Vec<_>>();
                if samples.is_empty() {
                    return;
                }
                let current = samples
                    .iter()
                    .position(|sample| Some(sample.sampled) == view.keyboard_sample)
                    .unwrap_or(samples.len() - 1);
                let index = match key {
                    "left" => current.saturating_sub(1),
                    "right" => (current + 1).min(samples.len() - 1),
                    "home" => 0,
                    _ => samples.len() - 1,
                };
                view.keyboard_sample = Some(samples[index].sampled);
                view.pointer = None;
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                view.pointer = Some(event.position);
                view.keyboard_sample = None;
                cx.notify();
            }))
            .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                if !hovered {
                    view.pointer = None;
                    cx.notify();
                }
            }))
            .child(canvas);
        if let Some(sample) = selected {
            let language = crate::gpui_shell::config::ui_language(cx);
            let mut tooltip =
                v_flex().p_2().gap_1().w(px(180.0)).text_size(px(11.0)).popover_style(cx);
            let age = now.saturating_duration_since(sample.sampled).as_secs_f32();
            tooltip = tooltip.child(div().text_color(cx.theme().muted_foreground).child(
                language.format(
                    crate::i18n::Message::MonitorSampleAge,
                    &[("seconds", &format!("{age:.0}"))],
                ),
            ));
            for series in self.series.iter() {
                let value = series
                    .points
                    .iter()
                    .find(|point| point.sampled == sample.sampled)
                    .and_then(|point| point.value);
                let value = if series.bytes_per_second {
                    super::presentation::optional_rate(value.map(f64::from))
                } else {
                    super::presentation::percent(value)
                };
                tooltip = tooltip.child(
                    h_flex()
                        .gap_2()
                        .child(div().w(px(5.0)).h(px(5.0)).rounded_full().bg(series.color))
                        .child(div().flex_1().child(series.label.clone()))
                        .child(div().child(value)),
                );
            }
            let anchor = self.pointer.unwrap_or_else(|| {
                point(
                    bounds.origin.x + bounds.size.width * (1.0 - age / 60.0).clamp(0.0, 1.0),
                    bounds.origin.y + bounds.size.height,
                )
            });
            root = root.child(
                gpui::deferred(
                    gpui::anchored()
                        .position(anchor + point(px(12.0), px(12.0)))
                        .snap_to_window()
                        .child(tooltip),
                )
                .with_priority(1),
            );
        }
        root
    }
}

/// A memory category's bytes and theme color determine its share of the ring.
pub(super) struct MemorySlice {
    /// Physical memory amount in bytes.
    pub bytes: u64,
    /// Current theme color matching the category legend.
    pub color: Hsla,
}

/// Draw proportional memory arcs without a background image or fixed light/dark colors.
pub(super) fn memory_ring(slices: Vec<MemorySlice>, total: u64, cx: &App) -> gpui::AnyElement {
    let border = cx.theme().border;
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center_x = f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0;
            let center_y = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0;
            let radius = f32::from(bounds.size.width.min(bounds.size.height)) / 2.0 - 4.0;
            let mut offset = -std::f32::consts::FRAC_PI_2;
            let mut draw_arc = |start: f32, angle: f32, color: Hsla| {
                let steps = (angle.abs() * 24.0).ceil().max(1.0) as usize;
                let mut path = gpui::PathBuilder::stroke(px(7.0));
                for step in 0..=steps {
                    let angle = start + angle * step as f32 / steps as f32;
                    let position = point(
                        px(center_x + radius * angle.cos()),
                        px(center_y + radius * angle.sin()),
                    );
                    if step == 0 {
                        path.move_to(position);
                    } else {
                        path.line_to(position);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            };
            draw_arc(offset, std::f32::consts::TAU, border);
            if total > 0 {
                for slice in slices {
                    let angle = std::f32::consts::TAU
                        * (slice.bytes as f64 / total as f64).clamp(0.0, 1.0) as f32;
                    if angle > 0.0 {
                        draw_arc(offset, angle, slice.color);
                        offset += angle;
                    }
                }
            }
        },
    )
    .w(px(48.0))
    .h(px(48.0))
    .flex_shrink_0()
    .into_any_element()
}

/// Core load uses a theme-colored segmented track whose density follows available width.
pub(super) fn core_track(usage: Option<f32>, cx: &App) -> gpui::AnyElement {
    let color = cx.theme().chart_1;
    let empty = cx.theme().border;
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let count = (f32::from(bounds.size.width) / 5.0).floor().max(1.0) as usize;
            let step = f32::from(bounds.size.width) / count as f32;
            for index in 0..count {
                let mut path = gpui::PathBuilder::stroke(px((step - 2.0).max(1.0)));
                let x = bounds.origin.x + px((index as f32 + 0.5) * step);
                path.move_to(point(x, bounds.origin.y));
                path.line_to(point(x, bounds.origin.y + bounds.size.height));
                let filled = usage.is_some_and(|value| {
                    index as f32 / (count as f32) < (value / 100.0).clamp(0.0, 1.0)
                });
                if let Ok(path) = path.build() {
                    window.paint_path(path, if filled { color } else { empty });
                }
            }
        },
    )
    .w_full()
    .h(px(10.0))
    .into_any_element()
}

/// Pair each category's textual label with its matching chart marker for theme-independent meaning.
pub(super) fn legend(label: &str, value: String, color: Hsla, cx: &App) -> Div {
    v_flex()
        .flex_1()
        .min_w(px(76.0))
        .p_2()
        .gap_1()
        .rounded_sm()
        .bg(cx.theme().muted)
        .child(
            h_flex().gap_1().child(div().w(px(5.0)).h(px(5.0)).rounded_full().bg(color)).child(
                div()
                    .text_size(px(10.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_owned()),
            ),
        )
        .child(div().text_size(px(11.0)).child(value))
}
