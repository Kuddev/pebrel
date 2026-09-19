//! Draft-only controls used by the native theme editor.
//!
//! The settings pane already has a live font picker and a live background
//! picker.  Those controls intentionally persist their result because they
//! edit the application settings page.  The theme editor has a different
//! transaction boundary: its controls may update the preview immediately,
//! but the runtime and preference files change only after Save and Apply.

use super::*;

use gpui::{Bounds, FocusHandle, Pixels};
use nebula_settings::{Rgb8, ThemeDefinition, ThemeUiColors, format_hex_rgb};

/// The four colors shown in the editor's common section.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(in crate::gpui_shell::settings_pane) enum ThemeColorSlot {
    Background,
    Foreground,
    Accent,
    Cursor,
}

impl ThemeColorSlot {
    pub(super) const ALL: [Self; 4] =
        [Self::Background, Self::Foreground, Self::Accent, Self::Cursor];

    pub(super) const fn editor_input(self) -> EditorInput {
        match self {
            Self::Background => EditorInput::Background,
            Self::Foreground => EditorInput::Foreground,
            Self::Accent => EditorInput::Accent,
            Self::Cursor => EditorInput::Cursor,
        }
    }

    pub(super) fn read(self, definition: &ThemeDefinition) -> Rgb8 {
        match self {
            Self::Background => definition.terminal.background,
            Self::Foreground => definition.terminal.foreground,
            Self::Accent => definition.resolved_ui().accent,
            Self::Cursor => definition.terminal.cursor.unwrap_or(definition.terminal.foreground),
        }
    }

    /// Return the stored value, preserving `None` for an inherited cursor.
    pub(super) fn read_override(self, definition: &ThemeDefinition) -> Option<Rgb8> {
        match self {
            Self::Background => Some(definition.terminal.background),
            Self::Foreground => Some(definition.terminal.foreground),
            Self::Accent => Some(definition.resolved_ui().accent),
            Self::Cursor => definition.terminal.cursor,
        }
    }

    pub(super) fn write(self, definition: &mut ThemeDefinition, color: Rgb8) {
        match self {
            Self::Background => definition.terminal.background = color,
            Self::Foreground => definition.terminal.foreground = color,
            Self::Accent => {
                definition.ui = definition.resolved_ui();
                definition.ui.derive = false;
                definition.ui.accent = color;
            },
            Self::Cursor => definition.terminal.cursor = Some(color),
        }
    }

    fn restore(
        self,
        definition: &mut ThemeDefinition,
        original: Option<Rgb8>,
        original_ui: Option<ThemeUiColors>,
    ) {
        match self {
            Self::Background => {
                if let Some(color) = original {
                    definition.terminal.background = color;
                }
            },
            Self::Foreground => {
                if let Some(color) = original {
                    definition.terminal.foreground = color;
                }
            },
            Self::Accent => {
                if let Some(ui) = original_ui {
                    definition.ui = ui;
                } else if let Some(color) = original {
                    definition.ui.accent = color;
                }
            },
            Self::Cursor => definition.terminal.cursor = original,
        }
    }
}

/// A single editor-local color picker transaction.
///
/// `original` is the stored value at open time.  It is intentionally an
/// `Option` so cancelling a cursor picker can restore inherited cursor color
/// semantics instead of turning the inherited value into an explicit one.
#[derive(Clone)]
pub(in crate::gpui_shell::settings_pane) struct ThemeColorPicker {
    pub(super) slot: ThemeColorSlot,
    pub(super) original: Option<Rgb8>,
    pub(super) original_ui: Option<ThemeUiColors>,
    pub(super) original_input_value: String,
    pub(super) original_input_invalid: bool,
    pub(super) current: Rgb8,
    pub(super) hsv: (f32, f32, f32),
    pub(super) sequence: u64,
    pub(super) drag: Option<crate::display::BgPickerPart>,
    pub(super) sv_bounds: Option<Bounds<Pixels>>,
    pub(super) hue_bounds: Option<Bounds<Pixels>>,
    pub(super) sv_focus: FocusHandle,
    pub(super) hue_focus: FocusHandle,
}

impl ThemeColorPicker {
    fn new(
        slot: ThemeColorSlot,
        original: Option<Rgb8>,
        original_ui: Option<ThemeUiColors>,
        original_input_value: String,
        original_input_invalid: bool,
        current: Rgb8,
        sequence: u64,
        sv_focus: FocusHandle,
        hue_focus: FocusHandle,
    ) -> Self {
        Self {
            slot,
            original,
            original_ui,
            original_input_value,
            original_input_invalid,
            current,
            hsv: rgb_to_hsv(current),
            sequence,
            drag: None,
            sv_bounds: None,
            hue_bounds: None,
            sv_focus,
            hue_focus,
        }
    }

    fn set_current(&mut self, color: Rgb8) {
        self.current = color;
        let (h, s, v) = rgb_to_hsv(color);
        // Keep a useful hue marker while editing gray or black colors.  This
        // matches the existing foreground picker and avoids a visual jump.
        self.hsv =
            if s <= f32::EPSILON || v <= f32::EPSILON { (self.hsv.0, s, v) } else { (h, s, v) };
    }
}

/// Convert an RGB color to the picker representation used by the existing
/// GPUI foreground/background controls.
pub(super) fn rgb_to_hsv(color: Rgb8) -> (f32, f32, f32) {
    let color = crate::display::color::Rgb::new(color[0], color[1], color[2]);
    let (h, s, v) = crate::display::rgb_to_hsv(color);
    (h, s, v)
}

pub(super) fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgb8 {
    let color = crate::display::hsv_to_rgb(h, s, v);
    [color.r, color.g, color.b]
}

impl SettingsPane {
    /// Build the selector's values from the cache shared with the regular
    /// settings font picker.  Catalog construction is pure and does not touch
    /// persistence.  The draft's entire chain is kept visible even when a
    /// manually entered fallback is not present in the system catalog.
    pub(super) fn theme_font_options(
        &self,
        current_font: &str,
        draft: &ThemeDefinition,
    ) -> Vec<String> {
        let selected_chain = draft.typography.font_family.as_deref().unwrap_or(current_font);
        let primary = crate::font_install::font_family_chain(selected_chain)
            .first()
            .cloned()
            .unwrap_or_else(|| crate::font_install::REQUIRED_FONT_FAMILY.to_owned());
        let system = self.font_system.as_deref().unwrap_or(&[]);
        let mut options =
            crate::font_install::font_catalog(system, &self.font_imported, true, "", &primary)
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>();

        for family in crate::font_install::font_family_chain(selected_chain) {
            if !options.iter().any(|known| known.eq_ignore_ascii_case(&family)) {
                options.push(family);
            }
        }
        if options.is_empty() {
            options.push(crate::font_install::REQUIRED_FONT_FAMILY.to_owned());
        }
        options
    }

    /// Refresh the theme editor selector after the background font catalog
    /// scan completes or after a template/reset replaces the draft.
    pub(in crate::gpui_shell::settings_pane) fn refresh_theme_font_select(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_font = self.current_font_chain(cx);
        let Some(editor) = self.theme_editor.as_ref() else { return };
        let options = self.theme_font_options(&current_font, &editor.draft);
        let selected_value = editor
            .draft
            .typography
            .font_family
            .as_deref()
            .and_then(|value| crate::font_install::font_family_chain(value).first().cloned())
            .unwrap_or_else(|| {
                crate::font_install::font_family_chain(&current_font)
                    .first()
                    .cloned()
                    .unwrap_or_else(|| crate::font_install::REQUIRED_FONT_FAMILY.to_owned())
            });
        let selected = options
            .iter()
            .position(|family| family.eq_ignore_ascii_case(&selected_value))
            .unwrap_or(0);
        let labels = options.iter().cloned().map(SharedString::from).collect::<Vec<_>>();
        let Some(editor) = self.theme_editor.as_mut() else { return };
        editor.font_options = options;
        editor.font_select.update(cx, |state, cx| {
            state.set_items(labels, window, cx);
            state.set_selected_index(Some(IndexPath::default().row(selected)), window, cx);
        });
    }

    /// Select one family for the draft while preserving a manually entered
    /// fallback chain.  This path deliberately does not call the regular
    /// settings font picker or emit a settings change event.
    pub(super) fn set_theme_font_family(
        &mut self,
        family: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let family = family.trim();
        if family.is_empty() {
            return;
        }
        let Some(editor) = self.theme_editor.as_ref() else { return };
        if editor.save_busy {
            return;
        }
        let current = editor
            .draft
            .typography
            .font_family
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| self.current_font_chain(cx));
        let value = crate::font_install::replace_primary_font_family(&current, family);
        let input = {
            let editor = self.theme_editor.as_mut().expect("theme editor checked above");
            editor.draft.typography.font_family = Some(value.clone());
            editor.input_values.insert(EditorInput::FontFamily, value.clone());
            editor.invalid_inputs.remove(&EditorInput::FontFamily);
            editor.draft_seq = editor.draft_seq.wrapping_add(1);
            editor.saved = false;
            editor.error = if editor.invalid_inputs.is_empty() {
                editor
                    .advanced_editor
                    .as_ref()
                    .and_then(|advanced| advanced.error().map(str::to_owned))
            } else {
                Some(
                    crate::gpui_shell::config::ui_language(cx)
                        .text(crate::i18n::Message::ThemeEditorInvalidValue)
                        .to_owned(),
                )
            };
            editor.font_input.clone()
        };
        input.update(cx, |state, cx| state.set_value(value, window, cx));
        self.refresh_theme_font_select(window, cx);
        cx.notify();
    }

    /// Start a picker session for one common color.  The picker is a view
    /// concern; this method only creates its transaction state.
    pub(super) fn open_theme_color_picker(
        &mut self,
        slot: ThemeColorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_ref() else { return };
        if editor.save_busy {
            return;
        }
        if editor.color_picker.as_ref().is_some_and(|picker| picker.slot == slot) {
            return;
        }
        if editor.color_picker.is_some() {
            self.cancel_theme_color_picker(window, cx);
        }
        let sequence = {
            let Some(editor) = self.theme_editor.as_mut() else { return };
            editor.color_picker_seq = editor.color_picker_seq.wrapping_add(1).max(1);
            let sequence = editor.color_picker_seq;
            let original = slot.read_override(&editor.draft);
            let original_ui = (slot == ThemeColorSlot::Accent).then_some(editor.draft.ui);
            let input = slot.editor_input();
            let original_input_value = editor
                .input_values
                .get(&input)
                .cloned()
                .unwrap_or_else(|| format_hex_rgb(slot.read(&editor.draft)));
            let original_input_invalid = editor.invalid_inputs.contains(&input);
            let current = slot.read(&editor.draft);
            editor.color_picker = Some(ThemeColorPicker::new(
                slot,
                original,
                original_ui,
                original_input_value,
                original_input_invalid,
                current,
                sequence,
                cx.focus_handle(),
                cx.focus_handle(),
            ));
            sequence
        };
        self.show_theme_editor_color_dialog(window, cx);
        window.refresh();
        cx.notify();
    }

    pub(super) fn update_theme_color_picker(
        &mut self,
        color: Rgb8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sequence = self
            .theme_editor
            .as_ref()
            .and_then(|editor| editor.color_picker.as_ref())
            .map(|picker| picker.sequence);
        let Some(sequence) = sequence else { return };
        self.update_theme_color_picker_for_session(sequence, color, window, cx);
    }

    pub(super) fn update_theme_color_picker_for_session(
        &mut self,
        sequence: u64,
        color: Rgb8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_ref() else { return };
        if editor.save_busy {
            return;
        }
        let Some(picker) = editor.color_picker.as_ref() else { return };
        if picker.sequence != sequence || picker.current == color {
            return;
        }
        let slot = picker.slot;
        let input = slot.editor_input();
        let input_entity = {
            let editor = self.theme_editor.as_mut().expect("theme editor checked above");
            slot.write(&mut editor.draft, color);
            editor.input_values.insert(input, format_hex_rgb(color));
            editor.invalid_inputs.remove(&input);
            editor.draft_seq = editor.draft_seq.wrapping_add(1);
            editor.saved = false;
            editor.error = if editor.invalid_inputs.is_empty() {
                editor
                    .advanced_editor
                    .as_ref()
                    .and_then(|advanced| advanced.error().map(str::to_owned))
            } else {
                Some(
                    crate::gpui_shell::config::ui_language(cx)
                        .text(crate::i18n::Message::ThemeEditorInvalidValue)
                        .to_owned(),
                )
            };
            if let Some(picker) = editor.color_picker.as_mut() {
                picker.set_current(color);
            }
            match input {
                EditorInput::Background => editor.background_input.clone(),
                EditorInput::Foreground => editor.foreground_input.clone(),
                EditorInput::Accent => editor.accent_input.clone(),
                EditorInput::Cursor => editor.cursor_input.clone(),
                _ => return,
            }
        };
        input_entity.update(cx, |state, cx| {
            state.set_value(format_hex_rgb(color), window, cx);
        });
        if let Some(editor) = self.theme_editor.as_mut() {
            editor.sync_inherited_cursor_input(window, cx);
        }
        window.refresh();
        cx.notify();
    }

    pub(super) fn cancel_theme_color_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.theme_editor.as_mut().and_then(|editor| editor.color_picker.take())
        else {
            return;
        };
        let input = picker.slot.editor_input();
        let (input_entity, original_input_value) = {
            let editor = self.theme_editor.as_mut().expect("theme editor is open");
            picker.slot.restore(&mut editor.draft, picker.original, picker.original_ui);
            editor.input_values.insert(input, picker.original_input_value.clone());
            if picker.original_input_invalid {
                editor.invalid_inputs.insert(input);
            } else {
                editor.invalid_inputs.remove(&input);
            }
            editor.draft_seq = editor.draft_seq.wrapping_add(1);
            editor.saved = false;
            editor.error = if editor.invalid_inputs.is_empty() {
                editor
                    .advanced_editor
                    .as_ref()
                    .and_then(|advanced| advanced.error().map(str::to_owned))
            } else {
                Some(
                    crate::gpui_shell::config::ui_language(cx)
                        .text(crate::i18n::Message::ThemeEditorInvalidValue)
                        .to_owned(),
                )
            };
            (
                match input {
                    EditorInput::Background => editor.background_input.clone(),
                    EditorInput::Foreground => editor.foreground_input.clone(),
                    EditorInput::Accent => editor.accent_input.clone(),
                    EditorInput::Cursor => editor.cursor_input.clone(),
                    _ => return,
                },
                picker.original_input_value,
            )
        };
        input_entity.update(cx, |state, cx| state.set_value(original_input_value, window, cx));
        if let Some(editor) = self.theme_editor.as_mut() {
            editor.sync_inherited_cursor_input(window, cx);
        }
        window.refresh();
        cx.notify();
    }

    pub(super) fn commit_theme_color_picker(&mut self, cx: &mut Context<Self>) {
        if self.theme_editor.as_mut().and_then(|editor| editor.color_picker.take()).is_some() {
            cx.notify();
        }
    }

    pub(super) fn commit_theme_color_picker_for_session(
        &mut self,
        sequence: u64,
        cx: &mut Context<Self>,
    ) {
        let should_commit = self
            .theme_editor
            .as_ref()
            .and_then(|editor| editor.color_picker.as_ref())
            .is_some_and(|picker| picker.sequence == sequence);
        if should_commit {
            self.commit_theme_color_picker(cx);
        }
    }

    pub(super) fn invalidate_theme_color_picker(&mut self) {
        if let Some(editor) = self.theme_editor.as_mut() {
            editor.color_picker = None;
        }
    }
}

impl ThemeEditor {
    /// Synchronize picker HSV after a user edits the corresponding hex field.
    /// The original snapshot remains untouched, so Cancel still restores the
    /// color from when the picker opened.
    pub(super) fn sync_color_picker_from_input(&mut self, field: EditorInput, color: Rgb8) {
        if let Some(picker) = self.color_picker.as_mut() {
            if picker.slot.editor_input() == field {
                picker.set_current(color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_settings::ThemeName;

    #[test]
    fn hsv_round_trip_keeps_color_close() {
        for color in [[8, 10, 24], [253, 246, 227], [40, 42, 54], [255, 0, 127]] {
            let (h, s, v) = rgb_to_hsv(color);
            let round_trip = hsv_to_rgb(h, s, v);
            assert!(
                color
                    .into_iter()
                    .zip(round_trip)
                    .all(|(left, right)| (i16::from(left) - i16::from(right)).abs() <= 1),
                "{color:?} -> ({h}, {s}, {v}) -> {round_trip:?}"
            );
        }
    }

    #[test]
    fn cancelling_inherited_cursor_restores_none() {
        let mut definition = ThemeDefinition::from_builtin(ThemeName::Nord);
        definition.terminal.cursor = None;
        let original = ThemeColorSlot::Cursor.read_override(&definition);
        ThemeColorSlot::Cursor.write(&mut definition, [255, 0, 0]);
        ThemeColorSlot::Cursor.restore(&mut definition, original, None);
        assert_eq!(definition.terminal.cursor, None);
    }
}
