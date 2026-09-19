//! Native theme editor surface.
//!
//! The editor owns a concrete draft for the lifetime of the view. Template
//! changes replace that draft with a new snapshot, while the source theme and
//! the persisted runtime stay untouched until an explicit save action.

use super::appearance_picker::{AppearanceColors, AppearancePicker, AppearanceSelection};
use super::theme_advanced::{ThemeAdvancedEditor, ThemeAdvancedField, ThemeAdvancedLabels};
use super::*;
use crate::i18n::Message;
use crate::theme_library::{
    RevisionPrecondition, ThemeDocument, ThemeLibraryStore, from_definition,
};
use nebula_settings::{Rgb8, ThemeDefinition, ThemeName, format_hex_rgb, parse_hex_rgb};
use std::collections::{HashMap, HashSet};

mod color_dialog;
mod controls;
mod view;

pub(super) use controls::{ThemeColorPicker, ThemeColorSlot};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum EditorInput {
    Name,
    Background,
    Foreground,
    Accent,
    Cursor,
    FontFamily,
    FontSize,
    LineHeight,
    Opacity,
}

struct ThemeSaveResult {
    document: ThemeDocument,
    preference_error: Option<String>,
    preference_revision: Option<crate::theme_library::preferences::PreferenceRevision>,
}

#[derive(Clone)]
struct ThemeTemplate {
    definition: ThemeDefinition,
    document: Option<ThemeDocument>,
    label: SharedString,
}

pub(super) struct ThemeEditor {
    session_seq: u64,
    /// The picker that opened this editor. Back restores this exact draft;
    /// closing or applying drops it so the whole workflow ends.
    return_picker: Option<AppearancePicker>,
    pub(super) draft: ThemeDefinition,
    baseline: ThemeDefinition,
    /// The immutable document loaded when this editor opened.  Built-in
    /// templates have no source document. Every template is copied on first
    /// save; later saves retain the new copy's id and revision for optimistic
    /// conflict detection.
    source_document: Option<ThemeDocument>,
    /// The first save always forks the source, including custom templates, so
    /// Save only cannot alter an existing theme or the active runtime snapshot.
    pub(super) fork_source: bool,
    /// Imported documents retain their vendor fields for the draft, but must
    /// always become a new library snapshot on save, even if their envelope
    /// happens to contain an id from another installation.
    source_is_import: bool,
    pub(super) template: ThemeName,
    templates: Vec<ThemeTemplate>,
    templates_loading: bool,
    selected_template: usize,
    pub(super) template_select: Entity<SelectState<Vec<SharedString>>>,
    pub(super) name_input: Entity<InputState>,
    pub(super) background_input: Entity<InputState>,
    pub(super) foreground_input: Entity<InputState>,
    pub(super) accent_input: Entity<InputState>,
    pub(super) cursor_input: Entity<InputState>,
    pub(super) font_input: Entity<InputState>,
    pub(super) font_select: Entity<SelectState<Vec<SharedString>>>,
    pub(super) font_options: Vec<String>,
    pub(super) font_size_input: Entity<InputState>,
    pub(super) line_height_input: Entity<InputState>,
    pub(super) opacity_input: Entity<InputState>,
    pub(super) advanced: bool,
    advanced_editor: Option<ThemeAdvancedEditor>,
    pub(super) preview: usize,
    pub(super) error: Option<String>,
    /// Fields with invalid text are kept separate from the last valid draft
    /// value. This prevents changing another field from accidentally making a
    /// stale color or number eligible for saving.
    invalid_inputs: HashSet<EditorInput>,
    input_values: HashMap<EditorInput, String>,
    subscriptions: Vec<gpui::Subscription>,
    saved: bool,
    pub(super) pending_template: Option<usize>,
    pub(super) save_busy: bool,
    pub(super) color_picker: Option<ThemeColorPicker>,
    color_picker_seq: u64,
    save_seq: u64,
    draft_seq: u64,
    preference_revision: Result<crate::theme_library::preferences::PreferenceRevision, String>,
    pub(super) focus: FocusHandle,
}

impl ThemeEditor {
    fn dirty(&self) -> bool {
        self.source_is_import
            || self.draft != self.baseline
            || !self.invalid_inputs.is_empty()
            || self.advanced_editor.as_ref().is_some_and(ThemeAdvancedEditor::has_errors)
    }

    fn selected_index(&self) -> usize {
        self.selected_template.min(self.templates.len().saturating_sub(1))
    }

    fn sync_inputs(
        &mut self,
        current_font: &str,
        current_size: f32,
        window: &mut Window,
        cx: &mut App,
    ) {
        let values = [
            (EditorInput::Name, &self.name_input, self.draft.name.clone()),
            (
                EditorInput::Background,
                &self.background_input,
                format_hex_rgb(self.draft.terminal.background),
            ),
            (
                EditorInput::Foreground,
                &self.foreground_input,
                format_hex_rgb(self.draft.terminal.foreground),
            ),
            (EditorInput::Accent, &self.accent_input, format_hex_rgb(self.draft.ui.accent)),
            (
                EditorInput::Cursor,
                &self.cursor_input,
                format_hex_rgb(
                    self.draft.terminal.cursor.unwrap_or(self.draft.terminal.foreground),
                ),
            ),
            (
                EditorInput::FontFamily,
                &self.font_input,
                self.draft
                    .typography
                    .font_family
                    .clone()
                    .unwrap_or_else(|| current_font.to_owned()),
            ),
            (
                EditorInput::FontSize,
                &self.font_size_input,
                self.draft
                    .typography
                    .font_size
                    .map_or_else(|| format!("{current_size:.1}"), |value| format!("{value:.1}")),
            ),
            (
                EditorInput::LineHeight,
                &self.line_height_input,
                self.draft
                    .typography
                    .line_height
                    .map_or_else(|| "1.60".to_owned(), |value| format!("{value:.2}")),
            ),
            (
                EditorInput::Opacity,
                &self.opacity_input,
                self.draft.effects.opacity.map_or_else(
                    || "100%".to_owned(),
                    |value| format!("{}%", (value * 100.0).round() as u16),
                ),
            ),
        ];
        for (field, input, value) in values {
            self.input_values.insert(field, value.clone());
            input.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }

    fn sync_inherited_cursor_input(&mut self, window: &mut Window, cx: &mut App) {
        if self.draft.terminal.cursor.is_some()
            || self.invalid_inputs.contains(&EditorInput::Cursor)
        {
            return;
        }
        let value = format_hex_rgb(self.draft.terminal.foreground);
        if self.input_values.get(&EditorInput::Cursor) == Some(&value) {
            return;
        }
        // Record the presentation update before emitting InputEvent::Change,
        // keeping the inherited cursor from becoming an explicit override.
        self.input_values.insert(EditorInput::Cursor, value.clone());
        self.cursor_input.update(cx, |state, cx| state.set_value(value, window, cx));
    }

    fn sync_advanced(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(advanced) = self.advanced_editor.as_mut() {
            advanced.sync_from_definition(&self.draft, window, cx);
        }
    }
}

fn theme_advanced_labels(language: crate::display::UiLanguage) -> ThemeAdvancedLabels {
    ThemeAdvancedLabels {
        title: SharedString::from(language.text(Message::ThemeEditorAdvanced)),
        ansi: SharedString::from(language.text(Message::ThemeEditorAnsi)),
        selection: SharedString::from(language.text(Message::ThemeEditorSelection)),
        selection_background: SharedString::from(
            language.text(Message::ThemeEditorSelectionBackground),
        ),
        selection_foreground: SharedString::from(
            language.text(Message::ThemeEditorSelectionForeground),
        ),
        cursor_text: SharedString::from(language.text(Message::ThemeEditorCursorText)),
        inherit: SharedString::from(language.text(Message::ThemeEditorInherit)),
        color_placeholder: SharedString::from("#rrggbb"),
        layout: SharedString::from(language.text(Message::ThemeEditorLayout)),
        radius: SharedString::from(language.text(Message::ThemeEditorRadius)),
        gutter: SharedString::from(language.text(Message::ThemeEditorGutter)),
        shadow: SharedString::from(language.text(Message::ThemeEditorShadow)),
        divider: SharedString::from(language.text(Message::ThemeEditorDivider)),
        invalid_color: SharedString::from(language.text(Message::ThemeEditorInvalidColor)),
        invalid_number: SharedString::from(language.text(Message::ThemeEditorInvalidNumber)),
    }
}

impl SettingsPane {
    /// Stable name for transfer adapters. Keep the implementation in this
    /// module so export always uses the live editor draft while preserving
    /// vendor fields from an imported or library source document.
    pub(super) fn document_for_draft(&self) -> Result<ThemeDocument, String> {
        let editor =
            self.theme_editor.as_ref().ok_or_else(|| "theme editor is not open".to_owned())?;
        if !editor.invalid_inputs.is_empty()
            || editor.advanced_editor.as_ref().is_some_and(ThemeAdvancedEditor::has_errors)
        {
            return Err("the draft contains invalid inputs".to_owned());
        }
        editor.draft.validate().map_err(|error| error.to_string())?;
        match editor.source_document.as_ref() {
            Some(source) => {
                source.with_definition(&editor.draft).map_err(|error| error.to_string())
            },
            None => from_definition(&editor.draft).map_err(|error| error.to_string()),
        }
    }

    /// Replace the editor snapshot after an import or a custom-template
    /// selection. The document remains a clean editable source, while the
    /// runtime and preference files stay untouched until an explicit apply.
    pub(super) fn replace_theme_editor_draft(
        &mut self,
        document: ThemeDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let definition = document.definition().map_err(|error| error.to_string())?;
        let current_font = self.current_font_chain(cx);
        let current_size = self.terminal_font_size_px(cx);
        let editor =
            self.theme_editor.as_mut().ok_or_else(|| "theme editor is not open".to_owned())?;
        let template = definition.base;
        editor.draft = definition.clone();
        editor.baseline = definition;
        editor.source_document = Some(document.clone());
        editor.fork_source = true;
        editor.source_is_import = true;
        editor.template = template;
        editor.draft_seq = editor.draft_seq.wrapping_add(1);
        editor.saved = false;
        editor.error = None;
        editor.invalid_inputs.clear();
        editor.color_picker = None;
        editor.sync_advanced(window, cx);
        let index = editor.templates.len();
        editor.templates.push(ThemeTemplate {
            definition: editor.draft.clone(),
            label: document.name().to_owned().into(),
            document: Some(document),
        });
        editor.selected_template = index;
        let labels = editor.templates.iter().map(|template| template.label.clone()).collect();
        editor.template_select.update(cx, |state, cx| {
            state.set_items(labels, window, cx);
            state.set_selected_index(Some(IndexPath::default().row(index)), window, cx);
        });
        editor.sync_inputs(&current_font, current_size, window, cx);
        self.refresh_theme_font_select(window, cx);
        cx.notify();
        Ok(())
    }

    /// Stable name for transfer adapters. Import only replaces the draft;
    /// persistence and runtime activation remain owned by the editor buttons.
    pub(super) fn load_imported_theme_editor(
        &mut self,
        document: ThemeDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        document.definition().map_err(|error| error.to_string())?;
        let editor = self.theme_editor.as_ref().ok_or("theme editor is not open")?;
        if editor.save_busy {
            return Err("a theme save is in progress".to_owned());
        }
        if !editor.dirty() {
            return self.replace_theme_editor_draft(document, window, cx);
        }
        let session = editor.session_seq;
        let language = crate::gpui_shell::config::ui_language(cx);
        let pane = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let pane = pane.clone();
            let document = document.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::ThemeEditorChangeTemplateTitle),
                language.text(Message::ThemeEditorChangeTemplateBody),
                language.text(Message::ThemeEditorChangeTemplate),
                language.text(Message::ThemeEditorKeepEditing),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = pane.update(cx, |this, cx| {
                    if this
                        .theme_editor
                        .as_ref()
                        .is_some_and(|editor| editor.session_seq == session)
                    {
                        if let Err(error) =
                            this.replace_theme_editor_draft(document.clone(), window, cx)
                        {
                            if let Some(editor) = this.theme_editor.as_mut() {
                                editor.error = Some(error);
                            }
                        }
                    }
                });
                true
            })
        });
        Ok(())
    }

    pub(super) fn open_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .appearance_picker
            .as_ref()
            .is_some_and(|picker| picker.apply_busy || picker.custom_loading)
        {
            return;
        }
        let selected_source =
            self.appearance_picker.as_ref().and_then(|picker| match picker.draft {
                AppearanceSelection::Custom(index) => picker.custom_themes.get(index).cloned(),
                _ => None,
            });
        let template = self
            .appearance_picker
            .as_ref()
            .and_then(|picker| match picker.draft {
                AppearanceSelection::Theme(theme) => Some(theme),
                AppearanceSelection::Custom(index) => {
                    picker.custom_definitions.get(index).map(|definition| definition.base)
                },
                AppearanceSelection::Icon(_) => None,
            })
            .unwrap_or_else(|| crate::gpui_shell::theme::effective_theme_name(cx));
        let foreground_override =
            self.appearance_picker.as_ref().and_then(|picker| picker.foreground_override);
        let return_picker = self.appearance_picker.take();
        let language = crate::gpui_shell::config::ui_language(cx);
        let preference_revision = Err(language.text(Message::ThemeEditorLoading).to_owned());
        let mut baseline = selected_source
            .as_ref()
            .and_then(|document| document.definition().ok())
            .unwrap_or_else(|| ThemeDefinition::from_builtin(template));
        let original_name = baseline.name.clone();
        baseline.name = language.format(Message::ThemeEditorNewName, &[("name", &original_name)]);
        if baseline.validate().is_err() {
            baseline.name = original_name;
        }
        let mut draft = baseline.clone();
        if let Some(foreground) = foreground_override {
            draft.terminal.foreground = foreground;
        }
        let mut templates = crate::theme_library::builtin_documents()
            .into_iter()
            .filter_map(|document| {
                let definition = document.definition().ok()?;
                Some(ThemeTemplate {
                    label: SharedString::from(document.name().to_owned()),
                    definition,
                    document: None,
                })
            })
            .collect::<Vec<_>>();
        if let Some(document) = selected_source.clone() {
            let already_present = document.id().is_some_and(|id| {
                templates.iter().any(|template| {
                    template.document.as_ref().and_then(ThemeDocument::id) == Some(id)
                })
            });
            if !already_present {
                if let Ok(definition) = document.definition() {
                    templates.push(ThemeTemplate {
                        label: SharedString::from(document.name().to_owned()),
                        definition,
                        document: Some(document),
                    });
                }
            }
        }
        let labels = templates.iter().map(|template| template.label.clone()).collect::<Vec<_>>();
        let selected = selected_source
            .as_ref()
            .and_then(|source| source.id())
            .and_then(|id| {
                templates.iter().position(|template| {
                    template.document.as_ref().and_then(ThemeDocument::id) == Some(id)
                })
            })
            .or_else(|| {
                templates.iter().position(|candidate| candidate.definition.base == template)
            })
            .unwrap_or(0);
        let template_select = cx.new(|cx| {
            SelectState::new(labels, Some(IndexPath::default().row(selected)), window, cx)
        });
        let current_font = self.current_font_chain(cx);
        let current_size = self.terminal_font_size_px(cx);
        let font_options = self.theme_font_options(&current_font, &draft);
        let selected_font = draft
            .typography
            .font_family
            .as_deref()
            .and_then(|value| crate::font_install::font_family_chain(value).first().cloned())
            .or_else(|| crate::font_install::font_family_chain(&current_font).first().cloned())
            .unwrap_or_else(|| crate::font_install::REQUIRED_FONT_FAMILY.to_owned());
        let selected_font_index = font_options
            .iter()
            .position(|family| family.eq_ignore_ascii_case(&selected_font))
            .unwrap_or(0);
        let font_select = cx.new(|cx| {
            SelectState::new(
                font_options.iter().cloned().map(SharedString::from).collect(),
                Some(IndexPath::default().row(selected_font_index)),
                window,
                cx,
            )
        });
        let mut make_input = |value: String, placeholder: &'static str| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        let name_input = make_input(draft.name.clone(), language.text(Message::ThemeEditorName));
        let background_input = make_input(format_hex_rgb(draft.terminal.background), "#rrggbb");
        let foreground_input = make_input(format_hex_rgb(draft.terminal.foreground), "#rrggbb");
        let accent_input = make_input(format_hex_rgb(draft.ui.accent), "#rrggbb");
        let cursor_input = make_input(
            format_hex_rgb(draft.terminal.cursor.unwrap_or(draft.terminal.foreground)),
            "#rrggbb",
        );
        let font_input = make_input(
            draft.typography.font_family.clone().unwrap_or_else(|| current_font.clone()),
            "Cascadia Code",
        );
        let font_size_input = make_input(
            draft
                .typography
                .font_size
                .map_or_else(|| format!("{current_size:.1}"), |value| format!("{value:.1}")),
            "15",
        );
        let line_height_input = make_input(
            draft
                .typography
                .line_height
                .map_or_else(|| "1.60".to_owned(), |value| format!("{value:.2}")),
            "1.60",
        );
        let opacity_input = make_input(
            draft.effects.opacity.map_or_else(
                || "100%".to_owned(),
                |value| format!("{}%", (value * 100.0).round() as u16),
            ),
            "100%",
        );
        let focus = cx.focus_handle();
        let advanced_editor = Some(ThemeAdvancedEditor::new(
            &draft,
            theme_advanced_labels(crate::gpui_shell::config::ui_language(cx)),
            window,
            cx,
        ));
        self.theme_editor_seq = self.theme_editor_seq.wrapping_add(1);
        let editor = ThemeEditor {
            session_seq: self.theme_editor_seq,
            return_picker,
            baseline,
            draft,
            source_document: selected_source.clone(),
            fork_source: true,
            source_is_import: false,
            template,
            templates,
            templates_loading: true,
            selected_template: selected,
            template_select: template_select.clone(),
            name_input: name_input.clone(),
            background_input: background_input.clone(),
            foreground_input: foreground_input.clone(),
            accent_input: accent_input.clone(),
            cursor_input: cursor_input.clone(),
            font_input: font_input.clone(),
            font_select: font_select.clone(),
            font_options,
            font_size_input: font_size_input.clone(),
            line_height_input: line_height_input.clone(),
            opacity_input: opacity_input.clone(),
            advanced: false,
            advanced_editor,
            preview: 0,
            error: None,
            invalid_inputs: HashSet::new(),
            input_values: HashMap::new(),
            subscriptions: Vec::new(),
            saved: false,
            pending_template: None,
            save_busy: false,
            color_picker: None,
            color_picker_seq: 0,
            save_seq: 0,
            draft_seq: 0,
            preference_revision,
            focus: focus.clone(),
        };
        self.theme_editor = Some(editor);
        window.focus(&focus, cx);

        let session_seq = self.theme_editor_seq;
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe_in(
            &template_select,
            window,
            move |this: &mut Self,
                  entity: &Entity<SelectState<Vec<SharedString>>>,
                  event: &SelectEvent<Vec<SharedString>>,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                if this.theme_editor.as_ref().is_none_or(|editor| editor.session_seq != session_seq)
                {
                    return;
                }
                if let SelectEvent::Confirm(Some(_)) = event {
                    let Some(index) = entity.read(cx).selected_index(cx).map(|path| path.row)
                    else {
                        return;
                    };
                    this.request_theme_template(index, window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &font_select,
            window,
            move |this: &mut Self,
                  entity: &Entity<SelectState<Vec<SharedString>>>,
                  event: &SelectEvent<Vec<SharedString>>,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                if this.theme_editor.as_ref().is_none_or(|editor| editor.session_seq != session_seq)
                {
                    return;
                }
                if let SelectEvent::Confirm(Some(_)) = event {
                    let Some(index) = entity.read(cx).selected_index(cx).map(|path| path.row)
                    else {
                        return;
                    };
                    let family = this
                        .theme_editor
                        .as_ref()
                        .and_then(|editor| editor.font_options.get(index).cloned());
                    if let Some(family) = family {
                        this.set_theme_font_family(family, window, cx);
                    }
                }
            },
        ));
        for (input, field) in [
            (name_input, EditorInput::Name),
            (background_input, EditorInput::Background),
            (foreground_input, EditorInput::Foreground),
            (accent_input, EditorInput::Accent),
            (cursor_input, EditorInput::Cursor),
            (font_input, EditorInput::FontFamily),
            (font_size_input, EditorInput::FontSize),
            (line_height_input, EditorInput::LineHeight),
            (opacity_input, EditorInput::Opacity),
        ] {
            if let Some(editor) = self.theme_editor.as_mut() {
                editor.input_values.insert(field, input.read(cx).value().to_string());
            }
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this: &mut Self,
                      _: &Entity<InputState>,
                      event: &InputEvent,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
                    if this
                        .theme_editor
                        .as_ref()
                        .is_none_or(|editor| editor.session_seq != session_seq)
                    {
                        return;
                    }
                    if matches!(event, InputEvent::Change) {
                        this.on_theme_editor_input(field, window, cx);
                    }
                },
            ));
        }
        for field in ThemeAdvancedEditor::input_fields().iter().copied() {
            let input = self
                .theme_editor
                .as_ref()
                .and_then(|editor| editor.advanced_editor.as_ref())
                .and_then(|advanced| advanced.input(field).cloned());
            let Some(input) = input else { continue };
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this: &mut Self,
                      _: &Entity<InputState>,
                      event: &InputEvent,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
                    if this
                        .theme_editor
                        .as_ref()
                        .is_none_or(|editor| editor.session_seq != session_seq)
                    {
                        return;
                    }
                    if matches!(event, InputEvent::Change) {
                        this.on_theme_advanced_input(field, window, cx);
                    }
                },
            ));
        }
        if let Some(editor) = self.theme_editor.as_mut() {
            editor.subscriptions = subscriptions;
        }
        // Reuse the shared asynchronous font catalog.  Its completion callback
        // refreshes this editor's selector without touching settings values.
        self.ensure_font_catalog(cx);
        let template_session =
            self.theme_editor.as_ref().map(|editor| editor.session_seq).unwrap_or_default();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (result, revision) = executor
                .spawn(async move {
                    let revision = crate::theme_library::preferences::load()
                        .map_err(|error| error.to_string());
                    let result = crate::theme_library::ThemeLibraryStore::default()
                        .list()
                        .map(|snapshot| snapshot.custom)
                        .map_err(|error| error.to_string());
                    (result, revision)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_theme_template_load(template_session, result, revision, window, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_theme_template_load(
        &mut self,
        session_seq: u64,
        result: Result<Vec<ThemeDocument>, String>,
        revision: Result<crate::theme_library::preferences::PreferenceRevision, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.session_seq != session_seq {
            return;
        }
        editor.templates_loading = false;
        editor.preference_revision = revision;
        match result {
            Ok(custom_documents) => {
                for document in custom_documents {
                    let duplicate = document.id().is_some_and(|id| {
                        editor.templates.iter().any(|template| {
                            template.document.as_ref().and_then(ThemeDocument::id) == Some(id)
                        })
                    });
                    if duplicate {
                        continue;
                    }
                    let Ok(definition) = document.definition() else { continue };
                    editor.templates.push(ThemeTemplate {
                        label: SharedString::from(document.name().to_owned()),
                        definition,
                        document: Some(document),
                    });
                }
                let labels = editor
                    .templates
                    .iter()
                    .map(|template| template.label.clone())
                    .collect::<Vec<_>>();
                let selected =
                    editor.selected_template.min(editor.templates.len().saturating_sub(1));
                editor.selected_template = selected;
                editor.template_select.update(cx, |state, cx| {
                    state.set_items(labels, window, cx);
                    state.set_selected_index(Some(IndexPath::default().row(selected)), window, cx);
                });
            },
            Err(error) => {
                // Built-in templates remain usable when the optional custom
                // directory cannot be read; surface the diagnostic without
                // turning it into a save-blocking draft error.
                editor.templates_loading = false;
                editor.error = Some(
                    crate::gpui_shell::config::ui_language(cx)
                        .format(Message::ThemeEditorLibraryError, &[("error", &error)]),
                );
            },
        }
        cx.notify();
    }

    fn request_theme_template(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy || index >= editor.templates.len() || index == editor.selected_index() {
            return;
        }
        if !editor.dirty() {
            self.apply_theme_template(index, window, cx);
            return;
        }
        editor.pending_template = Some(index);
        let language = crate::gpui_shell::config::ui_language(cx);
        let pane = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let pane = pane.clone();
            let pane_for_close = pane.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::ThemeEditorChangeTemplateTitle),
                language.text(Message::ThemeEditorChangeTemplateBody),
                language.text(Message::ThemeEditorChangeTemplate),
                language.text(Message::ThemeEditorKeepEditing),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = pane.update(cx, |this, cx| {
                    let index =
                        this.theme_editor.as_ref().and_then(|editor| editor.pending_template);
                    if let Some(index) = index {
                        this.apply_theme_template(index, window, cx);
                    }
                });
                true
            })
            .on_close(move |_, window, cx| {
                let _ = pane_for_close.update(cx, |this, cx| {
                    if let Some(editor) = this.theme_editor.as_mut() {
                        editor.pending_template = None;
                        let selected = editor.selected_index();
                        editor.template_select.update(cx, |state, cx| {
                            state.set_selected_index(
                                Some(IndexPath::default().row(selected)),
                                window,
                                cx,
                            );
                        });
                    }
                });
            })
        });
    }

    fn apply_theme_template(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let current_font = self.current_font_chain(cx);
        let current_size = self.terminal_font_size_px(cx);
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy {
            return;
        }
        let Some(template) = editor.templates.get(index).cloned() else { return };
        let name = editor.draft.name.clone();
        let mut draft = template.definition.clone();
        // The editor names a custom result, so changing a template does not
        // unexpectedly rename the user's in-progress theme.
        draft.name = name;
        editor.template = draft.base;
        editor.selected_template = index;
        editor.source_document = template.document.clone();
        editor.source_is_import = false;
        editor.fork_source = true;
        editor.baseline = draft.clone();
        editor.draft = draft;
        editor.draft_seq = editor.draft_seq.wrapping_add(1);
        editor.saved = false;
        editor.invalid_inputs.clear();
        editor.pending_template = None;
        editor.error = None;
        editor.color_picker = None;
        editor.template_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::default().row(index)), window, cx);
        });
        editor.sync_inputs(&current_font, current_size, window, cx);
        editor.sync_advanced(window, cx);
        self.refresh_theme_font_select(window, cx);
        cx.notify();
    }

    fn on_theme_editor_input(
        &mut self,
        field: EditorInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy {
            return;
        }
        let input = match field {
            EditorInput::Name => &editor.name_input,
            EditorInput::Background => &editor.background_input,
            EditorInput::Foreground => &editor.foreground_input,
            EditorInput::Accent => &editor.accent_input,
            EditorInput::Cursor => &editor.cursor_input,
            EditorInput::FontFamily => &editor.font_input,
            EditorInput::FontSize => &editor.font_size_input,
            EditorInput::LineHeight => &editor.line_height_input,
            EditorInput::Opacity => &editor.opacity_input,
        };
        let value = input.read(cx).value().to_string();
        if editor.input_values.get(&field) == Some(&value) {
            return;
        }
        editor.input_values.insert(field, value.clone());
        let mut candidate = editor.draft.clone();
        let parsed = match field {
            EditorInput::Name => {
                candidate.name = value.clone();
                true
            },
            EditorInput::Background => {
                parse_hex_rgb(&value).map(|color| candidate.terminal.background = color).is_some()
            },
            EditorInput::Foreground => {
                parse_hex_rgb(&value).map(|color| candidate.terminal.foreground = color).is_some()
            },
            EditorInput::Accent => parse_hex_rgb(&value)
                .map(|color| {
                    candidate.ui = candidate.resolved_ui();
                    candidate.ui.derive = false;
                    candidate.ui.accent = color;
                })
                .is_some(),
            EditorInput::Cursor => {
                parse_hex_rgb(&value).map(|color| candidate.terminal.cursor = Some(color)).is_some()
            },
            EditorInput::FontFamily => {
                candidate.typography.font_family =
                    (!value.trim().is_empty()).then(|| value.trim().to_owned());
                true
            },
            EditorInput::FontSize => value
                .trim()
                .parse::<f32>()
                .ok()
                .map(|number| candidate.typography.font_size = Some(number))
                .is_some(),
            EditorInput::LineHeight => value
                .trim()
                .parse::<f32>()
                .ok()
                .map(|number| candidate.typography.line_height = Some(number))
                .is_some(),
            EditorInput::Opacity => value
                .trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()
                .map(|number| candidate.effects.opacity = Some(number / 100.0))
                .is_some(),
        };
        // The shared model is the authority for scalar bounds and finite values.
        // Only a fully valid candidate reaches preview layout or serialization.
        if parsed && candidate.validate().is_ok() {
            editor.draft = candidate;
            editor.invalid_inputs.remove(&field);
            if let Some(color) = parse_hex_rgb(&value) {
                editor.sync_color_picker_from_input(field, color);
            }
        } else {
            editor.invalid_inputs.insert(field);
        }
        editor.draft_seq = editor.draft_seq.wrapping_add(1);
        editor.saved = false;
        editor.error = if !editor.invalid_inputs.is_empty() {
            Some(
                crate::gpui_shell::config::ui_language(cx)
                    .text(Message::ThemeEditorInvalidValue)
                    .to_owned(),
            )
        } else {
            editor.advanced_editor.as_ref().and_then(|advanced| advanced.error().map(str::to_owned))
        };
        editor.sync_inherited_cursor_input(window, cx);
        if editor.color_picker.is_some() {
            window.refresh();
        }
        cx.notify();
    }

    fn on_theme_advanced_input(
        &mut self,
        field: ThemeAdvancedField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy {
            return;
        }
        let Some(advanced) = editor.advanced_editor.as_mut() else { return };
        let valid = advanced.on_input_change(field, &mut editor.draft, window, cx);
        editor.draft_seq = editor.draft_seq.wrapping_add(1);
        editor.saved = false;
        if valid {
            if advanced.has_errors() {
                editor.error = advanced.error().map(str::to_owned);
            } else if editor.invalid_inputs.is_empty() {
                editor.error = None;
            }
        } else {
            editor.error = advanced.error().map(str::to_owned);
        }
        cx.notify();
    }

    fn request_close_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_editor.as_ref().is_some_and(|editor| editor.save_busy) {
            return;
        }
        let dirty = self.theme_editor.as_ref().is_some_and(ThemeEditor::dirty);
        if !dirty {
            self.close_theme_editor(window, cx);
            return;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let pane = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let pane = pane.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::ThemeEditorDiscardTitle),
                language.text(Message::ThemeEditorDiscardBody),
                language.text(Message::ThemeEditorDiscard),
                language.text(Message::ThemeEditorKeepEditing),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = pane.update(cx, |this, cx| this.close_theme_editor(window, cx));
                true
            })
        });
    }

    pub(super) fn request_back_theme_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_editor.as_ref().is_some_and(|editor| editor.save_busy) {
            return;
        }
        let dirty = self.theme_editor.as_ref().is_some_and(ThemeEditor::dirty);
        if !dirty {
            self.back_theme_editor(window, cx);
            return;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let pane = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let pane = pane.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::ThemeEditorDiscardTitle),
                language.text(Message::ThemeEditorDiscardBody),
                language.text(Message::ThemeEditorDiscard),
                language.text(Message::ThemeEditorKeepEditing),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = pane.update(cx, |this, cx| this.back_theme_editor(window, cx));
                true
            })
        });
    }

    fn back_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.theme_editor.take() else { return };
        let Some(picker) = editor.return_picker else {
            window.focus(&self.theme_picker_trigger, cx);
            cx.notify();
            return;
        };
        let focus = picker.focus.clone();
        self.appearance_picker = Some(picker);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn close_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.theme_editor.take() {
            window.focus(&self.theme_picker_trigger, cx);
            let _ = editor;
            cx.notify();
        }
    }

    fn save_theme_editor(&mut self, apply: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy || (apply && editor.templates_loading) {
            return;
        }
        if let Some(advanced) = editor.advanced_editor.as_ref() {
            let mut candidate = editor.draft.clone();
            if let Err(error) = advanced.apply_to(&mut candidate) {
                editor.error = Some(error);
                cx.notify();
                return;
            }
            editor.draft = candidate;
        }
        if !editor.invalid_inputs.is_empty() {
            editor.error = Some(
                crate::gpui_shell::config::ui_language(cx)
                    .text(Message::ThemeEditorInvalidValue)
                    .to_owned(),
            );
            cx.notify();
            return;
        }
        if let Err(error) = editor.draft.validate() {
            editor.error = Some(error.to_string());
            cx.notify();
            return;
        }

        let draft = editor.draft.clone();
        let draft_seq = editor.draft_seq;
        let session_seq = editor.session_seq;
        let source_document = editor.source_document.clone();
        let fork_source = editor.fork_source;
        let source_is_import = editor.source_is_import;
        let preference_revision = editor.preference_revision.clone();
        let draft_document = match from_definition(&draft) {
            Ok(document) => document,
            Err(error) => {
                editor.error = Some(error.to_string());
                cx.notify();
                return;
            },
        };
        // Validate encoding and the document size before moving work to the
        // background executor, so the editor can report a deterministic error.
        if let Err(error) = draft_document.to_json_bytes() {
            editor.error = Some(error.to_string());
            cx.notify();
            return;
        }

        editor.save_seq = editor.save_seq.wrapping_add(1);
        let operation = editor.save_seq;
        editor.save_busy = true;
        editor.error = None;
        let name = draft.name.clone();
        let task = cx.background_executor().spawn(async move {
            let store = ThemeLibraryStore::default();
            let candidate = if let Some(source) = source_document.as_ref() {
                source
                    .with_definition(&draft)
                    .and_then(|document| document.with_name(name.clone()))
                    .map_err(|error| error.to_string())?
            } else {
                draft_document
            };
            let stored = if let Some(source) = source_document {
                if fork_source || source_is_import {
                    store.import(&candidate, Some(&name)).map_err(|error| error.to_string())?
                } else {
                    store
                        .save(&candidate, RevisionPrecondition::Unchanged(source))
                        .map_err(|error| error.to_string())?
                }
            } else {
                store.import(&candidate, Some(&name)).map_err(|error| error.to_string())?
            };

            let (preference_error, preference_revision) = if apply {
                let updates =
                    crate::theme_library::preferences::custom_theme_updates(&stored, None)
                        .map_err(|error| error.to_string())?;
                match preference_revision {
                    Ok(revision) => {
                        match crate::theme_library::preferences::save(&revision, &updates) {
                            Ok(next) => (None, Some(next)),
                            Err(error) => (Some(error.to_string()), None),
                        }
                    },
                    Err(error) => (Some(error), None),
                }
            } else {
                (None, None)
            };
            Ok::<ThemeSaveResult, String>(ThemeSaveResult {
                document: stored,
                preference_error,
                preference_revision,
            })
        });
        let _ = window;
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |pane, window, cx| {
                let Some(editor) = pane.theme_editor.as_mut() else { return };
                if editor.session_seq != session_seq || editor.save_seq != operation {
                    return;
                }
                editor.save_busy = false;
                match result {
                    Err(error) => {
                        editor.error = Some(error);
                        cx.notify();
                    },
                    Ok(saved) => {
                        let apply_now = apply && saved.preference_error.is_none();
                        let preference_error = saved.preference_error;
                        if let Some(revision) = saved.preference_revision {
                            editor.preference_revision = Ok(revision);
                        }
                        let draft_stayed_current = editor.draft_seq == draft_seq;
                        let saved_document = saved.document;
                        let saved_definition =
                            saved_document.definition().expect("stored themes are validated");
                        if let Some(picker) = editor.return_picker.as_mut() {
                            let saved_index = saved_document.id().and_then(|id| {
                                picker
                                    .custom_themes
                                    .iter()
                                    .position(|document| document.id() == Some(id))
                            });
                            let index = if let Some(index) = saved_index {
                                picker.custom_themes[index] = saved_document.clone();
                                if let Some(definition) = picker.custom_definitions.get_mut(index) {
                                    *definition = saved_definition.clone();
                                }
                                index
                            } else {
                                let index = picker.custom_themes.len();
                                picker.custom_themes.push(saved_document.clone());
                                picker.custom_definitions.push(saved_definition.clone());
                                index
                            };
                            let saved_choice = AppearanceSelection::Custom(index);
                            if !picker.options.iter().any(|(choice, _)| *choice == saved_choice) {
                                picker.options.push((saved_choice, cx.focus_handle()));
                            }
                        }
                        editor.source_document = Some(saved_document);
                        editor.fork_source = false;
                        editor.source_is_import = false;
                        editor.baseline = saved_definition.clone();
                        if draft_stayed_current {
                            editor.draft = saved_definition;
                        }
                        editor.saved = true;
                        editor.error = preference_error;
                        let current_font = pane.current_font_chain(cx);
                        let current_size = pane.terminal_font_size_px(cx);
                        if let Some(editor) = pane.theme_editor.as_mut() {
                            editor.sync_inputs(&current_font, current_size, window, cx);
                        }
                        pane.refresh_theme_font_select(window, cx);
                        if apply_now && draft_stayed_current {
                            let (runtime, settings) =
                                crate::gpui_shell::config::Settings::load_current_snapshot(cx);
                            pane.runtime = runtime;
                            gpui_component::set_locale(
                                settings.ui_language.gpui_component_locale(),
                            );
                            cx.set_global(settings);
                            cx.emit(SettingsPaneEvent::Changed);
                            pane.close_theme_editor(window, cx);
                        } else {
                            cx.notify();
                        }
                    },
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn set_theme_advanced_shadow(&mut self, checked: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy {
            return;
        }
        let Some(advanced) = editor.advanced_editor.as_mut() else { return };
        advanced.set_shadow(&mut editor.draft, checked);
        editor.draft_seq = editor.draft_seq.wrapping_add(1);
        editor.saved = false;
        if editor.invalid_inputs.is_empty() && !advanced.has_errors() {
            editor.error = None;
        }
        cx.notify();
    }

    fn reset_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current_font = self.current_font_chain(cx);
        let current_size = self.terminal_font_size_px(cx);
        let Some(editor) = self.theme_editor.as_mut() else { return };
        if editor.save_busy {
            return;
        }
        editor.draft = editor.baseline.clone();
        editor.draft_seq = editor.draft_seq.wrapping_add(1);
        editor.saved = false;
        editor.invalid_inputs.clear();
        editor.error = None;
        editor.color_picker = None;
        editor.sync_inputs(&current_font, current_size, window, cx);
        editor.sync_advanced(window, cx);
        self.refresh_theme_font_select(window, cx);
        cx.notify();
    }
}
