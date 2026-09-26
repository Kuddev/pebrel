//! Theme picker foreground-color session.
//!
//! The picker owns only draft state.  Runtime preferences are updated by the
//! outer appearance picker after the user confirms that picker, so dragging the
//! SV square, choosing a swatch, or typing a valid hex value stays reversible.

use crate::gpui_shell::prelude::{ButtonVariant, Input, h_flex, v_flex};
use crate::i18n::Message;
use gpui::accesskit::{Role, Toggled};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Bounds, Context, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement as _, Pixels, Point, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, px,
};
use gpui_component::input::InputEvent;
use nebula_settings::{Rgb8, format_hex_rgb, parse_hex_rgb};
use std::cell::Cell;
use std::rc::Rc;

use super::*;

const SV_H: f32 = 132.0;
const HUE_H: f32 = 16.0;
const GAP: f32 = 8.0;
const SWATCH_SIZE: f32 = 28.0;

/// The same stable recommendation order used by the original foreground dialog.
pub(super) const FOREGROUND_SWATCHES: [[u8; 3]; 10] = [
    [255, 255, 255],
    [245, 247, 250],
    [20, 24, 31],
    [31, 94, 83],
    [42, 91, 140],
    [143, 76, 31],
    [112, 73, 153],
    [187, 53, 66],
    [31, 125, 118],
    [96, 105, 117],
];

pub(super) struct ThemeForegroundState {
    pub(super) hsv: (f32, f32, f32),
    pub(super) drag: Option<crate::display::BgPickerPart>,
    pub(super) sv_bounds: Option<Bounds<Pixels>>,
    pub(super) hue_bounds: Option<Bounds<Pixels>>,
    pub(super) session_seq: u64,
    pub(super) dialog_open: bool,
    before_override: Option<Option<Rgb8>>,
    pub(super) sv_focus: FocusHandle,
    pub(super) hue_focus: FocusHandle,
    pub(super) palette_focus: Vec<FocusHandle>,
}

impl ThemeForegroundState {
    pub(super) fn new(cx: &mut Context<SettingsPane>) -> Self {
        Self {
            hsv: (0.0, 0.0, 1.0),
            drag: None,
            sv_bounds: None,
            hue_bounds: None,
            session_seq: 0,
            dialog_open: false,
            before_override: None,
            sv_focus: cx.focus_handle(),
            hue_focus: cx.focus_handle(),
            palette_focus: (0..FOREGROUND_SWATCHES.len()).map(|_| cx.focus_handle()).collect(),
        }
    }

    fn set_hsv_from_rgb(&mut self, rgb: Rgb8) {
        let (h, s, v) =
            crate::display::rgb_to_hsv(crate::display::color::Rgb::new(rgb[0], rgb[1], rgb[2]));
        if s > f32::EPSILON && v > f32::EPSILON {
            self.hsv = (h, s, v);
        } else {
            // Keep the previous hue for grayscale/black values, matching the
            // background picker and preventing the hue cursor from jumping.
            self.hsv = (self.hsv.0, s, v);
        }
    }

    fn begin(&mut self, current: Rgb8, before_override: Option<Rgb8>) -> u64 {
        self.session_seq = self.session_seq.wrapping_add(1).max(1);
        self.dialog_open = true;
        self.drag = None;
        self.sv_bounds = None;
        self.hue_bounds = None;
        self.before_override = Some(before_override);
        self.set_hsv_from_rgb(current);
        self.session_seq
    }

    fn is_current(&self, sequence: u64) -> bool {
        self.dialog_open && self.session_seq == sequence
    }

    fn finish(&mut self, sequence: u64) {
        if self.session_seq != sequence {
            return;
        }
        self.dialog_open = false;
        self.drag = None;
        self.sv_bounds = None;
        self.hue_bounds = None;
        self.before_override = None;
    }

    fn take_before_override(&mut self, sequence: u64) -> Option<Option<Rgb8>> {
        if self.session_seq != sequence {
            return None;
        }
        self.dialog_open = false;
        self.drag = None;
        self.sv_bounds = None;
        self.hue_bounds = None;
        self.before_override.take()
    }

    pub(super) fn invalidate(&mut self) {
        self.session_seq = self.session_seq.wrapping_add(1).max(1);
        self.dialog_open = false;
        self.drag = None;
        self.sv_bounds = None;
        self.hue_bounds = None;
        self.before_override = None;
    }
}

impl SettingsPane {
    pub(super) fn on_theme_foreground_input_event(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            if matches!(event, InputEvent::Focus | InputEvent::Blur) {
                cx.notify();
            }
            return;
        }
        if self.theme_foreground_input_syncing
            || self
                .appearance_picker
                .as_ref()
                .is_none_or(|picker| picker.apply_busy || !picker.draft.is_theme())
            || !self.theme_foreground_picker.dialog_open
        {
            return;
        }
        let value = self.theme_foreground_input.read(cx).value().to_string();
        let Some(color) = parse_hex_rgb(&value) else { return };
        let Some(original) = self
            .appearance_picker
            .as_ref()
            .and_then(|picker| picker.default_foreground(picker.draft))
        else {
            return;
        };
        if let Some(picker) = self.appearance_picker.as_mut() {
            picker.set_foreground_preview((color != original).then_some(color));
        }
        self.theme_foreground_picker.set_hsv_from_rgb(color);
        // The color dialog is rendered by the workspace's Root layer, outside
        // this pane. Refresh it together with the pane's terminal preview.
        window.refresh();
        cx.notify();
    }

    pub(super) fn set_theme_foreground(
        &mut self,
        color: Rgb8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(original) = self
            .appearance_picker
            .as_ref()
            .and_then(|picker| picker.default_foreground(picker.draft))
        else {
            return;
        };
        if self.appearance_picker.as_ref().is_some_and(|picker| picker.apply_busy) {
            return;
        }
        if let Some(picker) = self.appearance_picker.as_mut() {
            picker.set_foreground_preview((color != original).then_some(color));
        }
        if self.theme_foreground_picker.dialog_open {
            self.theme_foreground_picker.set_hsv_from_rgb(color);
        }
        self.theme_foreground_input_syncing = true;
        self.theme_foreground_input.update(cx, |input, cx| {
            input.set_value(format_hex_rgb(color), window, cx);
        });
        self.theme_foreground_input_syncing = false;
        window.refresh();
        cx.notify();
    }

    pub(super) fn open_theme_foreground_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((current, previous)) = self.appearance_picker.as_ref().and_then(|picker| {
            if picker.apply_busy {
                return None;
            }
            let current = picker
                .foreground_override
                .or_else(|| picker.default_foreground(picker.draft))
                .unwrap_or([255, 255, 255]);
            Some((current, picker.foreground_override))
        }) else {
            return;
        };
        let sequence = self.theme_foreground_picker.begin(current, previous);
        self.write_theme_foreground_input(current, window, cx);

        let language = crate::gpui_shell::config::ui_language(cx);
        let input = self.theme_foreground_input.clone();
        let pane = cx.entity().downgrade();
        let committed = Rc::new(Cell::new(false));

        window.open_dialog(cx, move |dialog, window, dialog_cx| {
            let pane_for_content = pane.clone();
            let pane_for_close = pane.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::ThemePickerColorTitle),
                language.text(Message::ThemePickerColorHint),
                language.text(Message::ThemeTransferCompleted),
                language.text(Message::ThemePickerCancel),
                ButtonVariant::Primary,
            )
            .margin_top(px(((f32::from(window.viewport_size().height) - 560.0) * 0.5).max(16.0)))
            .max_h(px((f32::from(window.viewport_size().height) - 32.0).max(120.0)))
            .content(move |content, window, cx| {
                if let Some(pane) = pane_for_content.upgrade() {
                    content.child(Self::theme_foreground_dialog_body(pane, sequence, window, cx))
                } else {
                    content
                }
            })
            .on_ok({
                let committed = committed.clone();
                let input = input.clone();
                move |_, _, cx| {
                    if parse_hex_rgb(&input.read(cx).value()).is_none() {
                        return false;
                    }
                    committed.set(true);
                    true
                }
            })
            .on_close({
                let committed = committed.clone();
                move |_, window, cx| {
                    let _ = pane_for_close.update(cx, |this, cx| {
                        if committed.get() {
                            this.finish_theme_foreground_session(sequence, cx);
                        } else {
                            this.cancel_theme_foreground_session(sequence, window, cx);
                        }
                    });
                }
            })
        });
    }

    /// Build the controls inside DialogContent so their painted positions and
    /// pointer hitboxes share the modal's content bounds.
    fn theme_foreground_dialog_body(
        pane_entity: Entity<SettingsPane>,
        sequence: u64,
        window: &mut Window,
        dialog_cx: &mut App,
    ) -> gpui::Div {
        let pane = pane_entity.downgrade();
        let language = crate::gpui_shell::config::ui_language(dialog_cx);
        let dialog_text_color = dialog_cx.theme().foreground;
        let dialog_accent_color = dialog_cx.theme().primary;
        let dialog_border_color = dialog_cx.theme().border;
        let input = pane_entity.read(dialog_cx).theme_foreground_input.clone();
        let (hsv, current, sv_focus, hue_focus, palette_focus) =
            pane_entity.read_with(dialog_cx, |this, _| {
                let current = this
                    .appearance_picker
                    .as_ref()
                    .and_then(|picker| picker.foreground_override)
                    .or_else(|| {
                        this.appearance_picker
                            .as_ref()
                            .and_then(|picker| picker.default_foreground(picker.draft))
                    })
                    .unwrap_or([255, 255, 255]);
                (
                    this.theme_foreground_picker.hsv,
                    current,
                    this.theme_foreground_picker.sv_focus.clone(),
                    this.theme_foreground_picker.hue_focus.clone(),
                    this.theme_foreground_picker.palette_focus.clone(),
                )
            });

        let pane_for_sv_bounds = pane.clone();
        let sv = div()
            .id("theme-foreground-sv")
            .debug_selector(|| "theme-foreground-sv".to_owned())
            .w_full()
            .h(px(SV_H))
            .rounded(px(7.0))
            .overflow_hidden()
            .cursor_pointer()
            .track_focus(&sv_focus.clone().tab_stop(true))
            .role(Role::Slider)
            .aria_label(language.text(Message::ThemePickerSaturationValue))
            .when(sv_focus.is_focused(window), |element| {
                element.border_2().border_color(gpui::rgb(0xffffff))
            })
            .on_key_down({
                let pane = pane.clone();
                move |event: &KeyDownEvent, window, cx| {
                    if !matches!(
                        event.keystroke.key.to_ascii_lowercase().as_str(),
                        "left" | "right" | "up" | "down" | "home" | "end"
                    ) {
                        return;
                    }
                    cx.stop_propagation();
                    let _ = pane.update(cx, |this, cx| {
                        this.nudge_theme_foreground(
                            sequence,
                            crate::display::BgPickerPart::Sv,
                            &event.keystroke.key,
                            window,
                            cx,
                        );
                    });
                }
            })
            .on_mouse_down(MouseButton::Left, {
                let pane = pane.clone();
                move |event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let _ = pane.update(cx, |this, cx| {
                        this.begin_theme_foreground_drag(
                            sequence,
                            crate::display::BgPickerPart::Sv,
                            event.position,
                            window,
                            cx,
                        );
                    });
                }
            })
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = pane_for_sv_bounds.update(cx, |this, _| {
                            if this.theme_foreground_picker.is_current(sequence) {
                                this.theme_foreground_picker.sv_bounds = Some(bounds);
                            }
                        });
                    },
                    move |bounds, _, window, _| {
                        background_color::paint_sv(window, bounds, hsv.0, hsv.1, hsv.2);
                    },
                )
                .size_full(),
            );

        let pane_for_hue_bounds = pane.clone();
        let hue = div()
            .id("theme-foreground-hue")
            .debug_selector(|| "theme-foreground-hue".to_owned())
            .w_full()
            .h(px(HUE_H))
            .rounded(px(7.0))
            .overflow_hidden()
            .cursor_pointer()
            .track_focus(&hue_focus.clone().tab_stop(true))
            .role(Role::Slider)
            .aria_label(language.text(Message::ThemePickerHue))
            .when(hue_focus.is_focused(window), |element| {
                element.border_2().border_color(gpui::rgb(0xffffff))
            })
            .on_key_down({
                let pane = pane.clone();
                move |event: &KeyDownEvent, window, cx| {
                    if !matches!(
                        event.keystroke.key.to_ascii_lowercase().as_str(),
                        "left" | "right" | "home" | "end"
                    ) {
                        return;
                    }
                    cx.stop_propagation();
                    let _ = pane.update(cx, |this, cx| {
                        this.nudge_theme_foreground(
                            sequence,
                            crate::display::BgPickerPart::Hue,
                            &event.keystroke.key,
                            window,
                            cx,
                        );
                    });
                }
            })
            .on_mouse_down(MouseButton::Left, {
                let pane = pane.clone();
                move |event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let _ = pane.update(cx, |this, cx| {
                        this.begin_theme_foreground_drag(
                            sequence,
                            crate::display::BgPickerPart::Hue,
                            event.position,
                            window,
                            cx,
                        );
                    });
                }
            })
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = pane_for_hue_bounds.update(cx, |this, _| {
                            if this.theme_foreground_picker.is_current(sequence) {
                                this.theme_foreground_picker.hue_bounds = Some(bounds);
                            }
                        });
                    },
                    move |bounds, _, window, _| {
                        background_color::paint_hue(window, bounds, hsv.0);
                    },
                )
                .size_full(),
            );

        let pane_for_move = pane.clone();
        let pane_for_up = pane.clone();
        let panel = v_flex()
            .id("theme-foreground-picker-panel")
            .debug_selector(|| "theme-foreground-picker-panel".to_owned())
            .w_full()
            .gap(px(GAP))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                let _ = pane_for_move.update(cx, |this, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.apply_theme_foreground_pointer(sequence, event.position, window, cx);
                    } else if this.theme_foreground_picker.is_current(sequence) {
                        this.theme_foreground_picker.drag = None;
                    }
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                let _ = pane_for_up.update(cx, |this, _| {
                    if this.theme_foreground_picker.is_current(sequence) {
                        this.theme_foreground_picker.drag = None;
                    }
                });
            })
            .on_mouse_up_out(MouseButton::Left, {
                let pane = pane.clone();
                move |_, _, cx| {
                    let _ = pane.update(cx, |this, _| {
                        if this.theme_foreground_picker.is_current(sequence) {
                            this.theme_foreground_picker.drag = None;
                        }
                    });
                }
            })
            .child(sv)
            .child(hue);

        let pane_for_cells = pane.clone();
        let focused_swatch = palette_focus.iter().position(|focus| focus.is_focused(window));
        let swatches = h_flex().w_full().flex_wrap().gap(px(GAP)).children(
            FOREGROUND_SWATCHES.iter().copied().enumerate().map(move |(index, color)| {
                let pane = pane_for_cells.clone();
                let focus = palette_focus[index].clone();
                let selected = current == color;
                let label = format!(
                    "{} {}",
                    language.text(Message::ThemePickerTextCustom),
                    format_hex_rgb(color)
                );
                div()
                    .id(SharedString::from(format!("theme-foreground-palette-{index}")))
                    .debug_selector(move || format!("theme-foreground-palette-{index}"))
                    .size(px(SWATCH_SIZE))
                    .rounded_full()
                    .track_focus(&focus.clone().tab_stop(true))
                    .role(Role::RadioButton)
                    .aria_label(label.clone())
                    .aria_toggled(if selected { Toggled::True } else { Toggled::False })
                    .border_1()
                    .border_color(if selected { dialog_accent_color } else { dialog_border_color })
                    .when(focused_swatch == Some(index), |cell| {
                        cell.border_color(gpui::rgb(0xffffff))
                    })
                    .bg(super::rgb_hsla(color[0], color[1], color[2]))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click({
                        let pane = pane.clone();
                        move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = pane.update(cx, |this, cx| {
                                this.set_theme_foreground_for_session(sequence, color, window, cx);
                            });
                        }
                    })
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key.eq_ignore_ascii_case("enter")
                            || event.keystroke.key.eq_ignore_ascii_case("space")
                        {
                            cx.stop_propagation();
                            let _ = pane.update(cx, |this, cx| {
                                this.set_theme_foreground_for_session(sequence, color, window, cx);
                            });
                        }
                    })
            }),
        );

        let body = v_flex()
            .w_full()
            .gap(px(12.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(dialog_text_color)
                    .child(language.text(Message::ThemePickerColorHint)),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(dialog_text_color)
                    .child(language.text(Message::ThemePickerTextSuggestion)),
            )
            .child(
                div()
                    .id("theme-foreground-hex")
                    .debug_selector(|| "theme-foreground-hex".to_owned())
                    .w_full()
                    .border_b_1()
                    .border_color(if input.focus_handle(dialog_cx).is_focused(window) {
                        dialog_accent_color
                    } else {
                        dialog_border_color
                    })
                    .child(
                        Input::new(&input)
                            .w_full()
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false),
                    ),
            )
            .when(parse_hex_rgb(&input.read(dialog_cx).value()).is_none(), |body| {
                body.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(dialog_cx.theme().danger)
                        .child(language.text(Message::ThemePickerInvalidColor)),
                )
            })
            .child(panel)
            .child(swatches);

        body
    }

    fn write_theme_foreground_input(
        &mut self,
        color: Rgb8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_foreground_input_syncing = true;
        self.theme_foreground_input.update(cx, |input, cx| {
            input.set_value(format_hex_rgb(color), window, cx);
        });
        self.theme_foreground_input_syncing = false;
    }

    fn set_theme_foreground_for_session(
        &mut self,
        sequence: u64,
        color: Rgb8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_foreground_picker.is_current(sequence) {
            return;
        }
        self.set_theme_foreground(color, window, cx);
    }

    fn begin_theme_foreground_drag(
        &mut self,
        sequence: u64,
        part: crate::display::BgPickerPart,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_foreground_picker.is_current(sequence) {
            return;
        }
        let focus = match part {
            crate::display::BgPickerPart::Sv => &self.theme_foreground_picker.sv_focus,
            crate::display::BgPickerPart::Hue => &self.theme_foreground_picker.hue_focus,
        };
        window.focus(focus, cx);
        self.theme_foreground_picker.drag = Some(part);
        self.apply_theme_foreground_pointer(sequence, position, window, cx);
    }

    fn apply_theme_foreground_pointer(
        &mut self,
        sequence: u64,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_foreground_picker.is_current(sequence) {
            return;
        }
        let Some(part) = self.theme_foreground_picker.drag else { return };
        let changed = match part {
            crate::display::BgPickerPart::Sv => {
                let Some(bounds) = self.theme_foreground_picker.sv_bounds else { return };
                let w = f32::from(bounds.size.width).max(1.0);
                let h = f32::from(bounds.size.height).max(1.0);
                let x = f32::from(position.x - bounds.origin.x);
                let y = f32::from(position.y - bounds.origin.y);
                self.theme_foreground_picker.hsv.1 = (x / w).clamp(0.0, 1.0);
                self.theme_foreground_picker.hsv.2 = (1.0 - y / h).clamp(0.0, 1.0);
                true
            },
            crate::display::BgPickerPart::Hue => {
                let Some(bounds) = self.theme_foreground_picker.hue_bounds else { return };
                let w = f32::from(bounds.size.width).max(1.0);
                let x = f32::from(position.x - bounds.origin.x);
                self.theme_foreground_picker.hsv.0 = (x / w).clamp(0.0, 1.0) * 360.0;
                true
            },
        };
        if changed {
            let (h, s, v) = self.theme_foreground_picker.hsv;
            let color = crate::display::hsv_to_rgb(h, s, v);
            self.set_theme_foreground_for_session(
                sequence,
                [color.r, color.g, color.b],
                window,
                cx,
            );
        }
    }

    fn nudge_theme_foreground(
        &mut self,
        sequence: u64,
        part: crate::display::BgPickerPart,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_foreground_picker.is_current(sequence) {
            return;
        }
        let key = key.to_ascii_lowercase();
        match part {
            crate::display::BgPickerPart::Sv => match key.as_str() {
                "left" => self.theme_foreground_picker.hsv.1 -= 0.02,
                "right" => self.theme_foreground_picker.hsv.1 += 0.02,
                "down" => self.theme_foreground_picker.hsv.2 -= 0.02,
                "up" => self.theme_foreground_picker.hsv.2 += 0.02,
                "home" => self.theme_foreground_picker.hsv.1 = 0.0,
                "end" => self.theme_foreground_picker.hsv.1 = 1.0,
                _ => return,
            },
            crate::display::BgPickerPart::Hue => match key.as_str() {
                "left" => self.theme_foreground_picker.hsv.0 -= 2.0,
                "right" => self.theme_foreground_picker.hsv.0 += 2.0,
                "home" => self.theme_foreground_picker.hsv.0 = 0.0,
                "end" => self.theme_foreground_picker.hsv.0 = 360.0,
                _ => return,
            },
        }
        self.theme_foreground_picker.hsv.0 = self.theme_foreground_picker.hsv.0.rem_euclid(360.0);
        self.theme_foreground_picker.hsv.1 = self.theme_foreground_picker.hsv.1.clamp(0.0, 1.0);
        self.theme_foreground_picker.hsv.2 = self.theme_foreground_picker.hsv.2.clamp(0.0, 1.0);
        let (h, s, v) = self.theme_foreground_picker.hsv;
        let color = crate::display::hsv_to_rgb(h, s, v);
        self.set_theme_foreground_for_session(sequence, [color.r, color.g, color.b], window, cx);
    }

    fn finish_theme_foreground_session(&mut self, sequence: u64, cx: &mut Context<Self>) {
        if !self.theme_foreground_picker.is_current(sequence) {
            return;
        }
        self.theme_foreground_picker.finish(sequence);
        cx.notify();
    }

    fn cancel_theme_foreground_session(
        &mut self,
        sequence: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(previous) = self.theme_foreground_picker.take_before_override(sequence) else {
            return;
        };
        let Some(color) = self
            .appearance_picker
            .as_ref()
            .and_then(|picker| previous.or_else(|| picker.default_foreground(picker.draft)))
        else {
            return;
        };
        if let Some(picker) = self.appearance_picker.as_mut() {
            picker.foreground_override = previous;
            picker.error = None;
        }
        self.write_theme_foreground_input(color, window, cx);
        self.theme_foreground_picker.set_hsv_from_rgb(color);
        cx.notify();
    }

    pub(super) fn invalidate_theme_foreground_picker(&mut self) {
        self.theme_foreground_picker.invalidate();
    }
}
