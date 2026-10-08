//! Draft-only wallpaper and window-material controls. No image decode or
//! texture is owned here; applying uses the existing bounded wallpaper loader.

use super::*;
use nebula_settings::BlurModeName;

const BLUR: &[&str] = &["none", "mica", "mica-alt", "aero", "acrylic"];
const FIT: &[&str] = &["fill", "uniform", "uniform_to_fill", "none"];
const ALIGNMENT: &[&str] = &[
    "top_left",
    "top",
    "top_right",
    "left",
    "center",
    "right",
    "bottom_left",
    "bottom",
    "bottom_right",
];

#[derive(Clone, Copy)]
pub(super) enum BackgroundChoice {
    Blur,
    Fit,
    Alignment,
}

impl BackgroundChoice {
    fn values(self) -> &'static [&'static str] {
        match self {
            Self::Blur => BLUR,
            Self::Fit => FIT,
            Self::Alignment => ALIGNMENT,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Blur => "blur",
            Self::Fit => "background_image_fit",
            Self::Alignment => "background_image_alignment",
        }
    }

    fn index(self, draft: &ThemeDefinition) -> usize {
        let value = match self {
            Self::Blur => draft.effects.blur.map(BlurModeName::settings_value),
            Self::Fit => draft.effects.background_image_fit.as_deref(),
            Self::Alignment => draft.effects.background_image_alignment.as_deref(),
        };
        let canonical = match self {
            Self::Fit => value
                .and_then(crate::renderer::image::BackgroundImageFit::parse)
                .map(|fit| fit.settings_value()),
            Self::Alignment => value
                .and_then(crate::renderer::image::BackgroundImageAlignment::parse)
                .map(|alignment| alignment.settings_value()),
            Self::Blur => value,
        };
        canonical
            .and_then(|value| self.values().iter().position(|choice| *choice == value))
            .map_or(0, |index| index + 1)
    }
}

pub(super) struct ThemeBackgroundControls {
    pub(super) image_path: Entity<InputState>,
    pub(super) image_opacity: Entity<InputState>,
    blur: Entity<SelectState<Vec<SharedString>>>,
    fit: Entity<SelectState<Vec<SharedString>>>,
    alignment: Entity<SelectState<Vec<SharedString>>>,
}

impl ThemeBackgroundControls {
    pub(super) fn new(
        draft: &ThemeDefinition,
        window: &mut Window,
        cx: &mut Context<SettingsPane>,
    ) -> Self {
        let language = crate::gpui_shell::config::ui_language(cx);
        let placeholder = language.text(Message::ThemeEditorKeepPersonalSettings);
        let image_path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(draft.effects.background_image.clone().unwrap_or_default())
        });
        let image_opacity = cx.new(|cx| {
            InputState::new(window, cx).placeholder(placeholder).default_value(opacity_text(draft))
        });
        let mut select = |choice: BackgroundChoice| {
            let mut labels = super::super::localization::localized_select_labels(
                choice.key(),
                choice.values(),
                language,
            );
            labels.insert(0, SharedString::from(placeholder));
            cx.new(|cx| {
                SelectState::new(
                    labels,
                    Some(IndexPath::default().row(choice.index(draft))),
                    window,
                    cx,
                )
            })
        };
        Self {
            image_path,
            image_opacity,
            blur: select(BackgroundChoice::Blur),
            fit: select(BackgroundChoice::Fit),
            alignment: select(BackgroundChoice::Alignment),
        }
    }

    pub(super) fn choices(
        &self,
    ) -> [(Entity<SelectState<Vec<SharedString>>>, BackgroundChoice); 3] {
        [
            (self.blur.clone(), BackgroundChoice::Blur),
            (self.fit.clone(), BackgroundChoice::Fit),
            (self.alignment.clone(), BackgroundChoice::Alignment),
        ]
    }

    pub(super) fn sync(&self, draft: &ThemeDefinition, window: &mut Window, cx: &mut App) {
        for (entity, choice) in self.choices() {
            entity.update(cx, |select, cx| {
                select.set_selected_index(
                    Some(IndexPath::default().row(choice.index(draft))),
                    window,
                    cx,
                )
            });
        }
    }
}

fn opacity_text(draft: &ThemeDefinition) -> String {
    draft
        .effects
        .background_image_opacity
        .map_or_else(String::new, |value| format!("{}%", value * 100.0))
}

impl SettingsPane {
    pub(super) fn subscribe_theme_background(
        &mut self,
        session: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::Subscription> {
        let choices = self.theme_editor.as_ref().unwrap().background_controls.choices();
        choices
            .into_iter()
            .map(|(entity, choice)| {
                cx.subscribe_in(
                    &entity,
                    window,
                    move |this: &mut Self,
                          entity,
                          event: &SelectEvent<Vec<SharedString>>,
                          _,
                          cx| {
                        if this
                            .theme_editor
                            .as_ref()
                            .is_none_or(|editor| editor.session_seq != session || editor.save_busy)
                        {
                            return;
                        }
                        if let SelectEvent::Confirm(Some(_)) = event {
                            let Some(index) =
                                entity.read(cx).selected_index(cx).map(|path| path.row)
                            else {
                                return;
                            };
                            let value = index
                                .checked_sub(1)
                                .and_then(|index| choice.values().get(index))
                                .copied();
                            let editor = this.theme_editor.as_mut().unwrap();
                            match choice {
                                BackgroundChoice::Blur => {
                                    editor.draft.effects.blur =
                                        value.and_then(BlurModeName::from_settings)
                                },
                                BackgroundChoice::Fit => {
                                    editor.draft.effects.background_image_fit =
                                        value.map(str::to_owned)
                                },
                                BackgroundChoice::Alignment => {
                                    editor.draft.effects.background_image_alignment =
                                        value.map(str::to_owned)
                                },
                            }
                            editor.saved = false;
                            editor.draft_seq = editor.draft_seq.wrapping_add(1);
                            editor.refresh_background_preview(&this.runtime, cx);
                            cx.notify();
                        }
                    },
                )
            })
            .collect()
    }

    fn set_theme_background_image(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.theme_editor.as_mut().filter(|editor| !editor.save_busy) else {
            return;
        };
        let mut candidate = editor.draft.clone();
        candidate.effects.background_image = Some(path.clone());
        candidate.effects.background_media_kind = Some(nebula_settings::BackgroundMediaKind::Image);
        if candidate.validate().is_err() {
            editor.error = Some(
                crate::gpui_shell::config::ui_language(cx)
                    .text(Message::ThemeEditorInvalidValue)
                    .to_owned(),
            );
            cx.notify();
            return;
        }
        editor.draft.effects.background_image = Some(path.clone());
        editor.draft.effects.background_media_kind = candidate.effects.background_media_kind;
        editor.input_values.insert(EditorInput::ImagePath, path.clone());
        editor.invalid_inputs.remove(&EditorInput::ImagePath);
        editor
            .background_controls
            .image_path
            .update(cx, |input, cx| input.set_value(path, window, cx));
        editor.saved = false;
        editor.draft_seq = editor.draft_seq.wrapping_add(1);
        editor.refresh_background_preview(&self.runtime, cx);
        if editor.invalid_inputs.is_empty()
            && !editor.advanced_editor.as_ref().is_some_and(ThemeAdvancedEditor::has_errors)
        {
            editor.error = None;
        }
        cx.notify();
    }

    fn choose_theme_background_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.theme_editor.as_ref().filter(|editor| !editor.save_busy) else {
            return;
        };
        let session = editor.session_seq;
        let draft_sequence = editor.draft_seq;
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                crate::gpui_shell::config::ui_language(cx)
                    .text(Message::ThemeEditorChooseImage)
                    .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                // Closing, template replacement, or any intervening draft edit
                // invalidates the picker, including after reopening the editor.
                if this.theme_editor.as_ref().is_none_or(|editor| {
                    editor.session_seq != session
                        || editor.draft_seq != draft_sequence
                        || editor.save_busy
                }) {
                    return;
                }
                let Some(path) = path.to_str() else { return };
                this.set_theme_background_image(path.to_owned(), window, cx);
            });
        })
        .detach();
    }

    pub(super) fn theme_editor_background_fields(
        &self,
        editor: &ThemeEditor,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let disabled = editor.save_busy;
        let controls = &editor.background_controls;
        let input = |selector: &'static str, label: &'static str, entity: &Entity<InputState>| {
            let focused = entity.read(cx).focus_handle(cx).is_focused(window);
            let field = if selector == "theme-editor-image-opacity" {
                EditorInput::ImageOpacity
            } else {
                EditorInput::ImagePath
            };
            let invalid = editor.invalid_inputs.contains(&field);
            v_flex()
                .w_full()
                .min_w_0()
                .gap(px(6.0))
                .child(div().text_size(px(13.0)).child(label))
                .child(
                    div()
                        .debug_selector(move || selector.to_owned())
                        .w_full()
                        .h(px(34.0))
                        .border_b_1()
                        .border_color(if invalid {
                            cx.theme().danger
                        } else if focused {
                            colors.primary
                        } else {
                            colors.line
                        })
                        .child(
                            Input::new(entity)
                                .w_full()
                                .h_full()
                                .bordered(false)
                                .focus_bordered(false)
                                .appearance(false)
                                .rounded_none()
                                .disabled(disabled),
                        ),
                )
        };
        let select = |selector: &'static str,
                      label: &'static str,
                      entity: &Entity<SelectState<Vec<SharedString>>>| {
            v_flex()
                .w_full()
                .min_w_0()
                .gap(px(6.0))
                .child(div().text_size(px(13.0)).child(label))
                .child(
                    div()
                        .debug_selector(move || selector.to_owned())
                        .w_full()
                        .border_1()
                        .rounded(px(6.0))
                        .border_color(if entity.read(cx).focus_handle(cx).is_focused(window) {
                            colors.primary
                        } else {
                            colors.line
                        })
                        .hover(|style| style.bg(colors.subtle).border_color(colors.control))
                        .child(Select::new(entity).appearance(false).disabled(disabled)),
                )
        };
        v_flex()
            .w_full()
            .min_w_0()
            .mt(px(18.0))
            .gap(px(12.0))
            .child(
                div()
                    .text_size(px(16.0))
                    .font_semibold()
                    .child(language.text(Message::ThemeEditorBackgroundEffects)),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(colors.secondary)
                    .child(language.text(Message::ThemeEditorBackgroundEffectsDescription)),
            )
            .child(input(
                "theme-editor-image-path",
                language.text(Message::ThemeEditorImagePath),
                &controls.image_path,
            ))
            .child(
                h_flex()
                    .gap(px(8.0))
                    .child(
                        Button::new("theme-editor-image-choose")
                            .debug_selector(|| "theme-editor-image-choose".to_owned())
                            .label(language.text(Message::ThemeEditorChooseImage))
                            .ghost()
                            .outline()
                            .h(px(32.0))
                            .px(px(12.0))
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_theme_background_image(window, cx)
                            })),
                    )
                    .child(
                        Button::new("theme-editor-image-clear")
                            .debug_selector(|| "theme-editor-image-clear".to_owned())
                            .label(language.text(Message::ThemeEditorRemoveImage))
                            .ghost()
                            .h(px(32.0))
                            .px(px(12.0))
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_theme_background_image(String::new(), window, cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap(px(10.0))
                    .child(
                        input(
                            "theme-editor-image-opacity",
                            language.text(Message::ThemeEditorImageOpacity),
                            &controls.image_opacity,
                        )
                        .flex_1(),
                    )
                    .child(
                        select(
                            "theme-editor-image-fit",
                            language.text(Message::ThemeEditorImageFit),
                            &controls.fit,
                        )
                        .flex_1(),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap(px(10.0))
                    .child(
                        select(
                            "theme-editor-image-alignment",
                            language.text(Message::ThemeEditorImageAlignment),
                            &controls.alignment,
                        )
                        .flex_1(),
                    )
                    .child(
                        select(
                            "theme-editor-blur",
                            language.text(Message::ThemeEditorWindowBlur),
                            &controls.blur,
                        )
                        .flex_1(),
                    ),
            )
            .child(
                Switch::new("theme-editor-image-cover")
                    .checked(
                        editor
                            .draft
                            .effects
                            .background_image_cover_chrome
                            .unwrap_or(self.runtime.background_image_cover_chrome),
                    )
                    .label(language.text(Message::ThemeEditorImageCover))
                    .disabled(disabled)
                    .on_click(cx.listener(|this, value: &bool, _, cx| {
                        if let Some(editor) =
                            this.theme_editor.as_mut().filter(|editor| !editor.save_busy)
                        {
                            editor.draft.effects.background_image_cover_chrome = Some(*value);
                            editor.saved = false;
                            editor.draft_seq = editor.draft_seq.wrapping_add(1);
                            editor.refresh_background_preview(&this.runtime, cx);
                            cx.notify();
                        }
                    })),
            )
    }
}
