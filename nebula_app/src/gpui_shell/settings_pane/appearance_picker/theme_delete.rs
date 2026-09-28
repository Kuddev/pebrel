use super::*;

struct ThemeDeleteResult {
    deleted: bool,
    preference_revision: Option<crate::theme_library::preferences::PreferenceRevision>,
    preference_error: Option<String>,
    deletion_error: Option<String>,
}

impl SettingsPane {
    pub(super) fn request_delete_selected_theme(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self
            .appearance_picker
            .as_ref()
            .filter(|picker| !picker.apply_busy && !picker.custom_loading)
            .and_then(|picker| picker.custom_document(picker.draft))
        else {
            return;
        };
        let Some(id) = document.id() else { return };
        let name = document.name().to_owned();
        let active = self.runtime.custom_theme.as_deref() == Some(id);
        let language = crate::gpui_shell::config::ui_language(cx);
        let title = language.format(Message::ThemePickerDeleteTitle, &[("name", &name)]);
        let body = language.text(if active {
            Message::ThemePickerDeleteActiveBody
        } else {
            Message::ThemePickerDeleteBody
        });
        let pane = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let pane = pane.clone();
            confirm_dialog(
                dialog,
                window,
                title.clone(),
                body,
                language.text(Message::ThemePickerDeleteConfirm),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = pane.update(cx, |this, cx| {
                    this.delete_selected_theme(window, cx);
                });
                true
            })
        });
    }

    fn delete_selected_theme(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some((document, definition, session_seq, preference_revision)) = self
            .appearance_picker
            .as_ref()
            .filter(|picker| !picker.apply_busy && !picker.custom_loading)
            .and_then(|picker| {
                let document = picker.custom_document(picker.draft)?.clone();
                let definition = document.definition().ok()?;
                Some((
                    document,
                    definition,
                    picker.custom_load_seq,
                    picker.preference_revision.clone(),
                ))
            })
        else {
            return;
        };
        let Some(id) = document.id().map(str::to_owned) else { return };
        let document_revision = document.revision();
        let fallback = definition.base;
        let was_active = self.runtime.custom_theme.as_deref() == Some(id.as_str());
        if let Some(picker) = self.appearance_picker.as_mut() {
            picker.apply_busy = true;
            picker.error = None;
        }
        let executor = cx.background_executor().clone();
        let deleted_id = id.clone();
        cx.spawn(async move |this, cx| {
            let result = executor
                .spawn(async move {
                    let store = crate::theme_library::ThemeLibraryStore::default();
                    if !was_active {
                        store.delete(&id, document_revision).map_err(|error| error.to_string())?;
                        return Ok::<ThemeDeleteResult, String>(ThemeDeleteResult {
                            deleted: true,
                            preference_revision: None,
                            preference_error: None,
                            deletion_error: None,
                        });
                    }
                    let mut updates =
                        crate::gpui_shell::theme::theme_card_persist_updates(fallback).to_vec();
                    updates.push(("custom_theme", String::new()));
                    updates.push(("theme_foreground", String::new()));
                    let preference_revision = match preference_revision {
                        Ok(revision) => {
                            match crate::theme_library::preferences::save(&revision, &updates) {
                                Ok(next) => next,
                                Err(error) => {
                                    return Ok(ThemeDeleteResult {
                                        deleted: false,
                                        preference_revision: None,
                                        preference_error: Some(error.to_string()),
                                        deletion_error: None,
                                    });
                                },
                            }
                        },
                        Err(error) => {
                            return Ok(ThemeDeleteResult {
                                deleted: false,
                                preference_revision: None,
                                preference_error: Some(error),
                                deletion_error: None,
                            });
                        },
                    };
                    // Publish the fallback before removing the active source. A
                    // stale settings revision therefore leaves the theme file
                    // untouched. If deletion then loses its own revision race,
                    // the newer document remains available and only the active
                    // selection changes to the already-persisted fallback.
                    let deletion_error =
                        store.delete(&id, document_revision).err().map(|error| error.to_string());
                    Ok(ThemeDeleteResult {
                        deleted: deletion_error.is_none(),
                        preference_revision: Some(preference_revision),
                        preference_error: None,
                        deletion_error,
                    })
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_theme_delete(
                    session_seq,
                    &deleted_id,
                    fallback,
                    was_active,
                    result,
                    window,
                    cx,
                );
            });
        })
        .detach();
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_theme_delete(
        &mut self,
        session_seq: u64,
        id: &str,
        fallback: ThemeName,
        was_active: bool,
        result: Result<ThemeDeleteResult, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .appearance_picker
            .as_ref()
            .is_none_or(|picker| picker.custom_load_seq != session_seq)
        {
            return;
        }
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                let language = crate::gpui_shell::config::ui_language(cx);
                if let Some(picker) = self.appearance_picker.as_mut() {
                    picker.apply_busy = false;
                    picker.error = Some(
                        language.format(Message::ThemePickerDeleteError, &[("error", &error)]),
                    );
                }
                cx.notify();
                return;
            },
        };

        let fallback_persisted = was_active && outcome.preference_revision.is_some();
        let active_custom_id =
            if fallback_persisted { None } else { self.runtime.custom_theme.clone() };
        let active_builtin = self.runtime.theme;
        let active_foreground =
            if fallback_persisted { None } else { self.runtime.theme_foreground };
        if let Some(picker) = self.appearance_picker.as_mut() {
            picker.apply_busy = false;
            if let Some(revision) = outcome.preference_revision.clone() {
                picker.preference_revision = Ok(revision);
            }
            if outcome.deleted {
                if let Some(index) =
                    picker.custom_themes.iter().position(|document| document.id() == Some(id))
                {
                    picker.custom_themes.remove(index);
                }
                picker.custom_definitions = picker
                    .custom_themes
                    .iter()
                    .filter_map(|document| document.definition().ok())
                    .collect();
                picker.filter = 0;
                let restored = active_custom_id
                    .as_deref()
                    .and_then(|active_id| {
                        picker
                            .custom_themes
                            .iter()
                            .position(|document| document.id() == Some(active_id))
                            .map(AppearanceSelection::Custom)
                    })
                    .unwrap_or(AppearanceSelection::Theme(if fallback_persisted {
                        fallback
                    } else {
                        active_builtin
                    }));
                picker.draft = restored;
                picker.initial_draft = restored;
                picker.foreground_override = active_foreground;
                picker.initial_foreground_override = active_foreground;
                picker.draft_touched = false;
                picker.options = picker
                    .choices()
                    .into_iter()
                    .map(|choice| (choice, cx.focus_handle()))
                    .collect();
            } else if fallback_persisted {
                // Preference publication succeeded but a competing library edit
                // won the document revision race. Keep that newer document and
                // align the open picker with the fallback that is now active.
                let restored = AppearanceSelection::Theme(fallback);
                picker.draft = restored;
                picker.initial_draft = restored;
                picker.foreground_override = None;
                picker.initial_foreground_override = None;
                picker.draft_touched = false;
            }
            let language = crate::gpui_shell::config::ui_language(cx);
            picker.error = outcome
                .preference_error
                .as_ref()
                .map(|error| {
                    language.format(Message::ThemePickerDeleteSettingsError, &[("error", error)])
                })
                .or_else(|| {
                    outcome.deletion_error.as_ref().map(|error| {
                        language.format(Message::ThemePickerDeleteError, &[("error", error)])
                    })
                });
        }

        if fallback_persisted {
            let mut updates =
                crate::gpui_shell::theme::theme_card_persist_updates(fallback).to_vec();
            updates.push(("custom_theme", String::new()));
            updates.push(("theme_foreground", String::new()));
            self.apply_persisted_runtime(&updates, cx);
            self.sync_background_color_picker(window, cx);
        }
        cx.notify();
    }
}
