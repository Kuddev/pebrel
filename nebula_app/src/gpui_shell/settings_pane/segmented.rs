//! Short fixed choices stay visible and share the existing preference authority.

use super::*;
use gpui::{Animation, AnimationExt as _, ElementId, FontWeight, Pixels, RenderOnce, relative};
use gpui_component::button::ButtonCustomVariant;
use std::{cell::Cell, rc::Rc};

const TRACK_INSET: f32 = 2.0;
const SLIDE_DURATION: Duration = Duration::from_millis(280);

/// Keep the last displayed position, so a second click starts where the thumb is.
struct IndicatorMotion {
    position: Rc<Cell<f32>>,
    from: f32,
    target: f32,
    epoch: u64,
}

#[derive(IntoElement)]
struct SettingsSegments {
    key: &'static str,
    selected: usize,
    height: Pixels,
    labels: Vec<SharedString>,
    fallback: Option<gpui::AnyElement>,
    buttons: Vec<Button>,
}

impl RenderOnce for SettingsSegments {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let font_size = cx.theme().font_size * 0.875;
        let mut font = gpui::font(cx.theme().font_family.clone());
        font.weight = FontWeight::MEDIUM;
        let text_system = window.text_system();
        let slot_width = self
            .labels
            .iter()
            .map(|label| {
                text_system
                    .shape_line(
                        SharedString::from(label.clone()),
                        font_size,
                        &[gpui::TextRun {
                            len: label.len(),
                            font: font.clone(),
                            color: cx.theme().foreground,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    )
                    .width
                    + px(24.0)
            })
            .fold(px(64.0), |width, next| width.max(next));
        let width = slot_width * self.labels.len() as f32;
        // Longer translations retain the full dropdown instead of clipping text.
        if width + px(TRACK_INSET * 2.0) > px(SETTINGS_SELECT_WIDTH) {
            if let Some(fallback) = self.fallback {
                return fallback;
            }
        }
        let key = self.key;
        let count = self.buttons.len() as f32;
        let target = self.selected as f32 / count;
        let panel = crate::gpui_shell::theme::settings_panel_bg(cx);
        // Reuse the active theme's surface ramp; a choice is not a primary action.
        let (track, thumb) = if cx.theme().is_dark() {
            (panel, cx.theme().secondary)
        } else {
            (cx.theme().secondary, panel)
        };
        let motion = window.use_keyed_state(
            SharedString::from(format!("settings-indicator-motion-{key}")),
            cx,
            |_, _| IndicatorMotion {
                position: Rc::new(Cell::new(target)),
                from: target,
                target,
                epoch: 0,
            },
        );
        let (from, epoch, position) = motion.update(cx, |motion, _| {
            if motion.target != target {
                motion.from = motion.position.get();
                motion.target = target;
                motion.epoch += 1;
            }
            (motion.from, motion.epoch, motion.position.clone())
        });
        let indicator = div()
            .id(SharedString::from(format!("settings-indicator-{key}")))
            .debug_selector(move || format!("settings-indicator-{key}"))
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(target))
            .w(relative(1.0 / count))
            .rounded(self.height / 2.0)
            .border_1()
            .border_color(cx.theme().border.opacity(0.7))
            .bg(thumb)
            .shadow(vec![gpui::BoxShadow {
                offset: gpui::point(px(0.0), px(1.0)),
                blur_radius: px(3.0),
                ..crate::gpui_shell::theme::card_shadow(cx)
            }]);
        let indicator = if from == target {
            indicator.into_any_element()
        } else {
            indicator
                .with_animation(
                    ElementId::NamedInteger(format!("settings-slide-{key}").into(), epoch),
                    Animation::new(SLIDE_DURATION).with_easing(|progress| {
                        crate::motion::Easing::CssStandard.sample(progress)
                    }),
                    move |indicator, progress| {
                        let current = from + (target - from) * progress;
                        position.set(current);
                        indicator.left(relative(current))
                    },
                )
                .into_any_element()
        };
        div()
            .id(SharedString::from(format!("settings-choices-{key}")))
            .debug_selector(move || format!("settings-choices-{key}"))
            .w(width + px(TRACK_INSET * 2.0))
            .max_w_full()
            .border_1()
            .border_color(cx.theme().border)
            .p(px(TRACK_INSET - 1.0))
            .rounded(self.height / 2.0 + px(TRACK_INSET))
            .bg(track)
            .child(
                h_flex().relative().w_full().h(self.height).child(indicator).children(self.buttons),
            )
            .into_any_element()
    }
}

impl SettingsPane {
    pub(super) fn segmented_setting(
        &self,
        key: &'static str,
        cx: &Context<Self>,
    ) -> Option<gpui::AnyElement> {
        // Long prose choices (window routing, language, etc.) retain their dropdown.
        if !matches!(
            key,
            "density"
                | "tabs_position"
                | "tab_reveal"
                | "new_tab_position"
                | "vcs_display"
                | "cell_width_mode"
                | "completion_style"
        ) {
            return None;
        }
        let (_, state, values) = self.selects.iter().find(|(candidate, _, _)| *candidate == key)?;
        let selected = state.read(cx).selected_index(cx).map(|index| index.row).unwrap_or(0);
        let labels =
            localized_select_labels(key, values, crate::gpui_shell::config::ui_language(cx));
        let height = (cx.theme().font_size * 2.0).max(px(28.0));
        Some(
            SettingsSegments {
                key,
                selected,
                height,
                labels: labels.clone(),
                fallback: Some(
                    crate::gpui_shell::widgets::settings_select_frame(
                        SharedString::from(format!("settings-segments-dropdown-{key}")),
                        Select::new(state).appearance(false).h_full().rounded(px(6.0)),
                        cx,
                    )
                    .debug_selector(move || format!("settings-segments-dropdown-{key}"))
                    .w(px(SETTINGS_SELECT_WIDTH))
                    .into_any_element(),
                ),
                buttons: values
                    .iter()
                    .copied()
                    .zip(labels)
                    .enumerate()
                    .map(|(index, (value, label))| {
                        let active = index == selected;
                        Button::new(SharedString::from(format!("settings-choice-{key}-{value}")))
                            .debug_selector(move || format!("settings-choice-{key}-{value}"))
                            .flex_1()
                            .min_w_0()
                            .small()
                            .h(height)
                            .px(px(12.0))
                            .rounded(height / 2.0)
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .foreground(if active {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .hover(if active {
                                        cx.theme().transparent
                                    } else {
                                        cx.theme().foreground.opacity(0.04)
                                    })
                                    .active(cx.theme().foreground.opacity(0.08)),
                            )
                            .font_weight(if active {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .toggled(active)
                            .label(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                match this.try_persist(&[(key, value.to_owned())], cx) {
                                    Ok(()) => this.sync_select(key, value, window, cx),
                                    Err(error) => crate::gpui_shell::toast::toast(
                                        window,
                                        cx,
                                        crate::display::ToastKind::Warning,
                                        error.to_string(),
                                    ),
                                }
                            }))
                    })
                    .collect(),
            }
            .into_any_element(),
        )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
