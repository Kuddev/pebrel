//! 紧邻的互斥选项共用可中断的滑块过渡，业务选择仍由各自视图处理。

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Div, InteractiveElement as _, IntoElement as _, ParentElement as _, RenderOnce,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window, div, px,
    relative,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

const SLIDE_DURATION: Duration = Duration::from_millis(280);

pub(in crate::gpui_shell) struct IndicatorMotion {
    pub(in crate::gpui_shell) position: Rc<Cell<f32>>,
    pub(in crate::gpui_shell) from: f32,
    pub(in crate::gpui_shell) target: f32,
    // 宽度、纵向位置和高度共用时钟，窄窗换行的格式选项也不会画出整排大底色。
    dimensions: [Rc<Cell<f32>>; 3],
    from_dimensions: [f32; 3],
    target_dimensions: Option<[f32; 3]>,
    started_at: Instant,
}

impl IndicatorMotion {
    fn sample(
        &mut self,
        now: Instant,
        target: f32,
        target_dimensions: Option<[f32; 3]>,
        reduced: bool,
    ) -> bool {
        if self.target != target || self.target_dimensions != target_dimensions {
            // 快速反向时从当前显示位置接续，不能跳回上一次选项的起点。
            self.from = self.position.get();
            self.from_dimensions = std::array::from_fn(|index| self.dimensions[index].get());
            self.target = target;
            self.target_dimensions = target_dimensions;
            self.started_at = now;
        }
        if reduced {
            // 恢复系统动效后也应停在已选项，不能重播减弱动效期间的旧过渡。
            self.from = target;
            if let Some(values) = target_dimensions {
                self.from_dimensions = values;
            }
        }
        let elapsed = now.saturating_duration_since(self.started_at);
        let finished = (self.from == target
            && target_dimensions.is_none_or(|values| self.from_dimensions == values))
            || elapsed >= SLIDE_DURATION
            || reduced;
        let progress = if finished {
            1.0
        } else {
            crate::motion::Easing::CssStandard
                .sample(elapsed.as_secs_f32() / SLIDE_DURATION.as_secs_f32())
        };
        self.position.set(if finished {
            target
        } else {
            self.from + (target - self.from) * progress
        });
        if let Some(targets) = target_dimensions {
            for (index, cell) in self.dimensions.iter().enumerate() {
                cell.set(if finished {
                    targets[index]
                } else {
                    self.from_dimensions[index]
                        + (targets[index] - self.from_dimensions[index]) * progress
                });
            }
        }
        !finished
    }
}

pub(in crate::gpui_shell) fn animate_segment_indicator(
    id: SharedString,
    indicator: Stateful<Div>,
    target: f32,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyElement {
    animate_indicator(id, indicator, target, None, window, cx)
}

fn animate_indicator(
    id: SharedString,
    indicator: Stateful<Div>,
    target: f32,
    target_dimensions: Option<[f32; 3]>,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyElement {
    let reduced = cx.reduce_motion();
    // 同一帧只取一次单调时间：反向首帧保持当前位置，测试也能控制真实插值路径。
    let now = cx.background_executor().now();
    let motion = window.use_keyed_state(SharedString::from(format!("{id}-motion")), cx, |_, _| {
        IndicatorMotion {
            position: Rc::new(Cell::new(target)),
            from: target,
            target,
            dimensions: target_dimensions
                .unwrap_or_default()
                .map(|value| Rc::new(Cell::new(value))),
            from_dimensions: target_dimensions.unwrap_or_default(),
            target_dimensions,
            started_at: now,
        }
    });
    let (position, dimensions, animating) = motion.update(cx, |motion, _| {
        let animating = motion.sample(now, target, target_dimensions, reduced);
        (
            motion.position.get(),
            std::array::from_fn::<_, 3, _>(|index| motion.dimensions[index].get()),
            animating,
        )
    });
    if animating {
        // GPUI 的该接口在下一帧 notify 当前 view，确保重新取时并计算滑块布局。
        window.request_animation_frame();
    }
    indicator
        .left(relative(position))
        .when(target_dimensions.is_some(), |indicator| {
            let [width, top, height] = dimensions;
            indicator.w(relative(width)).top(relative(top)).h(relative(height))
        })
        .into_any_element()
}

/// 读取实际按钮布局，兼容等分、自然宽度和带等宽计数的混排选项。
#[derive(gpui::IntoElement)]
pub(in crate::gpui_shell) struct SegmentedRow {
    id: SharedString,
    selected: usize,
    base: Div,
    row: Div,
    indicator: Div,
}

impl SegmentedRow {
    pub(in crate::gpui_shell) fn new(
        id: impl Into<SharedString>,
        selected: usize,
        row: Div,
        indicator: Div,
    ) -> Self {
        Self { id: id.into(), selected, base: div().relative(), row, indicator }
    }
}

impl gpui::Styled for SegmentedRow {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for SegmentedRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl gpui::IntoElement {
        let geometry = window.use_keyed_state(
            SharedString::from(format!("{}-geometry", self.id)),
            cx,
            |_, _| Vec::<[f32; 4]>::new(),
        );
        let indicator =
            geometry.read(cx).get(self.selected).copied().map(|[left, width, top, height]| {
                let id = SharedString::from(format!("{}-indicator", self.id));
                let selector = id.clone();
                animate_indicator(
                    self.id.clone(),
                    self.indicator.id(id).debug_selector(move || selector.to_string()).absolute(),
                    left,
                    Some([width, top, height]),
                    window,
                    cx,
                )
            });
        let children_bounds = Rc::new(RefCell::new(Vec::new()));
        let measured = children_bounds.clone();
        let row = self.row.on_children_prepainted(move |bounds, _, _| {
            *measured.borrow_mut() = bounds;
        });
        let view = window.current_view();
        self.base
            .child(div().absolute().top_0().left_0().size_full())
            .children(indicator)
            .child(row)
            .on_children_prepainted(move |bounds, _, cx| {
                // 浮块的百分比基准是外层容器；内层行可能被窄窗限制了宽度。
                let Some(frame) = bounds
                    .first()
                    .filter(|bounds| bounds.size.width > px(0.0) && bounds.size.height > px(0.0))
                else {
                    return;
                };
                let spans = children_bounds
                    .borrow()
                    .iter()
                    .map(|child| {
                        [
                            f32::from(child.left() - frame.left()) / f32::from(frame.size.width),
                            f32::from(child.size.width) / f32::from(frame.size.width),
                            f32::from(child.top() - frame.top()) / f32::from(frame.size.height),
                            f32::from(child.size.height) / f32::from(frame.size.height),
                        ]
                    })
                    .collect::<Vec<_>>();
                let changed = geometry.update(cx, |previous, _| {
                    if *previous != spans {
                        *previous = spans;
                        true
                    } else {
                        false
                    }
                });
                if changed {
                    // 首帧测量后重绘；之后点击直接复用真实槽位，不靠字符数猜宽。
                    cx.notify(view);
                }
            })
            .id(self.id)
            .into_any_element()
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Context, IntoElement, Render, TestAppContext};
    use gpui_component::{button::Button, h_flex};

    struct MotionHost {
        selected: usize,
        wrap: bool,
    }

    impl Render for MotionHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            SegmentedRow::new(
                "natural-segments",
                self.selected,
                h_flex()
                    .h(px(28.0))
                    .gap(px(3.0))
                    .when(self.wrap, |row| row.w(px(150.0)).h_auto().flex_wrap())
                    .children([48.0, 112.0, 76.0].into_iter().enumerate().map(|(index, width)| {
                        Button::new(index)
                            .debug_selector(move || format!("segment-slot-{index}"))
                            .w(px(width))
                            .h(px(28.0))
                            .label(format!("Choice {index}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = index;
                                cx.notify();
                            }))
                    })),
                div().rounded(px(6.0)).bg(gpui::rgb(0x668899)),
            )
        }
    }

    #[gpui::test]
    fn unequal_segments_slide_resize_reverse_and_respect_reduced_motion(cx: &mut TestAppContext) {
        cx.update(|cx| gpui_component::init(cx));
        let mut host = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| MotionHost { selected: 0, wrap: false });
            host = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let host = host.unwrap();
        let draw = |window: &mut Window, cx: &mut App| {
            window.simulate_next_frame(cx);
            let _ = window.draw(cx);
        };
        // 首次真实布局确定槽位，下一帧即在选中按钮上绘制浮块。
        window.update(draw);
        window.update(draw);
        let first = window.debug_bounds("segment-slot-0").unwrap();
        assert_eq!(window.debug_bounds("natural-segments-indicator").unwrap(), first);

        let second = window.debug_bounds("segment-slot-1").unwrap();
        window.simulate_click(second.center(), gpui::Modifiers::default());
        host.read_with(window, |host, _| assert_eq!(host.selected, 1));
        window.update(draw);
        window.executor().advance_clock(Duration::from_millis(70));
        window.update(draw);
        let middle = window.debug_bounds("natural-segments-indicator").unwrap();
        assert!(
            middle.left() > first.left() && middle.left() < second.left(),
            "first={first:?}, middle={middle:?}, second={second:?}"
        );
        assert!(middle.size.width > first.size.width && middle.size.width < second.size.width);

        window.update(|_, cx| {
            host.update(cx, |this, cx| {
                this.selected = 0;
                cx.notify();
            })
        });
        window.update(draw);
        let reversed = window.debug_bounds("natural-segments-indicator").unwrap();
        assert!((f32::from(reversed.left() - middle.left())).abs() < 0.1);
        assert!((f32::from(reversed.size.width - middle.size.width)).abs() < 0.1);
        window.executor().advance_clock(Duration::from_millis(400));
        window.update(draw);
        assert_eq!(window.debug_bounds("natural-segments-indicator").unwrap(), first);
        window.update(|window, cx| assert_eq!(window.simulate_next_frame(cx), 0));

        window.update(|_, cx| {
            cx.set_reduce_motion(true);
            host.update(cx, |this, cx| {
                this.selected = 2;
                cx.notify();
            });
        });
        window.update(draw);
        let third = window.debug_bounds("segment-slot-2").unwrap();
        assert_eq!(window.debug_bounds("natural-segments-indicator").unwrap(), third);
        window.update(|window, cx| assert_eq!(window.simulate_next_frame(cx), 0));
        window.update(|_, cx| cx.set_reduce_motion(false));
        window.update(draw);
        assert_eq!(window.debug_bounds("natural-segments-indicator").unwrap(), third);

        window.update(|_, cx| {
            cx.set_reduce_motion(true);
            host.update(cx, |this, cx| {
                this.wrap = true;
                this.selected = 0;
                cx.notify();
            });
        });
        window.update(draw);
        window.update(draw);
        let first = window.debug_bounds("segment-slot-0").unwrap();
        let third = window.debug_bounds("segment-slot-2").unwrap();
        window.update(|_, cx| {
            cx.set_reduce_motion(false);
            host.update(cx, |this, cx| {
                this.selected = 2;
                cx.notify();
            });
        });
        window.update(draw);
        window.executor().advance_clock(Duration::from_millis(70));
        window.update(draw);
        let middle = window.debug_bounds("natural-segments-indicator").unwrap();
        assert!(middle.top() > first.top() && middle.top() < third.top());
        window.executor().advance_clock(Duration::from_millis(400));
        window.update(draw);
        assert_eq!(window.debug_bounds("natural-segments-indicator").unwrap(), third);
        window.update(|window, cx| assert_eq!(window.simulate_next_frame(cx), 0));
    }
}
