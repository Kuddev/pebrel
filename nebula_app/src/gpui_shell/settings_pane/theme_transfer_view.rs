//! Native theme transfer modal.
//!
//! File work and persistence live in theme_transfer.rs. This module only
//! renders snapshots from that state and routes user actions back through the
//! transfer state machine.

use super::appearance_picker::AppearanceColors;
use super::theme_picker::theme_definition_sample;
use super::theme_transfer::{ThemeTransferMode, ThemeTransferStatus};
use super::*;
use crate::i18n::Message;
use crate::theme_library::{ImportCandidate, ThemeFormat};
use gpui::accesskit::Role;

const EXPORT_FORMATS: [ThemeFormat; 6] = [
    ThemeFormat::Pebrel,
    ThemeFormat::WindowsTerminal,
    ThemeFormat::Kitty,
    ThemeFormat::Ghostty,
    ThemeFormat::WezTerm,
    ThemeFormat::Alacritty,
];

impl SettingsPane {
    pub(super) fn theme_transfer_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let mode = self.theme_transfer.mode?;
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let viewport = window.viewport_size();
        let compact = f32::from(viewport.width) < 720.0;
        let width = (f32::from(viewport.width) - if compact { 24.0 } else { 40.0 })
            .min(if compact { 520.0 } else { 760.0 })
            .max(280.0);
        let padding = if compact { 18.0 } else { 25.0 };
        let busy = self.theme_transfer.busy;
        let status = self.theme_transfer.status;
        let write_locked =
            matches!((mode, status), (ThemeTransferMode::Export, ThemeTransferStatus::Writing));
        let error = self.theme_transfer.error.clone();
        let completed = matches!(status, ThemeTransferStatus::Completed);
        let notice = completed.then(|| match mode {
            ThemeTransferMode::Import => language.text(Message::ThemeTransferTemplateReady),
            ThemeTransferMode::Export => language.text(Message::ThemeTransferExported),
        });
        let title = match mode {
            ThemeTransferMode::Import => language.text(Message::ThemeTransferImportTitle),
            ThemeTransferMode::Export => language.text(Message::ThemeTransferExportTitle),
        };
        let subtitle = match mode {
            ThemeTransferMode::Import => language.text(Message::ThemeTransferImportDescription),
            ThemeTransferMode::Export => language.text(Message::ThemeTransferExportDescription),
        };
        let body = match mode {
            ThemeTransferMode::Import => self.theme_transfer_import_body(compact, colors, cx),
            ThemeTransferMode::Export => self.theme_transfer_export_body(compact, colors, cx),
        };
        let confirm_enabled = match mode {
            ThemeTransferMode::Import => {
                !busy
                    && !self.theme_transfer.import_candidates.is_empty()
                    && !matches!(status, ThemeTransferStatus::Completed)
            },
            ThemeTransferMode::Export => {
                !busy
                    && self.theme_transfer.export_artifact.is_some()
                    && !matches!(status, ThemeTransferStatus::Completed)
            },
        };
        let confirm_label = match mode {
            ThemeTransferMode::Import => language.text(Message::ThemeTransferUseTemplate),
            ThemeTransferMode::Export => language.text(Message::ThemeTransferExport),
        };
        let status_label = transfer_status_label(status, language);
        let dialog = v_flex()
            .id("theme-transfer-dialog")
            .debug_selector(|| "theme-transfer-dialog".to_owned())
            .role(Role::Dialog)
            .aria_label(title)
            .w(px(width))
            .max_h(px((f32::from(viewport.height) - 28.0).max(240.0)))
            .rounded(px(14.0))
            .border_1()
            .border_color(colors.control)
            .bg(colors.surface)
            .text_color(colors.ink)
            .shadow_2xl()
            .overflow_hidden()
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    if !write_locked {
                        this.cancel_theme_transfer(cx);
                    }
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .px(px(padding))
                    .pt(px(if compact { 18.0 } else { 23.0 }))
                    .pb(px(if compact { 14.0 } else { 18.0 }))
                    .gap(px(12.0))
                    .items_start()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(colors.line)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .text_size(px(if compact { 17.0 } else { 19.0 }))
                                    .font_semibold()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(colors.secondary)
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(7.0))
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(colors.muted)
                                    .child(status_label),
                            )
                            .child(
                                Button::new("theme-transfer-close")
                                    .icon(IconName::Close)
                                    .ghost()
                                    .size(px(28.0))
                                    .text_color(colors.secondary)
                                    .disabled(write_locked)
                                    .tooltip(language.text(Message::ThemeTransferClose))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_theme_transfer(cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .id("theme-transfer-scroll")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(padding))
                    .py(px(if compact { 16.0 } else { 21.0 }))
                    .child(body),
            )
            .when_some(error, |dialog, error| {
                dialog.child(
                    div()
                        .px(px(padding))
                        .pb(px(10.0))
                        .text_size(px(11.0))
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .when_some(notice, |dialog, notice| {
                if completed {
                    dialog.child(
                        div()
                            .px(px(padding))
                            .pb(px(10.0))
                            .text_size(px(11.0))
                            .text_color(cx.theme().success)
                            .child(notice),
                    )
                } else {
                    dialog
                }
            })
            .child(
                h_flex()
                    .px(px(padding))
                    .py(px(15.0))
                    .gap(px(8.0))
                    .justify_end()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(colors.line)
                    .child(
                        Button::new("theme-transfer-cancel")
                            .debug_selector(|| "theme-transfer-cancel".to_owned())
                            .label(if write_locked {
                                language.text(Message::ThemeTransferWriting)
                            } else {
                                language.text(Message::ThemeTransferCancel)
                            })
                            .ghost()
                            .h(px(32.0))
                            .px(px(if compact { 10.0 } else { 14.0 }))
                            .disabled(write_locked)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_theme_transfer(cx);
                            })),
                    )
                    .child(if completed {
                        Button::new("theme-transfer-done")
                            .debug_selector(|| "theme-transfer-done".to_owned())
                            .label(language.text(Message::ThemeTransferClose))
                            .with_variant(ButtonVariant::Primary)
                            .h(px(32.0))
                            .px(px(if compact { 11.0 } else { 16.0 }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_theme_transfer(cx);
                            }))
                            .into_any_element()
                    } else {
                        Button::new("theme-transfer-confirm")
                            .debug_selector(|| "theme-transfer-confirm".to_owned())
                            .label(confirm_label)
                            .with_variant(ButtonVariant::Primary)
                            .h(px(32.0))
                            .px(px(if compact { 11.0 } else { 16.0 }))
                            .disabled(!confirm_enabled)
                            .on_click(cx.listener(move |this, _, window, cx| match mode {
                                ThemeTransferMode::Import => {
                                    this.confirm_theme_import(None, cx);
                                    if let Some(document) = this.take_imported_theme() {
                                        match this.load_imported_theme_editor(document, window, cx)
                                        {
                                            Ok(()) => this.cancel_theme_transfer(cx),
                                            Err(error) => {
                                                this.theme_transfer.status =
                                                    ThemeTransferStatus::Error;
                                                this.theme_transfer.error = Some(error);
                                                cx.notify();
                                            },
                                        }
                                    }
                                },
                                ThemeTransferMode::Export => {
                                    this.confirm_theme_export(window, cx);
                                },
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element();

        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(gpui::point(px(0.0), px(0.0)))
                    .child(
                        div()
                            .id("theme-transfer-overlay")
                            .w(viewport.width)
                            .h(viewport.height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .occlude()
                            .bg(colors.scrim)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    if write_locked {
                                        cx.stop_propagation();
                                    } else {
                                        this.cancel_theme_transfer(cx);
                                    }
                                }),
                            )
                            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                            .child(dialog),
                    ),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    fn theme_transfer_import_body(
        &self,
        compact: bool,
        colors: AppearanceColors,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let state = &self.theme_transfer;
        let candidates = state.import_candidates.clone();
        let selected = state.selected_import;
        let diagnostics = state.import_diagnostics.clone();
        let busy = state.busy;
        let candidate_count = candidates.len();
        let cards = candidates.iter().enumerate().map(|(index, candidate)| {
            self.theme_transfer_candidate_card(
                index,
                candidate,
                index == selected,
                compact,
                colors,
                cx,
            )
        });
        let preview = candidates
            .get(selected)
            .and_then(|candidate| candidate.document.definition().ok())
            .map(|definition| {
                theme_definition_sample(&definition, None, false, compact)
                    .id("theme-transfer-selected-preview")
            });
        let mut body = v_flex().w_full().gap(px(if compact { 14.0 } else { 18.0 }));
        if candidate_count == 0 {
            body = body.child(
                div()
                    .text_size(px(12.0))
                    .text_color(colors.secondary)
                    .child(language.text(Message::ThemeTransferNoCandidates)),
            );
        } else {
            body = body
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(colors.secondary)
                                .child(language.text(Message::ThemeTransferCandidates)),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(colors.muted)
                                .child(format!("{candidate_count}")),
                        ),
                )
                .child(v_flex().w_full().gap(px(7.0)).children(cards))
                .when_some(preview, |body, preview| {
                    body.child(
                        v_flex()
                            .w_full()
                            .gap(px(7.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(colors.secondary)
                                    .child(language.text(Message::ThemeTransferPreview)),
                            )
                            .child(preview),
                    )
                });
        }
        if !diagnostics.is_empty() {
            body = body.child(self.theme_transfer_diagnostics(&diagnostics, colors, cx));
        }
        body
    }

    fn theme_transfer_candidate_card(
        &self,
        index: usize,
        candidate: &ImportCandidate,
        selected: bool,
        compact: bool,
        colors: AppearanceColors,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let name = candidate.document.name().to_owned();
        let format = candidate.format.to_string();
        let filename = candidate.filename.clone();
        let warnings = candidate.warnings.clone();
        let preview = candidate
            .document
            .definition()
            .ok()
            .map(|definition| theme_definition_sample(&definition, None, true, compact));
        let selected_bg = colors.selected;
        let hover_bg = colors.subtle;
        let card = v_flex()
            .id(("theme-transfer-candidate", index))
            .debug_selector(move || format!("theme-transfer-candidate-{index}"))
            .w_full()
            .gap(px(8.0))
            .p(px(if compact { 9.0 } else { 11.0 }))
            .rounded(px(8.0))
            .border_1()
            .border_color(if selected { colors.primary } else { colors.control })
            .bg(if selected { selected_bg } else { colors.surface })
            .cursor_pointer()
            .hover(move |card| card.bg(hover_bg))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_theme_import(index, cx);
            }));
        card.child(
            h_flex()
                .w_full()
                .gap(px(9.0))
                .items_start()
                .child(
                    div()
                        .w(px(if compact { 112.0 } else { 145.0 }))
                        .flex_shrink_0()
                        .when_some(preview, |slot, preview| slot.child(preview)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(3.0))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .gap(px(6.0))
                                .child(div().font_medium().truncate().child(name))
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(10.0))
                                        .text_color(colors.secondary)
                                        .child(format),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(colors.muted)
                                .truncate()
                                .child(filename),
                        )
                        .when(!warnings.is_empty(), |row| {
                            row.child(
                                div().text_size(px(10.0)).text_color(cx.theme().warning).child(
                                    format!(
                                        "{} {}",
                                        language.text(Message::ThemeTransferWarnings),
                                        warnings.join("; ")
                                    ),
                                ),
                            )
                        }),
                ),
        )
    }

    fn theme_transfer_diagnostics(
        &self,
        diagnostics: &[crate::theme_library::ImportDiagnostic],
        colors: AppearanceColors,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        v_flex()
            .w_full()
            .gap(px(5.0))
            .pt(px(8.0))
            .border_t_1()
            .border_color(colors.line)
            .child(
                div()
                    .text_size(px(11.0))
                    .font_medium()
                    .text_color(colors.secondary)
                    .child(language.text(Message::ThemeTransferDiagnostics)),
            )
            .children(diagnostics.iter().map(|diagnostic| {
                v_flex()
                    .gap(px(2.0))
                    .text_size(px(10.0))
                    .text_color(cx.theme().danger)
                    .child(diagnostic.message.clone())
                    .child(
                        div()
                            .text_color(colors.muted)
                            .truncate()
                            .child(diagnostic.filename.clone()),
                    )
            }))
    }

    fn theme_transfer_export_body(
        &self,
        compact: bool,
        colors: AppearanceColors,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let state = &self.theme_transfer;
        let format = state.export_format;
        let artifact = state.export_artifact.clone();
        let losses = artifact.as_ref().map(|artifact| artifact.losses.clone()).unwrap_or_default();
        let summary = artifact.as_ref().map(|artifact| artifact.summary.clone());
        let busy = state.busy;
        let selected_definition =
            state.export_document.as_ref().and_then(|document| document.definition().ok());
        let mut body = v_flex().w_full().gap(px(if compact { 14.0 } else { 18.0 })).child(
            v_flex()
                .w_full()
                .gap(px(7.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.secondary)
                        .child(language.text(Message::ThemeTransferOutputFormat)),
                )
                .child(h_flex().w_full().flex_wrap().gap(px(5.0)).children(
                    EXPORT_FORMATS.into_iter().enumerate().map(|(index, candidate)| {
                        let selected = candidate == format;
                        Button::new(("theme-transfer-format", index))
                            .debug_selector(move || {
                                format!("theme-transfer-format-{:?}", candidate)
                            })
                            .role(Role::RadioButton)
                            .toggled(selected)
                            .label(candidate.to_string())
                            .ghost()
                            .small()
                            .rounded_full()
                            .text_color(if selected { colors.ink } else { colors.secondary })
                            .when(selected, |button| button.bg(colors.selected).font_semibold())
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_theme_export_format(candidate, cx);
                            }))
                    }),
                )),
        );
        if let Some(definition) = selected_definition {
            body = body.child(
                v_flex()
                    .w_full()
                    .gap(px(7.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.secondary)
                            .child(language.text(Message::ThemeTransferPreview)),
                    )
                    .child(theme_definition_sample(&definition, None, false, compact)),
            );
        }
        if let Some(summary) = summary {
            body =
                body.child(div().text_size(px(11.0)).text_color(colors.secondary).child(summary));
        }
        if !losses.is_empty() {
            body = body.child(
                v_flex()
                    .w_full()
                    .gap(px(5.0))
                    .pt(px(8.0))
                    .border_t_1()
                    .border_color(colors.line)
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_medium()
                            .text_color(cx.theme().warning)
                            .child(language.text(Message::ThemeTransferLosses)),
                    )
                    .children(losses.into_iter().map(|loss| {
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.secondary)
                            .child(format!("- {loss}"))
                    })),
            );
        }
        if let Some(path) = &state.export_path {
            body = body.child(
                v_flex()
                    .gap(px(3.0))
                    .pt(px(8.0))
                    .border_t_1()
                    .border_color(colors.line)
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.secondary)
                            .child(language.text(Message::ThemeTransferExported)),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.muted)
                            .truncate()
                            .child(path.display().to_string()),
                    ),
            );
        }
        body
    }
}

fn transfer_status_label(
    status: ThemeTransferStatus,
    language: crate::display::UiLanguage,
) -> &'static str {
    match status {
        ThemeTransferStatus::Picking => language.text(Message::ThemeTransferPicking),
        ThemeTransferStatus::Inspecting => language.text(Message::ThemeTransferInspecting),
        ThemeTransferStatus::Exporting => language.text(Message::ThemeTransferPreparing),
        ThemeTransferStatus::Importing => language.text(Message::ThemeTransferPreparing),
        ThemeTransferStatus::Writing => language.text(Message::ThemeTransferWriting),
        ThemeTransferStatus::Completed => language.text(Message::ThemeTransferCompleted),
        ThemeTransferStatus::Error => language.text(Message::ThemeTransferError),
        ThemeTransferStatus::Ready | ThemeTransferStatus::Idle => "",
    }
}
