//! Explicit editing and deletion of saved custom themes.

use super::appearance_picker::AppearanceSelection;
use super::*;
use crate::i18n::Message;
use crate::theme_library::ThemeDocument;

impl SettingsPane {
    pub(super) fn request_delete_custom_theme(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.appearance_picker.as_ref() else { return };
        if picker.apply_busy || picker.custom_loading {
            return;
        }
        let Some(document) = picker.custom_document(picker.draft).cloned() else { return };
        let sequence = picker.custom_load_seq;
        let language = crate::gpui_shell::config::ui_language(cx);
        let pane = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let pane = pane.clone();
            let document = document.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::ThemePickerDeleteTitle),
                language.format(Message::ThemePickerDeleteBody, &[("name", document.name())]),
                language.text(Message::CommonDelete),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = pane.update(cx, |this, cx| {
                    this.delete_custom_theme(document.clone(), sequence, window, cx);
                });
                true
            })
        });
    }

    fn delete_custom_theme(
        &mut self,
        document: ThemeDocument,
        sequence: u64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.appearance_picker.as_mut() else { return };
        if picker.custom_load_seq != sequence || picker.apply_busy || picker.custom_loading {
            return;
        }
        let revision = picker.preference_revision.clone();
        picker.apply_busy = true;
        picker.delete_busy = true;
        picker.error = None;
        let mut updates =
            crate::gpui_shell::theme::theme_card_persist_updates(ThemeName::default()).to_vec();
        updates.push(("custom_theme", String::new()));
        updates.push(("theme_foreground", String::new()));
        let fallback_updates = updates.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let deleted_id = document.id().map(str::to_owned);
            let result = executor
                .spawn(async move {
                    let revision = revision?;
                    crate::theme_library::preferences::delete_custom_theme(
                        &crate::theme_library::ThemeLibraryStore::default(),
                        &document,
                        &revision,
                        &fallback_updates,
                    )
                    .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this
                    .appearance_picker
                    .as_ref()
                    .is_some_and(|picker| picker.custom_load_seq == sequence)
                {
                    return;
                }
                let switched = result.as_ref().is_ok_and(|result| result.switched_to_default);
                if switched {
                    this.apply_persisted_runtime(&updates, cx);
                    this.sync_background_color_picker(window, cx);
                }
                let language = crate::gpui_shell::config::ui_language(cx);
                let picker = this.appearance_picker.as_mut().expect("current picker");
                picker.apply_busy = false;
                picker.delete_busy = false;
                match result {
                    Err(error) => {
                        picker.error = Some(
                            language.format(Message::ThemePickerDeleteError, &[("error", &error)]),
                        );
                    },
                    Ok(result) => {
                        picker.preference_revision = Ok(result.revision);
                        if switched {
                            picker.initial_draft = AppearanceSelection::Theme(ThemeName::default());
                            picker.initial_foreground_override = None;
                        }
                        if let Some(error) = result.delete_error {
                            picker.error = Some(language.format(
                                if switched {
                                    Message::ThemePickerDeletePartialError
                                } else {
                                    Message::ThemePickerDeleteError
                                },
                                &[("error", &error)],
                            ));
                        } else if let Some(index) = picker
                            .custom_themes
                            .iter()
                            .position(|theme| theme.id() == deleted_id.as_deref())
                        {
                            picker.custom_themes.remove(index);
                            picker.custom_definitions.remove(index);
                            let remap = |choice, replacement| match choice {
                                AppearanceSelection::Custom(old) if old == index => replacement,
                                AppearanceSelection::Custom(old) if old > index => {
                                    AppearanceSelection::Custom(old - 1)
                                },
                                other => other,
                            };
                            if picker.initial_draft == AppearanceSelection::Custom(index) {
                                picker.initial_foreground_override = None;
                            }
                            picker.initial_draft = remap(
                                picker.initial_draft,
                                AppearanceSelection::Theme(ThemeName::default()),
                            );
                            picker.draft = remap(picker.draft, picker.initial_draft);
                            picker.foreground_override = picker.initial_foreground_override;
                            picker.error = None;
                            picker.options = picker
                                .choices()
                                .into_iter()
                                .map(|choice| (choice, cx.focus_handle()))
                                .collect();
                            window.focus(&picker.focus, cx);
                        }
                    },
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
