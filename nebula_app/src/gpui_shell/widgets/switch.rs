//! Theme-aware switch presentation, using Button's pointer, keyboard and focus lifecycle.

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, App, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Disableable as _};

struct SwitchMotion {
    position: Rc<Cell<f32>>,
    from: f32,
    target: f32,
    epoch: u64,
}

#[derive(IntoElement)]
pub struct NebulaSwitch {
    key: SharedString,
    checked: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
}

impl NebulaSwitch {
    pub fn new(key: impl Into<SharedString>) -> Self {
        Self { key: key.into(), checked: false, disabled: false, on_click: None }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for NebulaSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let target = if self.checked { 1.0 } else { 0.0 };
        let state = window.use_keyed_state(
            SharedString::from(format!("switch-motion-{}", self.key)),
            cx,
            |_, _| SwitchMotion {
                position: Rc::new(Cell::new(target)),
                from: target,
                target,
                epoch: 0,
            },
        );
        let reduced = cx.reduce_motion() || self.disabled;
        let (from, position, epoch) = state.update(cx, |motion, _| {
            if motion.target != target {
                motion.from = motion.position.get();
                motion.target = target;
                motion.epoch += 1;
            }
            if reduced {
                motion.from = target;
                motion.position.set(target);
            }
            (motion.from, motion.position.clone(), motion.epoch)
        });
        let primary = cx.theme().primary;
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let white = gpui::hsla(0.0, 0.0, 1.0, 1.0);
        let checked = self.checked;
        let group = SharedString::from(format!("switch-hit-{}", self.key));
        let track = div()
            .w(px(40.0))
            .h(px(22.0))
            .relative()
            .flex_shrink_0()
            .rounded(px(11.0))
            .border_1()
            .when(!self.disabled, |track| {
                track.group_hover(group.clone(), move |style| {
                    style.border_color(if checked { primary } else { muted })
                })
            });
        let paint = move |track: gpui::Div, t: f32| {
            track.bg(primary.opacity(t)).border_color(border.blend(primary.opacity(t))).child(
                div()
                    .absolute()
                    .left(px(4.0 + 16.0 * t))
                    .top(px(4.0 - 2.0 * t))
                    .size(px(12.0 + 4.0 * t))
                    .rounded_full()
                    .bg(muted.blend(white.opacity(t))),
            )
        };
        let track = if from == target {
            paint(track, target).into_any_element()
        } else {
            track
                .with_animation(
                    ElementId::NamedInteger(format!("switch-slide-{}", self.key).into(), epoch),
                    Animation::new(Duration::from_millis(280))
                        .with_easing(|t| crate::motion::Easing::CssStandard.sample(t)),
                    move |track, t| {
                        let current = from + (target - from) * t;
                        position.set(current);
                        paint(track, current)
                    },
                )
                .into_any_element()
        };
        let selector = format!("nebula-switch-{}", self.key);
        let button = Button::new(ElementId::Name(selector.clone().into()))
            .debug_selector(move || selector.clone())
            .role(gpui::Role::Switch)
            .toggled(checked)
            .disabled(self.disabled)
            .custom(ButtonCustomVariant::new(cx))
            .w(px(44.0))
            .h(px(32.0))
            .px_0()
            .rounded(px(6.0))
            .child(div().when(self.disabled, |track| track.opacity(0.4)).child(track))
            .when_some(self.on_click, |button, on_click| {
                button.on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    on_click(&!checked, window, cx);
                })
            });
        div()
            .id(group.clone())
            .group(group)
            // A containing settings row can focus or toggle itself. Let Button
            // handle activation first, then keep the same gesture inside the switch.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(|event, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                }
            })
            .child(button)
    }
}
