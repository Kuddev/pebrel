//! The editor's color dialog renders local draft controls in the Root modal layer.

use super::*;
use crate::display::BgPickerPart;
use gpui::accesskit::Role;
use gpui::{Bounds, FocusHandle, Pixels, Point, WeakEntity, canvas};
use nebula_settings::Rgb8;
use std::cell::Cell;
use std::rc::Rc;

impl SettingsPane {
    pub(super) fn show_theme_editor_color_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_ref() else { return };
        let Some(picker) = editor.color_picker.as_ref() else { return };
        let sequence = picker.sequence;
        let slot = picker.slot;
        let input = editor_color_input(editor, slot);
        let title = match slot {
            ThemeColorSlot::Background => Message::ThemeEditorBackground,
            ThemeColorSlot::Foreground => Message::ThemeEditorForeground,
            ThemeColorSlot::Accent => Message::ThemeEditorAccent,
            ThemeColorSlot::Cursor => Message::ThemeEditorCursor,
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let pane = cx.entity().downgrade();
        let committed = Rc::new(Cell::new(false));
        window.open_dialog(cx, move |dialog, window, _| {
            let pane_for_body = pane.clone();
            let pane_for_close = pane.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(title),
                language.text(Message::ThemeEditorPreviewOnly),
                language.text(Message::ThemeTransferCompleted),
                language.text(Message::ThemePickerCancel),
                ButtonVariant::Primary,
            )
            .margin_top(px(((f32::from(window.viewport_size().height) - 450.0) * 0.5).max(16.0)))
            .max_h(px((f32::from(window.viewport_size().height) - 32.0).max(120.0)))
            .content(move |content, window, cx| {
                let Some(pane) = pane_for_body.upgrade() else { return content };
                content.child(Self::theme_editor_color_body(pane, sequence, window, cx))
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
                        let current = this
                            .theme_editor
                            .as_ref()
                            .and_then(|editor| editor.color_picker.as_ref())
                            .is_some_and(|picker| picker.sequence == sequence);
                        if !current {
                            return;
                        }
                        if committed.get() {
                            this.commit_theme_color_picker_for_session(sequence, cx);
                        } else {
                            this.cancel_theme_color_picker(window, cx);
                        }
                    });
                    window.refresh();
                }
            })
        });
    }

    fn theme_editor_color_body(
        pane: Entity<Self>,
        sequence: u64,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Div {
        let Some((hsv, current, sv_focus, hue_focus, input)) = pane.read_with(cx, |this, _| {
            let editor = this.theme_editor.as_ref()?;
            let picker = editor.color_picker.as_ref()?;
            (picker.sequence == sequence).then(|| {
                (
                    picker.hsv,
                    picker.current,
                    picker.sv_focus.clone(),
                    picker.hue_focus.clone(),
                    editor_color_input(editor, picker.slot),
                )
            })
        }) else {
            return div();
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let weak = pane.downgrade();
        let sv =
            editor_color_axis(weak.clone(), sequence, BgPickerPart::Sv, hsv, sv_focus, window, cx);
        let hue = editor_color_axis(
            weak.clone(),
            sequence,
            BgPickerPart::Hue,
            hsv,
            hue_focus,
            window,
            cx,
        );
        let move_pane = weak.clone();
        let up_pane = weak.clone();
        let out_pane = weak.clone();
        let input_focused = input.focus_handle(cx).is_focused(window);
        let invalid = parse_hex_rgb(&input.read(cx).value()).is_none();
        v_flex()
            .w_full()
            .gap(px(12.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_move(move |event: &gpui::MouseMoveEvent, window, cx| {
                let _ = move_pane.update(cx, |this, cx| {
                    let drag = this
                        .theme_editor
                        .as_ref()
                        .and_then(|editor| editor.color_picker.as_ref())
                        .filter(|picker| picker.sequence == sequence)
                        .and_then(|picker| picker.drag);
                    if event.pressed_button == Some(MouseButton::Left) {
                        if let Some(part) = drag {
                            this.theme_editor_color_pointer(
                                sequence,
                                part,
                                event.position,
                                window,
                                cx,
                            );
                        }
                    }
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                stop_editor_color_drag(&up_pane, sequence, cx);
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                stop_editor_color_drag(&out_pane, sequence, cx);
            })
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.secondary)
                    .child(language.text(Message::ThemeEditorPreviewOnly)),
            )
            .child(sv)
            .child(hue)
            .child(
                h_flex()
                    .w_full()
                    .gap(px(10.0))
                    .items_center()
                    .child(div().size(px(28.0)).rounded(px(5.0)).bg(theme_color_value(current)))
                    .child(
                        div()
                            .debug_selector(|| "theme-editor-color-hex".to_owned())
                            .flex_1()
                            .min_w_0()
                            .border_b_1()
                            .border_color(if input_focused { colors.primary } else { colors.line })
                            .child(
                                Input::new(&input)
                                    .w_full()
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false),
                            ),
                    ),
            )
            .when(invalid, |body| {
                body.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(cx.theme().danger)
                        .child(language.text(Message::ThemePickerInvalidColor)),
                )
            })
            .child(h_flex().w_full().flex_wrap().gap(px(8.0)).children(
                super::super::theme_foreground::FOREGROUND_SWATCHES.into_iter().enumerate().map(
                    |(index, color)| {
                        let pane = weak.clone();
                        Button::new(SharedString::from(format!(
                            "theme-editor-color-palette-{index}"
                        )))
                        .debug_selector(move || format!("theme-editor-color-palette-{index}"))
                        .size(px(26.0))
                        .rounded_full()
                        .bg(theme_color_value(color))
                        .border_color(if current == color {
                            colors.primary
                        } else {
                            colors.control
                        })
                        .tooltip(format_hex_rgb(color))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = pane.update(cx, |this, cx| {
                                this.update_theme_color_picker_for_session(
                                    sequence, color, window, cx,
                                );
                                window.refresh();
                            });
                        })
                    },
                ),
            ))
    }

    fn theme_editor_color_pointer(
        &mut self,
        sequence: u64,
        part: BgPickerPart,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy {
            return;
        }
        let Some(picker) = editor.color_picker.as_mut() else { return };
        if picker.sequence != sequence {
            return;
        }
        let bounds = match part {
            BgPickerPart::Sv => picker.sv_bounds,
            BgPickerPart::Hue => picker.hue_bounds,
        };
        let Some(bounds) = bounds else { return };
        let x = (f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.0))
            .clamp(0.0, 1.0);
        match part {
            BgPickerPart::Sv => {
                picker.hsv.1 = x;
                picker.hsv.2 = 1.0
                    - (f32::from(position.y - bounds.origin.y)
                        / f32::from(bounds.size.height).max(1.0))
                    .clamp(0.0, 1.0);
            },
            BgPickerPart::Hue => picker.hsv.0 = x * 360.0,
        }
        let (h, s, v) = picker.hsv;
        self.update_theme_color_picker_for_session(
            sequence,
            super::controls::hsv_to_rgb(h, s, v),
            window,
            cx,
        );
        window.refresh();
        cx.notify();
    }
}

fn editor_color_axis(
    pane: WeakEntity<SettingsPane>,
    sequence: u64,
    part: BgPickerPart,
    hsv: (f32, f32, f32),
    focus: FocusHandle,
    window: &Window,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let is_sv = matches!(part, BgPickerPart::Sv);
    let selector = if is_sv { "theme-editor-color-sv" } else { "theme-editor-color-hue" };
    let pane_for_bounds = pane.clone();
    let pane_for_keys = pane.clone();
    let language = crate::gpui_shell::config::ui_language(cx);
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .w_full()
        .h(px(if is_sv { 132.0 } else { 16.0 }))
        .flex_shrink_0()
        .rounded(px(5.0))
        .overflow_hidden()
        .cursor_pointer()
        .track_focus(&focus.clone().tab_stop(true))
        .role(Role::Slider)
        .aria_label(language.text(if is_sv {
            Message::ThemePickerSaturationValue
        } else {
            Message::ThemePickerHue
        }))
        .when(focus.is_focused(window), |axis| axis.border_1().border_color(cx.theme().foreground))
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            let _ = pane.update(cx, |this, cx| {
                let Some(picker) =
                    this.theme_editor.as_mut().and_then(|editor| editor.color_picker.as_mut())
                else {
                    return;
                };
                if picker.sequence != sequence {
                    return;
                }
                picker.drag = Some(part);
                window.focus(&focus, cx);
                this.theme_editor_color_pointer(sequence, part, event.position, window, cx);
            });
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            let key = event.keystroke.key.to_ascii_lowercase();
            if !matches!(key.as_str(), "left" | "right" | "up" | "down" | "home" | "end") {
                return;
            }
            cx.stop_propagation();
            let _ = pane_for_keys.update(cx, |this, cx| {
                let Some(picker) =
                    this.theme_editor.as_mut().and_then(|editor| editor.color_picker.as_mut())
                else {
                    return;
                };
                if picker.sequence != sequence {
                    return;
                }
                let (h, s, v) = &mut picker.hsv;
                if is_sv {
                    match key.as_str() {
                        "left" => *s = (*s - 0.01).max(0.0),
                        "right" => *s = (*s + 0.01).min(1.0),
                        "up" => *v = (*v + 0.01).min(1.0),
                        "down" => *v = (*v - 0.01).max(0.0),
                        "home" => {
                            *s = 0.0;
                            *v = 1.0;
                        },
                        "end" => {
                            *s = 1.0;
                            *v = 0.0;
                        },
                        _ => {},
                    }
                } else {
                    *h = match key.as_str() {
                        "home" => 0.0,
                        "end" => 360.0,
                        "left" | "down" => (*h - 3.0).max(0.0),
                        _ => (*h + 3.0).min(360.0),
                    };
                }
                let color = super::controls::hsv_to_rgb(*h, *s, *v);
                this.update_theme_color_picker_for_session(sequence, color, window, cx);
                window.refresh();
                cx.notify();
            });
        })
        .child(
            canvas(
                move |bounds: Bounds<Pixels>, _, cx| {
                    let _ = pane_for_bounds.update(cx, |this, _| {
                        let Some(picker) = this
                            .theme_editor
                            .as_mut()
                            .and_then(|editor| editor.color_picker.as_mut())
                        else {
                            return;
                        };
                        if picker.sequence != sequence {
                            return;
                        }
                        if is_sv {
                            picker.sv_bounds = Some(bounds);
                        } else {
                            picker.hue_bounds = Some(bounds);
                        }
                    });
                },
                move |bounds, _, window, _| {
                    if is_sv {
                        super::super::background_color::paint_sv(
                            window, bounds, hsv.0, hsv.1, hsv.2,
                        );
                    } else {
                        super::super::background_color::paint_hue(window, bounds, hsv.0);
                    }
                },
            )
            .size_full(),
        )
}

fn stop_editor_color_drag(pane: &WeakEntity<SettingsPane>, sequence: u64, cx: &mut App) {
    let _ = pane.update(cx, |this, _| {
        if let Some(picker) =
            this.theme_editor.as_mut().and_then(|editor| editor.color_picker.as_mut())
        {
            if picker.sequence == sequence {
                picker.drag = None;
            }
        }
    });
}

fn editor_color_input(editor: &ThemeEditor, slot: ThemeColorSlot) -> Entity<InputState> {
    match slot {
        ThemeColorSlot::Background => editor.background_input.clone(),
        ThemeColorSlot::Foreground => editor.foreground_input.clone(),
        ThemeColorSlot::Accent => editor.accent_input.clone(),
        ThemeColorSlot::Cursor => editor.cursor_input.clone(),
    }
}

fn theme_color_value([r, g, b]: [u8; 3]) -> Hsla {
    super::super::rgb_hsla(r, g, b)
}
