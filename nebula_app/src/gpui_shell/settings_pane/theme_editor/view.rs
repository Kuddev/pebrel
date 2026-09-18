//! Theme editor layout and preview rendering.

use super::*;
use crate::i18n::Message;
use gpui::accesskit::Role;
use gpui_component::FocusTrapElement as _;

impl SettingsPane {
    fn theme_editor_preview(
        &self,
        editor: &ThemeEditor,
        colors: AppearanceColors,
        language: crate::display::UiLanguage,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let draft = &editor.draft;
        let opacity = draft.effects.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
        let resolved_ui = draft.resolved_ui();
        let background = theme_color(draft.terminal.background, opacity);
        let foreground = theme_color(draft.terminal.foreground, 1.0);
        let accent = theme_color(resolved_ui.accent, 1.0);
        let cursor = theme_color(draft.terminal.cursor.unwrap_or(draft.terminal.foreground), 1.0);
        let selection_background = theme_color(
            draft
                .terminal
                .selection_background
                .unwrap_or_else(|| draft.resolved_ui().selection_rgb()),
            1.0,
        );
        let selection_foreground = theme_color(
            draft.terminal.selection_foreground.unwrap_or(draft.terminal.foreground),
            1.0,
        );
        let family = draft
            .typography
            .font_family
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| self.current_font_chain(cx));
        let font_size = draft
            .typography
            .font_size
            .unwrap_or_else(|| self.terminal_font_size_px(cx))
            .clamp(4.0, 96.0);
        let line_height = draft.typography.line_height.unwrap_or(1.60).clamp(0.5, 3.0);
        let preview_mode = editor.preview;

        let preview_surface = match preview_mode {
            1 => v_flex()
                .w_full()
                .min_h(px(if compact { 170.0 } else { 226.0 }))
                .p(px(14.0))
                .gap(px(8.0))
                .rounded(px(9.0))
                .bg(theme_color(draft.resolved_ui().background, opacity))
                .text_color(theme_color(draft.resolved_ui().foreground, 1.0))
                .child(
                    h_flex()
                        .w_full()
                        .h(px(28.0))
                        .px(px(9.0))
                        .items_center()
                        .justify_between()
                        .rounded(px(6.0))
                        .bg(theme_color(draft.resolved_ui().shell, opacity))
                        .child(div().text_size(px(10.5)).child(draft.name.clone()))
                        .child(
                            div()
                                .text_size(px(9.0))
                                .text_color(theme_color(resolved_ui.muted, 1.0))
                                .child(language.text(Message::CommonTheme)),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .h(px(32.0))
                        .px(px(9.0))
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(6.0))
                        .bg(selection_background.opacity(0.9))
                        .text_color(selection_foreground)
                        .child(div().size(px(7.0)).rounded_full().bg(accent))
                        .child(language.text(Message::ThemeEditorCurrentSetting)),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(8.0))
                        .items_center()
                        .child(div().w(px(56.0)).h(px(7.0)).rounded_full().bg(accent))
                        .child(
                            div()
                                .w(px(92.0))
                                .h(px(7.0))
                                .rounded_full()
                                .bg(foreground.opacity(0.38)),
                        )
                        .child(
                            div()
                                .w(px(36.0))
                                .h(px(7.0))
                                .rounded_full()
                                .bg(foreground.opacity(0.20)),
                        ),
                )
                .into_any_element(),
            2 => {
                let ansi = draft.terminal.palette.ansi_colors();
                v_flex()
                    .w_full()
                    .min_h(px(if compact { 170.0 } else { 226.0 }))
                    .p(px(15.0))
                    .gap(px(12.0))
                    .rounded(px(9.0))
                    .bg(background)
                    .text_color(foreground)
                    .child(
                        div()
                            .font(crate::font_install::gpui_font_with_fallbacks(&family))
                            .text_size(px(font_size * 0.76))
                            .line_height(gpui::relative(line_height))
                            .child(language.text(Message::ThemeEditorPreviewAnsi)),
                    )
                    .child(h_flex().w_full().flex_wrap().gap(px(7.0)).children(
                        ansi.into_iter().enumerate().map(|(index, color)| {
                            let tooltip = format!("ANSI {index} {}", format_hex_rgb(color));
                            div()
                                .id(("theme-editor-preview-ansi", index))
                                .size(px(if compact { 22.0 } else { 27.0 }))
                                .rounded(px(5.0))
                                .bg(theme_color(color, 1.0))
                                .tooltip(move |window, cx| {
                                    gpui_component::tooltip::Tooltip::new(tooltip.clone())
                                        .build(window, cx)
                                })
                        }),
                    ))
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(8.0))
                            .items_center()
                            .child(div().size(px(8.0)).rounded_full().bg(cursor))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .child(format!("{} · {line_height:.2}", family)),
                            ),
                    )
                    .into_any_element()
            },
            _ => v_flex()
                .w_full()
                .min_h(px(if compact { 170.0 } else { 226.0 }))
                .px(px(15.0))
                .py(px(14.0))
                .gap(px(7.0))
                .rounded(px(9.0))
                .bg(background)
                .text_color(foreground)
                .font(crate::font_install::gpui_font_with_fallbacks(&family))
                .text_size(px(font_size * 0.78))
                .line_height(gpui::relative(line_height))
                .child(
                    h_flex()
                        .w_full()
                        .h(px(24.0))
                        .px(px(8.0))
                        .items_center()
                        .justify_between()
                        .rounded(px(5.0))
                        .bg(theme_color(resolved_ui.shell, opacity))
                        .text_size(px(9.5))
                        .child("❯_")
                        .child(div().text_size(px(9.5)).child("PowerShell"))
                        .child(
                            div()
                                .text_color(theme_color(resolved_ui.muted, 1.0))
                                .child("~/workspace"),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(7.0))
                        .child(div().text_color(accent).child("❯"))
                        .child(div().text_color(foreground.opacity(0.82)).child("~/workspace")),
                )
                .child(div().text_color(foreground.opacity(0.74)).child("git status --short"))
                .child(
                    h_flex()
                        .gap(px(6.0))
                        .child(div().text_color(accent).child("M"))
                        .child("src/main.rs"),
                )
                .child(
                    h_flex()
                        .gap(px(6.0))
                        .child(div().text_color(theme_color([224, 174, 82], 1.0)).child("??"))
                        .child("themes/paper.json"),
                )
                .child(
                    div().mt(px(4.0)).text_color(foreground.opacity(0.74)).child("git diff --stat"),
                )
                .child(
                    h_flex()
                        .gap(px(6.0))
                        .child("src/main.rs")
                        .child(div().text_color(foreground.opacity(0.50)).child("│"))
                        .child(div().text_color(accent).child("++++++"))
                        .child(div().text_color(theme_color([224, 100, 110], 1.0)).child("──")),
                )
                .child(
                    div()
                        .text_color(foreground.opacity(0.52))
                        .child("1 file changed, 6 insertions(+), 2 deletions(-)"),
                )
                .child(
                    div()
                        .w_full()
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(px(4.0))
                        .bg(selection_background)
                        .text_color(selection_foreground)
                        .child(language.text(Message::ThemeEditorSelectedLine)),
                )
                .child(
                    h_flex()
                        .gap(px(7.0))
                        .items_center()
                        .child(div().text_color(accent).child("❯"))
                        .child(div().w(px(8.0)).h(px(font_size * line_height)).bg(cursor)),
                )
                .into_any_element(),
        };
        let tabs = [
            (
                0,
                "theme-editor-preview-terminal",
                language.text(Message::ThemeEditorPreviewTerminal),
            ),
            (1, "theme-editor-preview-ui", language.text(Message::ThemeEditorPreviewInterface)),
            (2, "theme-editor-preview-ansi", language.text(Message::ThemeEditorPreviewAnsi)),
        ];
        v_flex()
            .w(px(if compact { 0.0 } else { 360.0 }))
            .when(compact, |preview| preview.w_full())
            .flex_shrink_0()
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(10.0))
                    .font_semibold()
                    .text_color(colors.secondary)
                    .child(language.text(Message::ThemeEditorLivePreview)),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.0))
                            .font_semibold()
                            .child(editor.draft.name.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.0))
                            .text_color(colors.secondary)
                            .child(language.text(Message::CommonTheme)),
                    ),
            )
            .child(preview_surface)
            .child(h_flex().w_full().gap(px(4.0)).children(tabs.into_iter().map(
                |(index, selector, label)| {
                    Button::new(selector)
                        .debug_selector(move || selector.to_owned())
                        .label(label)
                        .ghost()
                        .small()
                        .rounded_full()
                        .when(preview_mode == index, |button| {
                            button.bg(colors.selected).font_semibold()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(editor) = this.theme_editor.as_mut() {
                                editor.preview = index;
                            }
                            cx.notify();
                        }))
                },
            )))
    }

    pub(in crate::gpui_shell::settings_pane) fn theme_editor_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let editor = self.theme_editor.as_ref()?;
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let viewport = window.viewport_size();
        let compact = f32::from(viewport.width) < 760.0;
        let width = (f32::from(viewport.width) - 32.0).min(920.0);
        let height = (f32::from(viewport.height) - 48.0).min(760.0).max(280.0);
        let save_busy = editor.save_busy;
        let dirty = editor.dirty();
        let preview = self.theme_editor_preview(editor, colors, language, compact, cx);
        let common =
            self.theme_editor_common_fields(editor, language, colors, save_busy, window, cx);
        let common = if editor.advanced {
            common.child(self.theme_editor_advanced(editor, language, colors, cx))
        } else {
            common
        };
        let body = if compact {
            v_flex().w_full().gap(px(22.0)).child(preview).child(common)
        } else {
            h_flex()
                .w_full()
                .h_full()
                .min_h_0()
                .items_start()
                .gap(px(18.0))
                .child(
                    div()
                        .id("theme-editor-fields-scroll")
                        .debug_selector(|| "theme-editor-fields-scroll".to_owned())
                        .min_h_0()
                        .flex_1()
                        .h_full()
                        .overflow_y_scroll()
                        .pr(px(10.0))
                        .child(common),
                )
                .child(div().w(px(1.0)).h_full().bg(colors.line))
                .child(
                    div()
                        .id("theme-editor-preview-scroll")
                        .debug_selector(|| "theme-editor-preview-scroll".to_owned())
                        .h_full()
                        .min_h_0()
                        .flex_shrink_0()
                        .overflow_y_scroll()
                        .child(preview),
                )
        };
        let error = editor.error.clone();
        let dialog = v_flex()
            .id("theme-editor-dialog")
            .debug_selector(|| "theme-editor-dialog".to_owned())
            .role(Role::Dialog)
            .aria_label(language.text(Message::ThemeEditorTitle))
            .w(px(width))
            .h(px(height))
            .max_h(px(f32::from(viewport.height) - 28.0))
            .rounded(px(14.0))
            .border_1()
            .border_color(colors.control)
            .bg(colors.surface)
            .text_color(colors.ink)
            .shadow_2xl()
            .overflow_hidden()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.request_close_theme_editor(window, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .px(px(26.0))
                    .pt(px(24.0))
                    .pb(px(18.0))
                    .justify_between()
                    .border_b_1()
                    .border_color(colors.line)
                    .child(
                        h_flex()
                            .gap(px(10.0))
                            .child(
                                Button::new("theme-editor-back")
                                    .debug_selector(|| "theme-editor-back".to_owned())
                                    .icon(IconName::ArrowLeft)
                                    .ghost()
                                    .size(px(28.0))
                                    .tooltip(language.text(Message::ThemeEditorBack))
                                    .disabled(save_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.request_back_theme_editor(window, cx);
                                    })),
                            )
                            .child(
                                v_flex()
                                    .gap(px(3.0))
                                    .child(
                                        div()
                                            .text_size(px(19.0))
                                            .font_semibold()
                                            .child(language.text(Message::ThemeEditorTitle)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(colors.secondary)
                                            .child(language.text(Message::ThemeEditorDescription)),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(7.0))
                            .child(
                                Button::new("theme-editor-import")
                                    .debug_selector(|| "theme-editor-import".to_owned())
                                    .label(language.text(Message::ThemeEditorImport))
                                    .ghost()
                                    .small()
                                    .disabled(save_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_theme_import(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("theme-editor-export")
                                    .debug_selector(|| "theme-editor-export".to_owned())
                                    .label(language.text(Message::ThemeEditorExport))
                                    .ghost()
                                    .small()
                                    .disabled(save_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_theme_export(window, cx);
                                    })),
                            )
                            .child(div().w(px(1.0)).h(px(20.0)).bg(colors.line))
                            .child(
                                Button::new("theme-editor-close")
                                    .debug_selector(|| "theme-editor-close".to_owned())
                                    .icon(IconName::Close)
                                    .ghost()
                                    .size(px(28.0))
                                    .text_color(colors.secondary)
                                    .tooltip(language.text(Message::ThemeTransferClose))
                                    .disabled(save_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.request_close_theme_editor(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .id("theme-editor-scroll")
                    .debug_selector(|| "theme-editor-scroll".to_owned())
                    .min_h_0()
                    .flex_1()
                    .h_full()
                    .when(compact, |scroll| scroll.overflow_y_scroll())
                    .when(!compact, |scroll| scroll.overflow_hidden())
                    .px(px(26.0))
                    .py(px(22.0))
                    .child(body),
            )
            .when_some(error, |dialog, error| {
                dialog.child(
                    div()
                        .px(px(26.0))
                        .pb(px(8.0))
                        .text_size(px(11.0))
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .when(editor.saved, |dialog| {
                dialog.child(
                    div()
                        .px(px(26.0))
                        .pb(px(8.0))
                        .text_size(px(11.0))
                        .text_color(cx.theme().success)
                        .child(language.text(Message::ThemeEditorSaved)),
                )
            })
            .child(
                h_flex()
                    .px(px(26.0))
                    .py(px(16.0))
                    .justify_between()
                    .border_t_1()
                    .border_color(colors.line)
                    .child(
                        h_flex()
                            .min_w_0()
                            .flex_1()
                            .gap(px(9.0))
                            .child(
                                div().size(px(8.0)).flex_shrink_0().rounded_full().bg(if dirty {
                                    colors.primary
                                } else {
                                    colors.line
                                }),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap(px(2.0))
                                    .child(
                                        div().truncate().text_size(px(11.0)).font_medium().child(
                                            language.text(Message::ThemeEditorEditingStatus),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(10.0))
                                            .text_color(colors.secondary)
                                            .child(if dirty {
                                                language.text(Message::ThemeEditorEditingChanged)
                                            } else {
                                                language.text(Message::ThemeEditorEditingUnchanged)
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .child(
                                Button::new("theme-editor-cancel")
                                    .debug_selector(|| "theme-editor-cancel".to_owned())
                                    .label(language.text(Message::ThemePickerCancel))
                                    .ghost()
                                    .disabled(save_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.request_close_theme_editor(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("theme-editor-save")
                                    .debug_selector(|| "theme-editor-save".to_owned())
                                    .label(if save_busy {
                                        language.text(Message::ThemeEditorSaving)
                                    } else {
                                        language.text(Message::ThemeEditorSaveOnly)
                                    })
                                    .disabled(save_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_theme_editor(false, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("theme-editor-save-apply")
                                    .debug_selector(|| "theme-editor-save-apply".to_owned())
                                    .label(language.text(Message::ThemeEditorApply))
                                    .with_variant(ButtonVariant::Primary)
                                    .disabled(save_busy || editor.templates_loading)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_theme_editor(true, window, cx);
                                    })),
                            ),
                    ),
            )
            .focus_trap("theme-editor-focus-trap", &editor.focus);
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(gpui::point(px(0.0), px(0.0)))
                    .child(
                        div()
                            .id("theme-editor-overlay")
                            .w(viewport.width)
                            .h(viewport.height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .occlude()
                            .bg(colors.scrim)
                            .child(dialog),
                    ),
            )
            // Component Select/ColorPicker popovers use deferred priority 1.
            // Keep this modal below its own popovers so they remain visible
            // and receive pointer events; confirmations remain above both.
            .with_priority(0)
            .into_any_element(),
        )
    }

    fn theme_editor_common_fields(
        &self,
        editor: &ThemeEditor,
        language: crate::display::UiLanguage,
        colors: AppearanceColors,
        save_busy: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let field = |selector: &'static str, label: &'static str, input: &Entity<InputState>| {
            let focused = input.read(cx).focus_handle(cx).is_focused(window);
            v_flex()
                .min_w_0()
                .debug_selector(move || selector.to_owned())
                .gap(px(5.0))
                .child(div().text_size(px(11.0)).text_color(colors.secondary).child(label))
                .child(
                    div()
                        .w_full()
                        .h(px(34.0))
                        .border_b_1()
                        .border_color(if focused { colors.primary } else { colors.line })
                        .child(
                            Input::new(input)
                                .w_full()
                                .h_full()
                                .bordered(false)
                                .focus_bordered(false)
                                .appearance(false)
                                .rounded_none()
                                .disabled(save_busy),
                        ),
                )
        };
        let selected_template = editor.templates.get(editor.selected_index());
        let template_color = selected_template
            .map(|template| template.definition.terminal.background)
            .unwrap_or(editor.draft.terminal.background);
        let source_name = selected_template
            .map(|template| template.label.to_string())
            .unwrap_or_else(|| editor.draft.name.clone());
        let font_field = v_flex()
            .flex_1()
            .min_w_0()
            .debug_selector(|| "theme-editor-font".to_owned())
            .gap(px(5.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.secondary)
                    .child(language.text(Message::ThemeEditorFontFamily)),
            )
            .child(div().w_full().h(px(34.0)).border_b_1().border_color(colors.line).child(
                Select::new(&editor.font_select).disabled(save_busy || editor.templates_loading),
            ));
        v_flex()
            .min_w_0()
            .flex_1()
            .gap(px(14.0))
            .child(
                v_flex()
                    .gap(px(5.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.secondary)
                            .child(language.text(Message::ThemeEditorTemplate)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(8.0))
                            .items_center()
                            .child(
                                div()
                                    .size(px(18.0))
                                    .flex_shrink_0()
                                    .rounded(px(5.0))
                                    .border_1()
                                    .border_color(colors.line)
                                    .bg(theme_color(template_color, 1.0)),
                            )
                            .child(
                                div()
                                    .debug_selector(|| "theme-editor-template".to_owned())
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        Select::new(&editor.template_select)
                                            .disabled(save_busy || editor.templates_loading),
                                    ),
                            ),
                    )
                    .child(div().text_size(px(10.5)).text_color(colors.secondary).child(
                        language.format(Message::ThemeEditorBasedOn, &[("name", &source_name)]),
                    )),
            )
            .child(field(
                "theme-editor-name",
                language.text(Message::ThemeEditorName),
                &editor.name_input,
            ))
            .when(editor.templates_loading, |view| {
                view.child(
                    div()
                        .text_size(px(10.5))
                        .text_color(colors.secondary)
                        .child(language.text(Message::ThemeEditorLoading)),
                )
            })
            .child(
                v_flex()
                    .gap(px(9.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_semibold()
                            .child(language.text(Message::ThemeEditorCommon)),
                    )
                    .child(
                        self.theme_editor_color_grid(
                            editor, language, colors, save_busy, window, cx,
                        ),
                    ),
            )
            .child(
                h_flex().w_full().gap(px(10.0)).child(font_field).child(
                    field(
                        "theme-editor-font-size",
                        language.text(Message::ThemeEditorFontSize),
                        &editor.font_size_input,
                    )
                    .w(px(108.0))
                    .flex_shrink_0(),
                ),
            )
            .child(
                h_flex().w_full().gap(px(10.0)).children([
                    field(
                        "theme-editor-line-height",
                        language.text(Message::ThemeEditorLineHeight),
                        &editor.line_height_input,
                    )
                    .flex_1()
                    .min_w_0()
                    .into_any_element(),
                    field(
                        "theme-editor-opacity",
                        language.text(Message::ThemeEditorOpacity),
                        &editor.opacity_input,
                    )
                    .flex_1()
                    .min_w_0()
                    .into_any_element(),
                ]),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(9.0))
                    .child(
                        Button::new("theme-editor-advanced-toggle")
                            .debug_selector(|| "theme-editor-advanced-toggle".to_owned())
                            .label(if editor.advanced {
                                language.text(Message::ThemeEditorAdvancedHide)
                            } else {
                                language.text(Message::ThemeEditorAdvanced)
                            })
                            .ghost()
                            .justify_start()
                            .px_0()
                            .text_color(colors.primary)
                            .disabled(save_busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(editor) = this.theme_editor.as_mut() {
                                    editor.advanced = !editor.advanced;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("theme-editor-reset")
                            .debug_selector(|| "theme-editor-reset".to_owned())
                            .label(language.text(Message::ThemeEditorResetShort))
                            .ghost()
                            .xsmall()
                            .text_color(colors.secondary)
                            .disabled(save_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reset_theme_editor(window, cx);
                            })),
                    ),
            )
    }

    fn theme_editor_color_grid(
        &self,
        editor: &ThemeEditor,
        language: crate::display::UiLanguage,
        colors: AppearanceColors,
        save_busy: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let template_definition = editor
            .templates
            .get(editor.selected_index())
            .map(|template| template.definition.clone());
        let card = |selector: &'static str,
                    slot: ThemeColorSlot,
                    message: Message,
                    input: &Entity<InputState>| {
            let focused = input.read(cx).focus_handle(cx).is_focused(window);
            let color = slot.read(&editor.draft);
            let reset_color = template_definition
                .as_ref()
                .map(|definition| slot.read(definition))
                .unwrap_or_else(|| slot.read(&editor.baseline));
            let swatch_selector = format!("{selector}-swatch");
            let reset_selector = format!("theme-editor-reset-{selector}");
            v_flex()
                .debug_selector(move || selector.to_owned())
                .flex_1()
                .min_w_0()
                .gap(px(7.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.secondary)
                        .child(language.text(message)),
                )
                .child(
                    h_flex()
                        .w_full()
                        .items_end()
                        .gap(px(7.0))
                        .child(
                            div()
                                .id(SharedString::from(swatch_selector.clone()))
                                .debug_selector(move || swatch_selector.clone())
                                .size(px(22.0))
                                .flex_shrink_0()
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(colors.line)
                                .bg(theme_color(color, 1.0))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_theme_color_picker(slot, window, cx);
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(34.0))
                                .border_b_1()
                                .border_color(if focused { colors.primary } else { colors.line })
                                .child(
                                    Input::new(input)
                                        .w_full()
                                        .h_full()
                                        .bordered(false)
                                        .focus_bordered(false)
                                        .appearance(false)
                                        .rounded_none()
                                        .disabled(save_busy),
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(reset_selector.clone()))
                                .debug_selector(move || reset_selector.clone())
                                .icon(IconName::Undo2)
                                .ghost()
                                .xsmall()
                                .tooltip(language.text(Message::ThemeEditorResetShort))
                                .disabled(save_busy || color == reset_color)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.reset_theme_color_slot(slot, window, cx);
                                })),
                        ),
                )
        };
        v_flex()
            .w_full()
            .gap(px(10.0))
            .child(
                h_flex()
                    .w_full()
                    .gap(px(12.0))
                    .child(card(
                        "theme-editor-background",
                        ThemeColorSlot::Background,
                        Message::ThemeEditorBackground,
                        &editor.background_input,
                    ))
                    .child(card(
                        "theme-editor-foreground",
                        ThemeColorSlot::Foreground,
                        Message::ThemeEditorForeground,
                        &editor.foreground_input,
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap(px(12.0))
                    .child(card(
                        "theme-editor-accent",
                        ThemeColorSlot::Accent,
                        Message::ThemeEditorAccent,
                        &editor.accent_input,
                    ))
                    .child(card(
                        "theme-editor-cursor",
                        ThemeColorSlot::Cursor,
                        Message::ThemeEditorCursor,
                        &editor.cursor_input,
                    )),
            )
    }

    fn reset_theme_color_slot(
        &mut self,
        slot: ThemeColorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_theme_color_picker();
        let (input, value) = {
            let Some(editor) = self.theme_editor.as_mut() else { return };
            if editor.save_busy {
                return;
            }
            let source = editor
                .templates
                .get(editor.selected_index())
                .map(|template| &template.definition)
                .unwrap_or(&editor.baseline);
            match slot {
                ThemeColorSlot::Background => {
                    editor.draft.terminal.background = source.terminal.background
                },
                ThemeColorSlot::Foreground => {
                    editor.draft.terminal.foreground = source.terminal.foreground
                },
                ThemeColorSlot::Accent => editor.draft.ui = source.ui,
                ThemeColorSlot::Cursor => editor.draft.terminal.cursor = source.terminal.cursor,
            }
            let color = slot.read(&editor.draft);
            let field = slot.editor_input();
            editor.input_values.insert(field, format_hex_rgb(color));
            editor.invalid_inputs.remove(&field);
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
                        .text(Message::ThemeEditorInvalidValue)
                        .to_owned(),
                )
            };
            let input = match field {
                EditorInput::Background => editor.background_input.clone(),
                EditorInput::Foreground => editor.foreground_input.clone(),
                EditorInput::Accent => editor.accent_input.clone(),
                EditorInput::Cursor => editor.cursor_input.clone(),
                _ => return,
            };
            (input, format_hex_rgb(color))
        };
        input.update(cx, |state, cx| state.set_value(value, window, cx));
        if let Some(editor) = self.theme_editor.as_mut() {
            editor.sync_inherited_cursor_input(window, cx);
        }
        cx.notify();
    }

    fn theme_editor_advanced(
        &self,
        editor: &ThemeEditor,
        _language: crate::display::UiLanguage,
        colors: AppearanceColors,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(advanced) = editor.advanced_editor.as_ref() else {
            return v_flex().into_any_element();
        };
        let pane = cx.entity().downgrade();
        advanced
            .render(colors, cx.theme().danger, editor.save_busy, move |checked, _, cx| {
                let _ = pane.update(cx, |this, cx| {
                    this.set_theme_advanced_shadow(*checked, cx);
                });
            })
            .into_any_element()
    }
}

fn theme_color([r, g, b]: [u8; 3], alpha: f32) -> Hsla {
    gpui::Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: alpha.clamp(0.0, 1.0),
    }
    .into()
}
