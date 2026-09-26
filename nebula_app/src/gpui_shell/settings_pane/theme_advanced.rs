//! Advanced controls for the native theme editor.
//!
//! This module owns only the controls and their session-local values.  The
//! theme editor owns the subscriptions and the actual draft, so opening or
//! closing the advanced section cannot leave listeners attached to the pane.

use std::collections::BTreeMap;

use super::appearance_picker::AppearanceColors;
use super::navigation::rgb_hsla;
use super::*;
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use nebula_settings::{
    MAX_PANE_CARD_DIVIDER, MAX_PANE_CARD_GUTTER, MAX_PANE_CARD_RADIUS, Rgb8, ThemeDefinition,
    format_hex_rgb, parse_hex_rgb,
};

const ANSI_COUNT: usize = 16;

/// All text is supplied by the settings catalog owner.  Keeping resolved
/// labels in the editor session lets this module remain independent of the
/// catalog enum while still requiring typed messages at the call site.
#[derive(Clone)]
pub(super) struct ThemeAdvancedLabels {
    pub(super) title: SharedString,
    pub(super) ansi: SharedString,
    pub(super) selection: SharedString,
    pub(super) selection_background: SharedString,
    pub(super) selection_foreground: SharedString,
    pub(super) cursor_text: SharedString,
    pub(super) inherit: SharedString,
    pub(super) color_placeholder: SharedString,
    pub(super) layout: SharedString,
    pub(super) radius: SharedString,
    pub(super) gutter: SharedString,
    pub(super) shadow: SharedString,
    pub(super) divider: SharedString,
    pub(super) invalid_color: SharedString,
    pub(super) invalid_number: SharedString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ThemeAdvancedField {
    Ansi(usize),
    SelectionBackground,
    SelectionForeground,
    CursorText,
    Radius,
    Gutter,
    Divider,
}

impl ThemeAdvancedField {
    pub(super) const INPUT_FIELDS: [Self; 22] = [
        Self::Ansi(0),
        Self::Ansi(1),
        Self::Ansi(2),
        Self::Ansi(3),
        Self::Ansi(4),
        Self::Ansi(5),
        Self::Ansi(6),
        Self::Ansi(7),
        Self::Ansi(8),
        Self::Ansi(9),
        Self::Ansi(10),
        Self::Ansi(11),
        Self::Ansi(12),
        Self::Ansi(13),
        Self::Ansi(14),
        Self::Ansi(15),
        Self::SelectionBackground,
        Self::SelectionForeground,
        Self::CursorText,
        Self::Radius,
        Self::Gutter,
        Self::Divider,
    ];

    fn is_color(self) -> bool {
        matches!(
            self,
            Self::Ansi(_)
                | Self::SelectionBackground
                | Self::SelectionForeground
                | Self::CursorText
        )
    }
}

/// Session-local advanced editor state.
pub(super) struct ThemeAdvancedEditor {
    pub(super) labels: ThemeAdvancedLabels,
    ansi: [Rgb8; ANSI_COUNT],
    selection_background: Option<Rgb8>,
    selection_foreground: Option<Rgb8>,
    cursor_text: Option<Rgb8>,
    radius: f32,
    gutter: f32,
    divider: f32,
    pub(super) shadow: bool,
    ansi_inputs: Vec<Entity<InputState>>,
    ansi_pickers: Vec<Entity<ColorPickerState>>,
    selection_background_input: Entity<InputState>,
    selection_foreground_input: Entity<InputState>,
    cursor_text_input: Entity<InputState>,
    selection_background_picker: Entity<ColorPickerState>,
    selection_foreground_picker: Entity<ColorPickerState>,
    cursor_text_picker: Entity<ColorPickerState>,
    radius_input: Entity<InputState>,
    gutter_input: Entity<InputState>,
    divider_input: Entity<InputState>,
    picker_subscriptions: Vec<gpui::Subscription>,
    errors: BTreeMap<ThemeAdvancedField, String>,
}

impl ThemeAdvancedEditor {
    pub(super) fn new(
        definition: &ThemeDefinition,
        labels: ThemeAdvancedLabels,
        window: &mut Window,
        cx: &mut Context<SettingsPane>,
    ) -> Self {
        let mut make_input = |value: String, placeholder: SharedString| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        let ansi_inputs: Vec<Entity<InputState>> = (0..ANSI_COUNT)
            .map(|_| make_input(String::new(), labels.color_placeholder.clone()))
            .collect();
        let selection_background_input = make_input(String::new(), labels.inherit.clone());
        let selection_foreground_input = make_input(String::new(), labels.inherit.clone());
        let cursor_text_input = make_input(String::new(), labels.inherit.clone());
        let radius_input = make_input(String::new(), SharedString::from("0..28"));
        let gutter_input = make_input(String::new(), SharedString::from("0..32"));
        let divider_input = make_input(String::new(), SharedString::from("0..4"));
        let mut picker_subscriptions = Vec::with_capacity(ANSI_COUNT + 3);
        let ansi_pickers = (0..ANSI_COUNT)
            .map(|index| {
                new_color_picker(
                    Some(definition.terminal.palette.get(index).unwrap_or([0, 0, 0])),
                    ansi_inputs[index].clone(),
                    window,
                    cx,
                    &mut picker_subscriptions,
                )
            })
            .collect();
        let selection_background_picker = new_color_picker(
            definition.terminal.selection_background,
            selection_background_input.clone(),
            window,
            cx,
            &mut picker_subscriptions,
        );
        let selection_foreground_picker = new_color_picker(
            definition.terminal.selection_foreground,
            selection_foreground_input.clone(),
            window,
            cx,
            &mut picker_subscriptions,
        );
        let cursor_text_picker = new_color_picker(
            definition.terminal.cursor_text,
            cursor_text_input.clone(),
            window,
            cx,
            &mut picker_subscriptions,
        );
        let mut editor = Self {
            labels,
            ansi: [[0; 3]; ANSI_COUNT],
            selection_background: None,
            selection_foreground: None,
            cursor_text: None,
            radius: 0.0,
            gutter: 0.0,
            divider: 0.0,
            shadow: false,
            ansi_inputs,
            ansi_pickers,
            selection_background_input,
            selection_foreground_input,
            cursor_text_input,
            selection_background_picker,
            selection_foreground_picker,
            cursor_text_picker,
            radius_input,
            gutter_input,
            divider_input,
            picker_subscriptions,
            errors: BTreeMap::new(),
        };
        editor.sync_from_definition(definition, window, cx);
        editor
    }

    /// The editor owns one subscription per returned input.  Keeping the list
    /// here makes registration explicit for the editor session that owns it.
    pub(super) const fn input_fields() -> [ThemeAdvancedField; 22] {
        ThemeAdvancedField::INPUT_FIELDS
    }

    pub(super) fn input(&self, field: ThemeAdvancedField) -> Option<&Entity<InputState>> {
        match field {
            ThemeAdvancedField::Ansi(index) => self.ansi_inputs.get(index),
            ThemeAdvancedField::SelectionBackground => Some(&self.selection_background_input),
            ThemeAdvancedField::SelectionForeground => Some(&self.selection_foreground_input),
            ThemeAdvancedField::CursorText => Some(&self.cursor_text_input),
            ThemeAdvancedField::Radius => Some(&self.radius_input),
            ThemeAdvancedField::Gutter => Some(&self.gutter_input),
            ThemeAdvancedField::Divider => Some(&self.divider_input),
        }
    }

    /// Replace all control values after a template/reset/import operation.
    /// Invalid input errors belong to the old draft and therefore disappear.
    pub(super) fn sync_from_definition(
        &mut self,
        definition: &ThemeDefinition,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.ansi = definition.terminal.palette.ansi_colors();
        self.selection_background = definition.terminal.selection_background;
        self.selection_foreground = definition.terminal.selection_foreground;
        self.cursor_text = definition.terminal.cursor_text;
        self.radius = definition.layout.card.radius;
        self.gutter = definition.layout.card.gutter;
        self.divider = definition.layout.card.divider;
        self.shadow = definition.layout.card.shadow;
        self.errors.clear();

        for index in 0..ANSI_COUNT {
            let color = self.ansi[index];
            set_input_value(&self.ansi_inputs[index], format_hex_rgb(color), window, cx);
            sync_color_picker(&self.ansi_pickers[index], Some(color), window, cx);
        }
        set_input_value(
            &self.selection_background_input,
            optional_color_text(self.selection_background),
            window,
            cx,
        );
        set_input_value(
            &self.selection_foreground_input,
            optional_color_text(self.selection_foreground),
            window,
            cx,
        );
        set_input_value(&self.cursor_text_input, optional_color_text(self.cursor_text), window, cx);
        sync_color_picker(&self.selection_background_picker, self.selection_background, window, cx);
        sync_color_picker(&self.selection_foreground_picker, self.selection_foreground, window, cx);
        sync_color_picker(&self.cursor_text_picker, self.cursor_text, window, cx);
        set_input_value(&self.radius_input, format_number(self.radius), window, cx);
        set_input_value(&self.gutter_input, format_number(self.gutter), window, cx);
        set_input_value(&self.divider_input, format_number(self.divider), window, cx);
    }

    /// Read one input after its session-owned subscription receives Change.
    /// A failed parse leaves the last valid value and only records an error.
    pub(super) fn on_input_change(
        &mut self,
        field: ThemeAdvancedField,
        draft: &mut ThemeDefinition,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(input) = self.input(field) else { return false };
        let value = input.read(cx).value().to_string();
        let valid = match field {
            ThemeAdvancedField::Ansi(index) => parse_hex_rgb(value.trim()).is_some_and(|color| {
                self.ansi[index] = color;
                true
            }),
            ThemeAdvancedField::SelectionBackground => parse_optional_color(&value)
                .map(|color| self.selection_background = color)
                .is_some(),
            ThemeAdvancedField::SelectionForeground => parse_optional_color(&value)
                .map(|color| self.selection_foreground = color)
                .is_some(),
            ThemeAdvancedField::CursorText => {
                parse_optional_color(&value).map(|color| self.cursor_text = color).is_some()
            },
            ThemeAdvancedField::Radius => parse_layout_number(&value, 0.0, MAX_PANE_CARD_RADIUS)
                .map(|number| self.radius = number)
                .is_some(),
            ThemeAdvancedField::Gutter => parse_layout_number(&value, 0.0, MAX_PANE_CARD_GUTTER)
                .map(|number| self.gutter = number)
                .is_some(),
            ThemeAdvancedField::Divider => parse_layout_number(&value, 0.0, MAX_PANE_CARD_DIVIDER)
                .map(|number| self.divider = number)
                .is_some(),
        };
        if valid {
            self.errors.remove(&field);
            self.apply_values(draft);
            match field {
                ThemeAdvancedField::Ansi(index) => {
                    sync_color_picker(
                        &self.ansi_pickers[index],
                        Some(self.ansi[index]),
                        window,
                        cx,
                    );
                },
                ThemeAdvancedField::SelectionBackground => {
                    sync_color_picker(
                        &self.selection_background_picker,
                        self.selection_background,
                        window,
                        cx,
                    );
                },
                ThemeAdvancedField::SelectionForeground => {
                    sync_color_picker(
                        &self.selection_foreground_picker,
                        self.selection_foreground,
                        window,
                        cx,
                    );
                },
                ThemeAdvancedField::CursorText => {
                    sync_color_picker(&self.cursor_text_picker, self.cursor_text, window, cx);
                },
                _ => {},
            }
        } else {
            let message = if field.is_color() {
                self.labels.invalid_color.to_string()
            } else {
                self.labels.invalid_number.to_string()
            };
            self.errors.insert(field, message);
        }
        valid
    }

    pub(super) fn toggle_shadow(&mut self, draft: &mut ThemeDefinition) {
        self.shadow = !self.shadow;
        self.apply_values(draft);
    }

    pub(super) fn set_shadow(&mut self, draft: &mut ThemeDefinition, shadow: bool) {
        self.shadow = shadow;
        self.apply_values(draft);
    }

    pub(super) fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.errors.values().next().map(String::as_str)
    }

    /// Apply the last valid advanced snapshot to the editor draft. Save must
    /// call this before serializing; invalid text never reaches the runtime.
    pub(super) fn apply_to(&self, draft: &mut ThemeDefinition) -> Result<(), String> {
        if let Some(error) = self.error() {
            return Err(error.to_owned());
        }
        self.apply_values(draft);
        draft.validate().map_err(|error| error.to_string())
    }

    fn apply_values(&self, draft: &mut ThemeDefinition) {
        for (index, color) in self.ansi.into_iter().enumerate() {
            let _ = draft.terminal.palette.set(index, color);
        }
        draft.terminal.selection_background = self.selection_background;
        draft.terminal.selection_foreground = self.selection_foreground;
        draft.terminal.cursor_text = self.cursor_text;
        draft.layout.card.radius = self.radius;
        draft.layout.card.gutter = self.gutter;
        draft.layout.card.divider = self.divider;
        draft.layout.card.shadow = self.shadow;
    }

    /// Render the advanced editor. The owning editor supplies the shadow
    /// callback so this module never captures a pane or installs a subscription.
    pub(super) fn render<F>(
        &self,
        colors: AppearanceColors,
        error_color: Hsla,
        disabled: bool,
        on_shadow_toggle: F,
    ) -> gpui::Div
    where
        F: Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static,
    {
        let swatch = |picker: &Entity<ColorPickerState>, color: Option<Rgb8>| {
            if disabled {
                div()
                    .size(px(20.0))
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(colors.control)
                    .bg(color.map_or(colors.subtle, |[r, g, b]| rgb_hsla(r, g, b)))
                    .into_any_element()
            } else {
                ColorPicker::new(picker).small().into_any_element()
            }
        };
        let ansi_cell = |index: usize| {
            h_flex()
                .id(("theme-advanced-ansi", index))
                .debug_selector(move || format!("theme-editor-ansi-{index}"))
                .gap(px(6.0))
                .items_center()
                .w_full()
                .child(swatch(&self.ansi_pickers[index], Some(self.ansi[index])))
                .child(
                    div()
                        .w(px(22.0))
                        .text_size(px(10.5))
                        .text_color(colors.secondary)
                        .child(format!("{index:02}")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(30.0))
                        .border_b_1()
                        .border_color(colors.line)
                        .child(
                            Input::new(&self.ansi_inputs[index])
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .disabled(disabled),
                        ),
                )
        };
        let optional_row = |label: SharedString,
                            input: &Entity<InputState>,
                            color: Option<Rgb8>,
                            picker: &Entity<ColorPickerState>| {
            h_flex()
                .gap(px(8.0))
                .items_center()
                .child(swatch(picker, color))
                .child(
                    div()
                        .w(px(112.0))
                        .text_size(px(11.0))
                        .text_color(colors.secondary)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(30.0))
                        .border_b_1()
                        .border_color(colors.line)
                        .child(
                            Input::new(input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .disabled(disabled),
                        ),
                )
        };
        let number_row = |label: SharedString, input: &Entity<InputState>| {
            h_flex()
                .gap(px(8.0))
                .items_center()
                .child(
                    div().w(px(88.0)).text_size(px(11.0)).text_color(colors.secondary).child(label),
                )
                .child(
                    div().w(px(92.0)).h(px(30.0)).border_b_1().border_color(colors.line).child(
                        Input::new(input)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .disabled(disabled),
                    ),
                )
        };
        let shadow = Switch::new("theme-advanced-shadow")
            .checked(self.shadow)
            .disabled(disabled)
            .label(self.labels.shadow.clone())
            .on_click(on_shadow_toggle);
        let error = self.error().map(SharedString::from);

        v_flex()
            .w_full()
            .min_w_0()
            .gap(px(14.0))
            .child(div().text_size(px(13.0)).font_semibold().child(self.labels.title.clone()))
            .child(
                v_flex()
                    .gap(px(7.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.secondary)
                            .child(self.labels.ansi.clone()),
                    )
                    .child(v_flex().gap(px(6.0)).children((0..8).map(|row| {
                        h_flex().gap(px(7.0)).children([ansi_cell(row * 2), ansi_cell(row * 2 + 1)])
                    }))),
            )
            .child(
                v_flex()
                    .gap(px(7.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.secondary)
                            .child(self.labels.selection.clone()),
                    )
                    .child(optional_row(
                        self.labels.selection_background.clone(),
                        &self.selection_background_input,
                        self.selection_background,
                        &self.selection_background_picker,
                    ))
                    .child(optional_row(
                        self.labels.selection_foreground.clone(),
                        &self.selection_foreground_input,
                        self.selection_foreground,
                        &self.selection_foreground_picker,
                    ))
                    .child(optional_row(
                        self.labels.cursor_text.clone(),
                        &self.cursor_text_input,
                        self.cursor_text,
                        &self.cursor_text_picker,
                    )),
            )
            .child(
                v_flex()
                    .gap(px(7.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.secondary)
                            .child(self.labels.layout.clone()),
                    )
                    .child(number_row(self.labels.radius.clone(), &self.radius_input))
                    .child(number_row(self.labels.gutter.clone(), &self.gutter_input))
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .child(number_row(self.labels.divider.clone(), &self.divider_input))
                            .child(shadow),
                    ),
            )
            .when_some(error, |view, error| {
                view.child(div().text_size(px(10.5)).text_color(error_color).child(error))
            })
    }
}

fn new_color_picker(
    color: Option<Rgb8>,
    input: Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<SettingsPane>,
    subscriptions: &mut Vec<gpui::Subscription>,
) -> Entity<ColorPickerState> {
    let picker = cx.new(|cx| {
        let state = ColorPickerState::new(window, cx);
        match color {
            Some([r, g, b]) => state.default_value(rgb_hsla(r, g, b)),
            None => state,
        }
    });
    let subscription = cx.subscribe_in(
        &picker,
        window,
        move |this: &mut SettingsPane, _picker, event: &ColorPickerEvent, window, cx| {
            let Some(editor) = this.theme_editor.as_ref() else { return };
            if editor.save_busy {
                return;
            }
            let ColorPickerEvent::Change(Some(value)) = event else {
                return;
            };
            let rgba: gpui::Rgba = (*value).into();
            let rgb = [
                (rgba.r.clamp(0.0, 1.0) * 255.0).round() as u8,
                (rgba.g.clamp(0.0, 1.0) * 255.0).round() as u8,
                (rgba.b.clamp(0.0, 1.0) * 255.0).round() as u8,
            ];
            input.update(cx, |state, cx| {
                state.set_value(format_hex_rgb(rgb), window, cx);
            });
        },
    );
    subscriptions.push(subscription);
    picker
}

fn sync_color_picker(
    picker: &Entity<ColorPickerState>,
    color: Option<Rgb8>,
    window: &mut Window,
    cx: &mut App,
) {
    picker.update(cx, |state, cx| {
        if let Some([r, g, b]) = color {
            state.set_value(rgb_hsla(r, g, b), window, cx);
        } else {
            *state = ColorPickerState::new(window, cx);
            cx.notify();
        }
    });
}

fn set_input_value(input: &Entity<InputState>, value: String, window: &mut Window, cx: &mut App) {
    input.update(cx, |state, cx| state.set_value(value, window, cx));
}

fn optional_color_text(color: Option<Rgb8>) -> String {
    color.map_or_else(String::new, format_hex_rgb)
}

fn parse_optional_color(value: &str) -> Option<Option<Rgb8>> {
    if value.trim().is_empty() { Some(None) } else { parse_hex_rgb(value.trim()).map(Some) }
}

fn parse_layout_number(value: &str, min: f32, max: f32) -> Option<f32> {
    let value = value.trim().parse::<f32>().ok()?;
    value.is_finite().then_some(value).filter(|value| (min..=max).contains(value))
}

fn format_number(value: f32) -> String {
    format!("{value:.2}")
}

#[cfg(test)]
mod tests {
    use super::{parse_layout_number, parse_optional_color};

    #[test]
    fn optional_colors_accept_blank_as_inherit_but_reject_bad_hex() {
        assert_eq!(parse_optional_color("   "), Some(None));
        assert_eq!(parse_optional_color("#123"), Some(Some([17, 34, 51])));
        assert_eq!(parse_optional_color("not-a-color"), None);
    }

    #[test]
    fn layout_numbers_reject_non_finite_and_out_of_range_values() {
        assert_eq!(parse_layout_number("NaN", 0.0, 28.0), None);
        assert_eq!(parse_layout_number("inf", 0.0, 28.0), None);
        assert_eq!(parse_layout_number("-1", 0.0, 28.0), None);
        assert_eq!(parse_layout_number("29", 0.0, 28.0), None);
        assert_eq!(parse_layout_number(" 8.5 ", 0.0, 28.0), Some(8.5));
    }
}
