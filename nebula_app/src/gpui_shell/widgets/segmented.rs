//! Settings and reader segments share one interruptible indicator transition.

use gpui::{
    Animation, AnimationExt as _, App, Div, ElementId, IntoElement as _, SharedString, Stateful,
    Styled as _, Window, relative,
};
use std::{cell::Cell, rc::Rc, time::Duration};

pub(in crate::gpui_shell) struct IndicatorMotion {
    pub(in crate::gpui_shell) position: Rc<Cell<f32>>,
    pub(in crate::gpui_shell) from: f32,
    pub(in crate::gpui_shell) target: f32,
    epoch: u64,
}

pub(in crate::gpui_shell) fn animate_segment_indicator(
    id: SharedString,
    indicator: Stateful<Div>,
    target: f32,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyElement {
    let motion = window.use_keyed_state(SharedString::from(format!("{id}-motion")), cx, |_, _| {
        IndicatorMotion { position: Rc::new(Cell::new(target)), from: target, target, epoch: 0 }
    });
    let (from, epoch, position) = motion.update(cx, |motion, _| {
        if motion.target != target {
            // 快速反向时从当前显示位置接续，不能跳回上一次选项的起点。
            motion.from = motion.position.get();
            motion.target = target;
            motion.epoch += 1;
        }
        (motion.from, motion.epoch, motion.position.clone())
    });
    if from == target || cx.reduce_motion() {
        position.set(target);
        indicator.left(relative(target)).into_any_element()
    } else {
        indicator
            .with_animation(
                ElementId::NamedInteger(format!("{id}-slide").into(), epoch),
                Animation::new(Duration::from_millis(280))
                    .with_easing(|progress| crate::motion::Easing::CssStandard.sample(progress)),
                move |indicator, progress| {
                    let current = from + (target - from) * progress;
                    position.set(current);
                    indicator.left(relative(current))
                },
            )
            .into_any_element()
    }
}
