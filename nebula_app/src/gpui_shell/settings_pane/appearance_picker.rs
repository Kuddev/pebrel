use super::*;
use crate::i18n::Message;
use gpui::accesskit::{Role, Toggled};
use gpui_component::FocusTrapElement as _;
use nebula_settings::AppIconName;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppearanceSelection {
    Theme(ThemeName),
    /// Index into the picker-owned custom theme snapshot. The index is local
    /// to one dialog session, so the selection remains Copy without storing
    /// filesystem strings inside every focus callback.
    Custom(usize),
    Icon(AppIconName),
}

impl AppearanceSelection {
    pub(super) fn is_theme(self) -> bool {
        matches!(self, Self::Theme(_) | Self::Custom(_))
    }

    pub(super) fn label(self, language: crate::display::UiLanguage) -> &'static str {
        match self {
            Self::Theme(theme) => chrome_theme(theme).short_label(),
            Self::Custom(_) => language.text(Message::ThemePickerCustomize),
            Self::Icon(icon) => language.pick(icon.palette().name_zh, icon.palette().name_en),
        }
    }

    pub(super) fn updates(self) -> Vec<(&'static str, String)> {
        match self {
            Self::Theme(theme) => {
                crate::gpui_shell::theme::theme_card_persist_updates(theme).to_vec()
            },
            Self::Custom(_) => Vec::new(),
            Self::Icon(icon) => vec![("app_icon", icon.settings_value().to_owned())],
        }
    }

    pub(super) fn choices(self, filter: usize) -> Vec<Self> {
        match self {
            Self::Theme(_) | Self::Custom(_) => super::theme_picker::THEME_ORDER
                .into_iter()
                .filter(|theme| match filter {
                    1 => chrome_theme(*theme).palette().is_light,
                    2 => !chrome_theme(*theme).palette().is_light,
                    3 => false,
                    _ => true,
                })
                .map(Self::Theme)
                .collect(),
            Self::Icon(_) => AppIconName::ALL
                .into_iter()
                .filter(|icon| filter == 0 || super::app_icon::icon_family(*icon) == filter)
                .map(Self::Icon)
                .collect(),
        }
    }
}

pub(super) struct AppearancePicker {
    pub(super) draft: AppearanceSelection,
    pub(super) initial_draft: AppearanceSelection,
    pub(super) filter: usize,
    pub(super) dark_preview: bool,
    pub(super) focus: FocusHandle,
    pub(super) options: Vec<(AppearanceSelection, FocusHandle)>,
    pub(super) custom_themes: Vec<crate::theme_library::ThemeDocument>,
    pub(super) custom_definitions: Vec<nebula_settings::ThemeDefinition>,
    pub(super) foreground_focus: Vec<FocusHandle>,
    pub(super) custom_loading: bool,
    pub(super) custom_load_error: Option<String>,
    pub(super) apply_busy: bool,
    custom_load_seq: u64,
    preference_revision: Result<crate::theme_library::preferences::PreferenceRevision, String>,
    draft_touched: bool,
    pub(super) error: Option<String>,
    /// A picker-only terminal foreground override. `None` means use the
    /// selected theme's declared foreground; it is persisted only on apply.
    pub(super) foreground_override: Option<nebula_settings::Rgb8>,
    pub(super) initial_foreground_override: Option<nebula_settings::Rgb8>,
}

impl AppearancePicker {
    pub(super) fn set_foreground_preview(&mut self, foreground: Option<[u8; 3]>) {
        self.draft_touched = true;
        self.foreground_override = foreground;
        self.error = None;
    }

    pub(super) fn choices(&self) -> Vec<AppearanceSelection> {
        let mut choices = self.draft.choices(self.filter);
        if self.draft.is_theme() {
            choices.extend(
                self.custom_themes
                    .iter()
                    .enumerate()
                    .filter(|(_, document)| match self.filter {
                        1 => document.appearance() == "light",
                        2 => document.appearance() == "dark",
                        3 => true,
                        _ => true,
                    })
                    .map(|(index, _)| AppearanceSelection::Custom(index)),
            );
        }
        choices
    }

    pub(super) fn choice_label(
        &self,
        choice: AppearanceSelection,
        language: crate::display::UiLanguage,
    ) -> String {
        match choice {
            AppearanceSelection::Custom(index) => self
                .custom_themes
                .get(index)
                .map(|document| document.name().to_owned())
                .unwrap_or_else(|| language.text(Message::ThemePickerCustomize).to_owned()),
            _ => choice.label(language).to_owned(),
        }
    }

    pub(super) fn custom_document(
        &self,
        choice: AppearanceSelection,
    ) -> Option<&crate::theme_library::ThemeDocument> {
        match choice {
            AppearanceSelection::Custom(index) => self.custom_themes.get(index),
            _ => None,
        }
    }

    pub(super) fn default_foreground(
        &self,
        choice: AppearanceSelection,
    ) -> Option<nebula_settings::Rgb8> {
        match choice {
            AppearanceSelection::Theme(theme) => Some(super::theme_picker::theme_foreground(theme)),
            AppearanceSelection::Custom(index) => {
                self.custom_definitions.get(index).map(|definition| definition.terminal.foreground)
            },
            AppearanceSelection::Icon(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct AppearanceColors {
    pub(super) surface: Hsla,
    pub(super) subtle: Hsla,
    pub(super) selected: Hsla,
    pub(super) ink: Hsla,
    pub(super) secondary: Hsla,
    pub(super) muted: Hsla,
    pub(super) line: Hsla,
    pub(super) control: Hsla,
    pub(super) primary: Hsla,
    pub(super) on_primary: Hsla,
    pub(super) scrim: Hsla,
}

impl AppearanceColors {
    pub(super) fn current(cx: &App) -> Self {
        let light = crate::gpui_shell::theme::resolved_palette(cx).is_light;
        let theme = cx.theme();
        Self {
            surface: crate::gpui_shell::theme::settings_panel_bg(cx),
            subtle: crate::gpui_shell::theme::settings_hover_bg(cx, false),
            selected: theme.list_active,
            ink: theme.foreground,
            secondary: theme.muted_foreground,
            muted: crate::gpui_shell::theme::faint_ink(cx),
            line: crate::gpui_shell::theme::settings_hairline(cx),
            control: theme.border,
            primary: theme.primary,
            on_primary: theme.primary_foreground,
            scrim: Hsla::from(gpui::rgb(0x000000)).opacity(if light { 0.30 } else { 0.42 }),
        }
    }
}

pub(super) fn picker_columns(theme: bool, viewport_width: f32) -> usize {
    match (theme, viewport_width <= 390.0) {
        (true, true) => 2,
        (true, false) => 3,
        (false, true) => 4,
        (false, false) => 5,
    }
}

pub(super) fn picker_next_index(
    key: &str,
    current: usize,
    count: usize,
    columns: usize,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let offset = match key {
        "left" => -1,
        "right" => 1,
        "up" => -(columns as isize),
        "down" => columns as isize,
        "home" => return Some(0),
        "end" => return Some(count - 1),
        _ => return None,
    };
    Some((current as isize + offset).rem_euclid(count as isize) as usize)
}

impl SettingsPane {
    pub(super) fn open_appearance_picker(
        &mut self,
        theme: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_font_picker(window, false, cx);
        self.close_background_picker(cx);
        self.appearance_picker_seq = self.appearance_picker_seq.wrapping_add(1);
        let load_seq = self.appearance_picker_seq;
        let draft = if theme {
            AppearanceSelection::Theme(crate::gpui_shell::theme::effective_theme_name(cx))
        } else {
            AppearanceSelection::Icon(crate::app_icon::selected())
        };
        let foreground_override = if theme { self.runtime.theme_foreground } else { None };
        let focus = cx.focus_handle();
        let choices = draft.choices(0);
        let options = choices.into_iter().map(|choice| (choice, cx.focus_handle())).collect();
        let foreground_focus = (0..4).map(|_| cx.focus_handle()).collect();
        window.focus(&focus, cx);
        self.appearance_picker = Some(AppearancePicker {
            draft,
            initial_draft: draft,
            filter: 0,
            dark_preview: false,
            focus,
            options,
            custom_themes: Vec::new(),
            custom_definitions: Vec::new(),
            foreground_focus,
            custom_loading: true,
            custom_load_error: None,
            apply_busy: false,
            custom_load_seq: load_seq,
            preference_revision: Err(crate::gpui_shell::config::ui_language(cx)
                .text(Message::ThemePickerLoading)
                .to_owned()),
            draft_touched: false,
            error: None,
            foreground_override,
            initial_foreground_override: foreground_override,
        });
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (result, revision) = executor
                .spawn(async move {
                    let revision = crate::theme_library::preferences::load()
                        .map_err(|error| error.to_string());
                    let result = if theme {
                        crate::theme_library::ThemeLibraryStore::default()
                            .list()
                            .map(|snapshot| snapshot.custom)
                            .map_err(|error| error.to_string())
                    } else {
                        Ok(Vec::new())
                    };
                    (result, revision)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.finish_custom_theme_load(load_seq, result, revision, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_custom_theme_load(
        &mut self,
        sequence: u64,
        result: Result<Vec<crate::theme_library::ThemeDocument>, String>,
        revision: Result<crate::theme_library::preferences::PreferenceRevision, String>,
        cx: &mut Context<Self>,
    ) {
        let active_custom_id = self.runtime.custom_theme.clone();
        let Some(picker) = self.appearance_picker.as_mut() else { return };
        if picker.custom_load_seq != sequence {
            return;
        }
        picker.custom_loading = false;
        picker.preference_revision = revision;
        match result {
            Ok(custom_themes) => {
                picker.custom_definitions = custom_themes
                    .iter()
                    .filter_map(|document| document.definition().ok())
                    .collect();
                picker.custom_themes = custom_themes;
                picker.custom_load_error = None;
                if picker.draft.is_theme() && !picker.draft_touched {
                    if let Some(active_id) = active_custom_id.as_deref() {
                        if let Some(index) = picker
                            .custom_themes
                            .iter()
                            .position(|document| document.id() == Some(active_id))
                        {
                            picker.draft = AppearanceSelection::Custom(index);
                            picker.initial_draft = picker.draft;
                        }
                    }
                }
                picker.options = picker
                    .choices()
                    .into_iter()
                    .map(|choice| (choice, cx.focus_handle()))
                    .collect();
            },
            Err(error) => {
                picker.custom_load_error = Some(error.clone());
                picker.error = Some(
                    crate::gpui_shell::config::ui_language(cx)
                        .format(Message::ThemeEditorLibraryError, &[("error", &error)]),
                );
            },
        }
        cx.notify();
    }

    fn close_appearance_picker_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_theme_foreground_picker();
        if let Some(picker) = self.appearance_picker.take() {
            let trigger = if picker.draft.is_theme() {
                &self.theme_picker_trigger
            } else {
                &self.icon_picker_trigger
            };
            window.focus(trigger, cx);
            cx.notify();
        }
    }

    fn appearance_picker_dirty(&self) -> bool {
        self.appearance_picker.as_ref().is_some_and(|picker| {
            picker.draft != picker.initial_draft
                || picker.foreground_override != picker.initial_foreground_override
        })
    }

    /// Canceling a picker is deliberately explicit once a theme or text color
    /// has been changed. The modal remains open while the confirmation dialog
    /// is shown, so a mistaken click cannot lose the current draft.
    pub(super) fn close_appearance_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.appearance_picker.as_ref().is_some_and(|picker| picker.apply_busy) {
            return;
        }
        if !self.appearance_picker_dirty() {
            self.close_appearance_picker_now(window, cx);
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
                language.text(Message::ThemePickerDiscardBody),
                language.text(Message::ThemeEditorDiscard),
                language.text(Message::ThemeEditorKeepEditing),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = pane.update(cx, |this, cx| {
                    this.close_appearance_picker_now(window, cx);
                });
                true
            })
        });
    }

    fn apply_appearance_selection(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self
            .appearance_picker
            .as_ref()
            .is_none_or(|picker| picker.apply_busy || picker.custom_loading)
        {
            return;
        }
        let Some((draft, foreground_override)) = self
            .appearance_picker
            .as_ref()
            .map(|picker| (picker.draft, picker.foreground_override))
        else {
            return;
        };
        let updates = match draft {
            AppearanceSelection::Custom(index) => {
                let Some(document) = self
                    .appearance_picker
                    .as_ref()
                    .and_then(|picker| picker.custom_themes.get(index))
                    .cloned()
                else {
                    return;
                };
                match crate::theme_library::preferences::custom_theme_updates(
                    &document,
                    foreground_override,
                ) {
                    Ok(updates) => updates,
                    Err(error) => {
                        if let Some(picker) = self.appearance_picker.as_mut() {
                            picker.error = Some(error.to_string());
                        }
                        cx.notify();
                        return;
                    },
                }
            },
            AppearanceSelection::Theme(theme) => {
                let mut updates = AppearanceSelection::Theme(theme).updates();
                // Selecting a built-in theme leaves the custom library
                // namespace; otherwise the stale custom_theme reference would
                // win during the next runtime snapshot.
                updates.push(("custom_theme", String::new()));
                updates.push((
                    "theme_foreground",
                    foreground_override.map(nebula_settings::format_hex_rgb).unwrap_or_default(),
                ));
                updates
            },
            AppearanceSelection::Icon(icon) => AppearanceSelection::Icon(icon).updates(),
        };
        let session_seq = self
            .appearance_picker
            .as_ref()
            .map(|picker| picker.custom_load_seq)
            .unwrap_or_default();
        if let Some(picker) = self.appearance_picker.as_mut() {
            picker.apply_busy = true;
            picker.error = None;
        }
        let revision =
            self.appearance_picker.as_ref().expect("picker is open").preference_revision.clone();
        let runtime_updates = updates.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = executor
                .spawn(async move {
                    let revision = revision?;
                    crate::theme_library::preferences::save(&revision, &updates)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_appearance_apply(
                    session_seq,
                    draft,
                    runtime_updates,
                    result,
                    window,
                    cx,
                );
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_appearance_apply(
        &mut self,
        session_seq: u64,
        draft: AppearanceSelection,
        updates: Vec<(&'static str, String)>,
        result: Result<crate::theme_library::preferences::PreferenceRevision, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .appearance_picker
            .as_ref()
            .is_some_and(|picker| picker.custom_load_seq == session_seq);
        if !current {
            return;
        }
        if let Some(picker) = self.appearance_picker.as_mut() {
            picker.apply_busy = false;
        }
        match result {
            Ok(_) => {
                self.apply_persisted_runtime(&updates, cx);
                if draft.is_theme() {
                    self.sync_background_color_picker(window, cx);
                }
                self.close_appearance_picker_now(window, cx);
            },
            Err(error) => {
                let language = crate::gpui_shell::config::ui_language(cx);
                if let Some(picker) = self.appearance_picker.as_mut() {
                    picker.error =
                        Some(language.format(Message::ThemePickerSaveError, &[("error", &error)]));
                }
                cx.notify();
            },
        }
    }

    pub(super) fn intercept_appearance_picker(
        &mut self,
        event: &gpui::KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.appearance_picker.as_ref() else {
            return;
        };
        if !picker.focus.contains_focused(window, cx) {
            return;
        }
        let key = event.keystroke.key.to_ascii_lowercase();
        if key == "escape" {
            cx.stop_propagation();
            self.close_appearance_picker(window, cx);
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            cx.stop_propagation();
            return;
        }
        let visible = picker.choices();
        let current = visible.iter().position(|choice| {
            picker
                .options
                .iter()
                .any(|(option, focus)| option == choice && focus.is_focused(window))
        });
        let Some(current) = current else {
            return;
        };
        let columns =
            picker_columns(picker.draft.is_theme(), f32::from(window.viewport_size().width));
        if let Some(next) = picker_next_index(&key, current, visible.len(), columns) {
            cx.stop_propagation();
            self.choose_appearance_draft(visible[next], window, cx);
        } else if matches!(key.as_str(), "space" | "enter") {
            cx.stop_propagation();
            self.choose_appearance_draft(visible[current], window, cx);
        }
    }

    fn choose_appearance_draft(
        &mut self,
        choice: AppearanceSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(picker) = self.appearance_picker.as_mut() {
            if picker.apply_busy {
                return;
            }
            picker.draft_touched = true;
            picker.draft = choice;
            picker.error = None;
            if choice.is_theme() {
                // A theme foreground override is scoped to the selected
                // theme. Picking another template starts at that template's
                // own readable foreground, just like the first color ball.
                picker.foreground_override = None;
            }
            if let Some((_, focus)) = picker.options.iter().find(|(option, _)| *option == choice) {
                window.focus(focus, cx);
            }
            cx.notify();
        }
    }

    pub(super) fn appearance_option(
        &self,
        choice: AppearanceSelection,
        width: f32,
        content: impl IntoElement,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let picker = self.appearance_picker.as_ref().unwrap();
        let selected = choice == picker.draft;
        let visible = picker.choices();
        let tab_choice = visible
            .iter()
            .copied()
            .find(|choice| *choice == picker.draft)
            .or_else(|| visible.first().copied())
            .unwrap_or(picker.draft);
        let focus = &picker.options.iter().find(|(option, _)| *option == choice).unwrap().1;
        let colors = AppearanceColors::current(cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let label = picker.choice_label(choice, language);
        div()
            .id(SharedString::from(format!("appearance-option-{label}")))
            .track_focus(&focus.clone().tab_stop(choice == tab_choice))
            .role(Role::RadioButton)
            .aria_label(label)
            .aria_toggled(if selected { Toggled::True } else { Toggled::False })
            .w(px(width))
            .flex_shrink_0()
            .rounded(px(8.0))
            .border_1()
            .border_color(if selected || focus.is_focused(window) {
                colors.primary
            } else {
                gpui::transparent_black()
            })
            .when(selected, |option| option.bg(colors.subtle))
            .hover(move |option| option.bg(colors.subtle))
            .cursor_pointer()
            .child(content)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.choose_appearance_draft(choice, window, cx);
            }))
            .into_any_element()
    }

    fn appearance_filters(&self, compact: bool, cx: &mut Context<Self>) -> gpui::Div {
        let picker = self.appearance_picker.as_ref().unwrap();
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let filters: &[Message] = if picker.draft.is_theme() {
            &[
                Message::ThemePickerAll,
                Message::ThemePickerLight,
                Message::ThemePickerDark,
                Message::ThemePickerCustom,
            ]
        } else {
            &[
                Message::ThemePickerAll,
                Message::ThemePickerNeutral,
                Message::ThemePickerBlue,
                Message::ThemePickerViolet,
                Message::ThemePickerGreen,
            ]
        };
        let count = picker.choices().len();
        h_flex()
            .mx(px(if compact { 19.0 } else { 27.0 }))
            .pb(px(15.0))
            .gap(px(4.0))
            .border_b_1()
            .border_color(colors.line)
            .flex_shrink_0()
            .children(filters.iter().enumerate().map(|(index, message)| {
                let selected = picker.filter == index;
                Button::new(("appearance-filter", index))
                    .debug_selector(move || format!("appearance-filter-{index}"))
                    .role(Role::RadioButton)
                    .toggled(selected)
                    .label(language.text(*message))
                    .ghost()
                    .h(px(27.0))
                    .px(px(if compact { 7.0 } else { 10.0 }))
                    .text_size(px(11.0))
                    .rounded_full()
                    .text_color(if selected { colors.ink } else { colors.secondary })
                    .when(selected, |button| button.bg(colors.selected).font_semibold())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(picker) = this.appearance_picker.as_mut() {
                            if picker.apply_busy {
                                return;
                            }
                            picker.filter = index;
                        }
                        cx.notify();
                    }))
            }))
            .when(!compact, |filters| {
                filters.child(
                    div().flex_1().text_right().text_size(px(10.5)).text_color(colors.muted).child(
                        language.format(
                            if picker.draft.is_theme() {
                                Message::ThemePickerThemeCount
                            } else {
                                Message::ThemePickerIconCount
                            },
                            &[("count", &count.to_string())],
                        ),
                    ),
                )
            })
    }

    pub(super) fn appearance_picker_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let picker = self.appearance_picker.as_ref()?;
        let draft = picker.draft;
        let focus = picker.focus.clone();
        let error = picker.error.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let viewport = window.viewport_size();
        let compact = f32::from(viewport.width) <= 720.0;
        let padding = if compact { 19.0 } else { 27.0 };
        let width = (f32::from(viewport.width) - if compact { 24.0 } else { 40.0 }).min(770.0);
        let height = f32::from(viewport.height) - 48.0;
        let title = if draft.is_theme() {
            language.text(Message::ThemePickerTitle)
        } else {
            language.text(Message::ThemePickerIconTitle)
        };
        let subtitle = if draft.is_theme() {
            language.text(Message::ThemePickerDescription)
        } else {
            language.text(Message::ThemePickerIconDescription)
        };
        let body = if draft.is_theme() {
            self.theme_picker_body(width - 2.0 - padding * 2.0, compact, window, cx)
        } else {
            self.icon_picker_body(width - 2.0 - padding * 2.0, compact, window, cx)
        };
        let apply_label = if picker.apply_busy {
            language.text(Message::ThemeEditorSaving)
        } else if draft.is_theme() {
            language.text(Message::ThemePickerApply)
        } else {
            language.text(Message::ThemePickerApplyIcon)
        };
        let dialog = v_flex()
            .id("appearance-picker-dialog")
            .debug_selector(|| "appearance-picker-dialog".to_owned())
            .role(Role::Dialog)
            .aria_label(title)
            .w(px(width.max(1.0)))
            .max_h(px(height.max(1.0)))
            .flex_shrink_0()
            .rounded(px(14.0))
            .border_1()
            .border_color(colors.control)
            .bg(colors.surface)
            .text_color(colors.ink)
            .shadow_2xl()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .px(px(padding))
                    .pt(px(25.0))
                    .pb(px(21.0))
                    .gap_4()
                    .items_start()
                    .flex_shrink_0()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(5.0))
                            .child(div().text_size(px(19.0)).font_semibold().child(title))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(colors.secondary)
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        Button::new("close-appearance-picker")
                            .debug_selector(|| "close-appearance-picker".to_owned())
                            .icon(IconName::Close)
                            .ghost()
                            .size(px(28.0))
                            .text_color(colors.secondary)
                            .tooltip(language.text(Message::ThemeTransferClose))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_appearance_picker(window, cx)
                            })),
                    ),
            )
            .child(self.appearance_filters(compact, cx))
            .child(
                div()
                    .id("appearance-picker-scroll")
                    .min_h_0()
                    .flex_shrink(1.0)
                    .overflow_y_scroll()
                    .px(px(padding))
                    .pt(px(21.0))
                    .pb(px(24.0))
                    .child(body),
            )
            .when_some(error, |dialog, error| {
                dialog.child(
                    div()
                        .px(px(padding))
                        .pb_2()
                        .text_size(px(11.0))
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .px(px(padding))
                    .py(px(17.0))
                    .gap(px(9.0))
                    .justify_between()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(colors.line)
                    .child(if draft.is_theme() {
                        Button::new("customize-theme")
                            .debug_selector(|| "customize-theme".to_owned())
                            .icon(IconName::Plus)
                            .label(language.text(Message::ThemePickerCustomize))
                            .disabled(picker.apply_busy || picker.custom_loading)
                            .ghost()
                            .h(px(31.0))
                            .px(px(if compact { 8.0 } else { 10.0 }))
                            .text_size(px(if compact { 10.0 } else { 11.0 }))
                            .rounded_full()
                            .text_color(colors.primary)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_theme_editor(window, cx);
                            }))
                            .into_any_element()
                    } else {
                        h_flex()
                            .min_w_0()
                            .gap(px(7.0))
                            .text_size(px(if compact { 10.0 } else { 11.0 }))
                            .child(
                                div()
                                    .text_color(colors.secondary)
                                    .child(language.text(Message::ThemePickerSelected)),
                            )
                            .child(div().font_medium().truncate().child(draft.label(language)))
                            .into_any_element()
                    })
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .flex_shrink_0()
                            .child(
                                Button::new("cancel-appearance-picker")
                                    .debug_selector(|| "cancel-appearance-picker".to_owned())
                                    .label(language.text(Message::ThemePickerCancel))
                                    .h(px(33.0))
                                    .px(px(if compact { 11.0 } else { 16.0 }))
                                    .text_size(px(12.0))
                                    .rounded(px(6.0))
                                    .bg(colors.surface)
                                    .border_color(colors.control)
                                    .text_color(colors.ink)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_appearance_picker(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("apply-appearance-picker")
                                    .debug_selector(|| "apply-appearance-picker".to_owned())
                                    .label(apply_label)
                                    .h(px(33.0))
                                    .px(px(if compact { 11.0 } else { 16.0 }))
                                    .text_size(px(12.0))
                                    .rounded(px(6.0))
                                    .bg(colors.primary)
                                    .border_color(colors.primary)
                                    .text_color(colors.on_primary)
                                    .disabled(picker.apply_busy || picker.custom_loading)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_appearance_selection(window, cx)
                                    })),
                            ),
                    ),
            )
            .focus_trap("appearance-picker-focus-trap", &focus);
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(gpui::point(px(0.0), px(0.0)))
                    .child(
                        div()
                            .id("appearance-picker-overlay")
                            .w(viewport.width)
                            .h(viewport.height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .occlude()
                            .bg(colors.scrim)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close_appearance_picker(window, cx);
                                }),
                            )
                            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                            .child(dialog),
                    ),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }
}
