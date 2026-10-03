//! Theme-driven package controls; file operations belong to theme_package.rs.
use super::appearance_picker::AppearanceColors;
use super::theme_package::{Mode, Phase};
use super::*;
use crate::i18n::Message;
use gpui::accesskit::Role;
use gpui_component::FocusTrapElement as _;

impl SettingsPane {
    pub(super) fn theme_package_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let state = self.theme_package.as_ref()?;
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let viewport = window.viewport_size();
        let width = (f32::from(viewport.width) - 32.0).min(620.0);
        let compact = width < 500.0;
        let busy = state.phase.busy();
        let locked = state.phase == Phase::Committing;
        let done = state.phase == Phase::Done;
        let mode = state.mode;
        let mut body = v_flex().w_full().gap(px(18.0));
        if mode == Mode::Export && !done {
            let field = |selector: &'static str, message: Message, input: &Entity<InputState>| {
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.0))
                    .child(div().text_size(px(13.0)).child(language.text(message)))
                    .child(
                        div()
                            .id(selector)
                            .debug_selector(move || selector.to_owned())
                            .w_full()
                            .border_b_1()
                            .border_color(if input.read(cx).focus_handle(cx).is_focused(window) {
                                colors.primary
                            } else {
                                colors.line
                            })
                            .child(
                                Input::new(input)
                                    .aria_label(language.text(message))
                                    .appearance(false)
                                    .h(px(32.0))
                                    .w_full()
                                    .disabled(busy),
                            ),
                    )
            };
            let row = || {
                if compact {
                    v_flex().w_full().gap(px(18.0))
                } else {
                    h_flex().w_full().gap(px(20.0))
                }
            };
            body = body
                .child(
                    row()
                        .child(field(
                            "theme-package-author",
                            Message::ThemePackageAuthor,
                            &state.author,
                        ))
                        .child(field(
                            "theme-package-github",
                            Message::ThemePackageGithub,
                            &state.github,
                        )),
                )
                .child(
                    row()
                        .child(field(
                            "theme-package-version",
                            Message::ThemePackageVersion,
                            &state.version,
                        ))
                        .child(field(
                            "theme-package-license",
                            Message::ThemePackageLicense,
                            &state.license,
                        )),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(colors.secondary)
                        .child(language.text(Message::ThemePackageMetadataHint)),
                )
                .child(
                    v_flex()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .child(language.text(Message::ThemePackagePreview)),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .gap(px(8.0))
                                .items_center()
                                .child(
                                    Button::new("theme-package-preview-choose")
                                        .debug_selector(|| {
                                            "theme-package-preview-choose".to_owned()
                                        })
                                        .label(language.text(Message::ThemePackageChoosePreview))
                                        .outline()
                                        .rounded(px(6.0))
                                        .h(px(32.0))
                                        .px(px(12.0))
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.choose_package_file(true, window, cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .text_size(px(13.0))
                                        .text_color(colors.secondary)
                                        .child(
                                            state
                                                .preview
                                                .as_ref()
                                                .and_then(|path| path.file_name())
                                                .map(|name| name.to_string_lossy().into_owned())
                                                .unwrap_or_else(|| {
                                                    language
                                                        .text(Message::ThemePackageNoPreview)
                                                        .to_owned()
                                                }),
                                        ),
                                )
                                .when(state.preview.is_some(), |row| {
                                    row.child(
                                        Button::new("theme-package-preview-clear")
                                            .label(language.text(Message::ThemeEditorRemoveImage))
                                            .ghost()
                                            .h(px(32.0))
                                            .px(px(12.0))
                                            .disabled(busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(state) = &mut this.theme_package {
                                                    state.preview = None;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                }),
                        ),
                );
        } else if mode == Mode::Import && !done {
            body = body
                .child(
                    Button::new("theme-package-source-choose")
                        .debug_selector(|| "theme-package-source-choose".to_owned())
                        .label(language.text(Message::ThemePackageChooseFile))
                        .outline()
                        .rounded(px(6.0))
                        .h(px(32.0))
                        .px(px(12.0))
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_package_file(false, window, cx)
                        })),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(colors.secondary)
                        .child(language.text(Message::ThemePackageImportHint)),
                );
            if let Some(checked) = &state.inspected {
                let manifest = &checked.manifest;
                body = body.child(super::theme_picker::theme_definition_sample(
                    &checked.definition,
                    None,
                    false,
                    compact,
                ));
                body = body.child(
                    v_flex()
                        .w_full()
                        .gap(px(12.0))
                        .child(
                            div().font_semibold().text_size(px(16.0)).child(manifest.name.clone()),
                        )
                        .child(div().text_size(px(13.0)).text_color(colors.secondary).child(
                            language.format(
                                Message::ThemePackageIdentity,
                                &[
                                    ("author", &manifest.author.name),
                                    ("version", &manifest.version),
                                    ("license", &manifest.license),
                                ],
                            ),
                        ))
                        .child(div().text_size(px(13.0)).text_color(colors.secondary).child(
                            language.format(
                                Message::ThemePackageResources,
                                &[("count", &manifest.resources.len().to_string())],
                            ),
                        )),
                );
            }
        }
        if done {
            let message = if mode == Mode::Import {
                language.format(
                    Message::ThemePackageImported,
                    &[(
                        "name",
                        state.installed.as_ref().map(|document| document.name()).unwrap_or(""),
                    )],
                )
            } else {
                language.text(Message::ThemePackageExported).to_owned()
            };
            body = body.child(
                div()
                    .debug_selector(|| "theme-package-success".to_owned())
                    .p(px(12.0))
                    .rounded(px(6.0))
                    .bg(cx.theme().success.opacity(0.08))
                    .text_color(cx.theme().success)
                    .text_size(px(13.0))
                    .child(message),
            );
            if let Some(output) = &state.output {
                body = body.child(
                    div()
                        .w_full()
                        .text_size(px(13.0))
                        .text_color(colors.secondary)
                        .child(output.display().to_string()),
                );
            }
        }
        if busy {
            body = body.child(
                div()
                    .debug_selector(|| "theme-package-busy".to_owned())
                    .text_size(px(13.0))
                    .text_color(colors.secondary)
                    .child(language.text(match state.phase {
                        Phase::Picking => Message::ThemeTransferPicking,
                        Phase::Inspecting => Message::ThemePackageChecking,
                        _ => Message::ThemePackageWorking,
                    })),
            );
        }
        if let Some(error) = &state.error {
            body = body.child(
                div()
                    .debug_selector(|| "theme-package-error".to_owned())
                    .p(px(12.0))
                    .rounded(px(6.0))
                    .bg(cx.theme().danger.opacity(0.08))
                    .text_size(px(13.0))
                    .text_color(cx.theme().danger)
                    .child(language.format(Message::ThemePackageFailed, &[("detail", error)])),
            );
        }
        let enabled = !busy
            && match mode {
                Mode::Import => state.inspected.is_some(),
                Mode::Export => {
                    state.document.is_some()
                        && !state.author.read(cx).value().trim().is_empty()
                        && !state.version.read(cx).value().trim().is_empty()
                        && !state.license.read(cx).value().trim().is_empty()
                },
            };
        let tabs = h_flex()
            .self_start()
            .gap(px(2.0))
            .p(px(2.0))
            .rounded_full()
            .border_1()
            .border_color(colors.line)
            .children(
                [
                    (Mode::Export, "theme-package-export-mode", Message::ThemePackageExportMode),
                    (Mode::Import, "theme-package-import-mode", Message::ThemePackageImportMode),
                ]
                .into_iter()
                .map(|(tab, id, message)| {
                    Button::new(id)
                        .debug_selector(move || id.to_owned())
                        .label(language.text(message))
                        .ghost()
                        .h(px(32.0))
                        .px(px(12.0))
                        .rounded_full()
                        .disabled(busy)
                        .when(mode == tab, |button| {
                            button.bg(colors.subtle).text_color(colors.ink).font_semibold()
                        })
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.switch_theme_package(tab, cx)),
                        )
                }),
            );
        let dialog = v_flex()
            .id("theme-package-dialog")
            .debug_selector(|| "theme-package-dialog".to_owned())
            .role(Role::Dialog)
            .aria_label(language.text(Message::ThemePackageTitle))
            .w(px(width))
            .max_h(px(f32::from(viewport.height) - 32.0))
            .p(px(if compact { 18.0 } else { 24.0 }))
            .gap(px(20.0))
            .rounded(px(12.0))
            .border_1()
            .border_color(colors.control)
            .bg(colors.surface)
            .text_color(colors.ink)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.close_theme_package(window, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                v_flex()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(22.0))
                            .font_semibold()
                            .child(language.text(Message::ThemePackageTitle)),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(colors.secondary)
                            .child(language.text(Message::ThemePackageDescription)),
                    ),
            )
            .child(tabs)
            .child(
                div()
                    .id("theme-package-scroll")
                    .w_full()
                    .max_h(px((f32::from(viewport.height) - 270.0).max(140.0)))
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(px(8.0))
                    .when(done && state.installed.is_some(), |row| {
                        row.child(
                            Button::new("theme-package-edit")
                                .debug_selector(|| "theme-package-edit".to_owned())
                                .label(language.text(Message::ThemePackageEditCopy))
                                .ghost()
                                .h(px(32.0))
                                .px(px(12.0))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.edit_installed_package(window, cx)
                                })),
                        )
                    })
                    .when(!done, |row| {
                        row.child(
                            Button::new("theme-package-cancel")
                                .debug_selector(|| "theme-package-cancel".to_owned())
                                .label(language.text(Message::ThemeTransferCancel))
                                .ghost()
                                .h(px(32.0))
                                .px(px(12.0))
                                .disabled(locked)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_theme_package(window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("theme-package-confirm")
                            .debug_selector(|| "theme-package-confirm".to_owned())
                            .label(language.text(if done {
                                Message::ThemeTransferClose
                            } else if mode == Mode::Export {
                                Message::ThemePackageExportMode
                            } else {
                                Message::ThemePackageInstall
                            }))
                            .with_variant(ButtonVariant::Primary)
                            .h(px(32.0))
                            .px(px(12.0))
                            .disabled(!done && !enabled)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if done {
                                    this.close_theme_package(window, cx);
                                } else if mode == Mode::Import {
                                    this.confirm_package_import(window, cx);
                                } else {
                                    this.confirm_package_export(window, cx);
                                }
                            })),
                    ),
            )
            .focus_trap("theme-package-focus-trap", &state.focus);
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(gpui::point(px(0.0), px(0.0)))
                    .child(
                        div()
                            .id("theme-package-overlay")
                            .w(viewport.width)
                            .h(viewport.height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .occlude()
                            .bg(colors.scrim)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                            .child(dialog),
                    ),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }
}
