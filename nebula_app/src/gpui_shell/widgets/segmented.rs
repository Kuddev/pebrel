//! 紧邻的互斥选项共用可中断的滑块过渡，业务选择仍由各自视图处理。

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, App, Div, ElementId, InteractiveElement as _, IntoElement as _,
    ParentElement as _, RenderOnce, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, px, relative,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

pub(in crate::gpui_shell) struct IndicatorMotion {
    pub(in crate::gpui_shell) position: Rc<Cell<f32>>,
    pub(in crate::gpui_shell) from: f32,
    pub(in crate::gpui_shell) target: f32,
    // 宽度、纵向位置和高度共用时钟，窄窗换行的格式选项也不会画出整排大底色。
    dimensions: [Rc<Cell<f32>>; 3],
    from_dimensions: [f32; 3],
    target_dimensions: Option<[f32; 3]>,
    epoch: u64,
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
            epoch: 0,
        }
    });
    let (from, from_dimensions, epoch, position, dimensions) = motion.update(cx, |motion, _| {
        if motion.target != target || motion.target_dimensions != target_dimensions {
            // 快速反向时从当前显示位置接续，不能跳回上一次选项的起点。
            motion.from = motion.position.get();
            motion.from_dimensions = std::array::from_fn(|index| motion.dimensions[index].get());
            motion.target = target;
            motion.target_dimensions = target_dimensions;
            motion.epoch += 1;
        }
        if reduced {
            // 恢复系统动效后也应停在已选项，不能重播减弱动效期间的旧过渡。
            motion.position.set(target);
            motion.from = target;
            if let Some(values) = target_dimensions {
                for (cell, value) in motion.dimensions.iter().zip(values) {
                    cell.set(value);
                }
                motion.from_dimensions = values;
            }
        }
        (
            motion.from,
            motion.from_dimensions,
            motion.epoch,
            motion.position.clone(),
            motion.dimensions.clone(),
        )
    });
    if (from == target && target_dimensions.is_none_or(|values| from_dimensions == values))
        || reduced
    {
        position.set(target);
        indicator
            .left(relative(target))
            .when_some(target_dimensions, |indicator, values| {
                for (cell, value) in dimensions.iter().zip(values) {
                    cell.set(value);
                }
                let [width, top, height] = values;
                indicator.w(relative(width)).top(relative(top)).h(relative(height))
            })
            .into_any_element()
    } else {
        indicator
            .with_animation(
                ElementId::NamedInteger(format!("{id}-slide").into(), epoch),
                Animation::new(Duration::from_millis(280))
                    .with_easing(|progress| crate::motion::Easing::CssStandard.sample(progress)),
                move |indicator, progress| {
                    let current = from + (target - from) * progress;
                    position.set(current);
                    indicator.left(relative(current)).when_some(
                        target_dimensions,
                        |indicator, targets| {
                            let values: [f32; 3] = std::array::from_fn(|index| {
                                from_dimensions[index]
                                    + (targets[index] - from_dimensions[index]) * progress
                            });
                            for (cell, value) in dimensions.iter().zip(values) {
                                cell.set(value);
                            }
                            let [width, top, height] = values;
                            indicator.w(relative(width)).top(relative(top)).h(relative(height))
                        },
                    )
                },
            )
            .into_any_element()
    }
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
        // GPUI 的一次性 Animation 使用真实 Instant，executor 的假时钟只推进任务计时器。
        std::thread::sleep(Duration::from_millis(70));
        window.update(draw);
        let middle = window.debug_bounds("natural-segments-indicator").unwrap();
        assert!(
            middle.left() > first.left() && middle.left() < second.left(),
            "first={first:?}, middle={middle:?}, second={second:?}"
        );
        assert!(middle.size.width > first.size.width && middle.size.width < second.size.width);

        // 在同一次更新中采样、反向并读取首帧，避免调度器在多个 update 之间
        // 绘制额外动画帧，使连续性断言拿新位置与过期快照比较。
        let (before_reverse, reversed) = window.update(|window, cx| {
            let before_reverse =
                window.debug_bounds_for_selector("natural-segments-indicator").unwrap();
            host.update(cx, |this, cx| {
                this.selected = 0;
                cx.notify();
            });
            window.refresh();
            draw(window, cx);
            let reversed = window.debug_bounds_for_selector("natural-segments-indicator").unwrap();
            (before_reverse, reversed)
        });
        assert!(
            (f32::from(reversed.left() - before_reverse.left())).abs() < 0.1,
            "before_reverse={before_reverse:?}, reversed={reversed:?}"
        );
        assert!(
            (f32::from(reversed.size.width - before_reverse.size.width)).abs() < 0.1,
            "before_reverse={before_reverse:?}, reversed={reversed:?}"
        );
        std::thread::sleep(Duration::from_millis(400));
        window.update(draw);
        assert_eq!(window.debug_bounds("natural-segments-indicator").unwrap(), first);

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
        std::thread::sleep(Duration::from_millis(70));
        window.update(draw);
        let middle = window.debug_bounds("natural-segments-indicator").unwrap();
        assert!(middle.top() > first.top() && middle.top() < third.top());
        std::thread::sleep(Duration::from_millis(400));
        window.update(draw);
        assert_eq!(window.debug_bounds("natural-segments-indicator").unwrap(), third);
    }
}
