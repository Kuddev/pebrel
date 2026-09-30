use super::appearance_picker::{AppearanceColors, AppearanceSelection, picker_columns};
use super::*;
use crate::i18n::Message;

pub(super) const THEME_ORDER: [ThemeName; ThemeName::BUILTIN.len()] = ThemeName::BUILTIN;

pub(super) fn theme_foreground(name: ThemeName) -> [u8; 3] {
    let theme = name.term_theme();
    if let Some(exact) = theme.exact {
        exact.foreground
    } else if theme.is_light {
        nebula_settings::LIGHT_FOREGROUND
    } else {
        let foreground = chrome_theme(name).card_ink().fg;
        [foreground.r, foreground.g, foreground.b]
    }
}

fn on_solid_rgb(color: [u8; 3]) -> Hsla {
    let luma =
        0.2126 * f32::from(color[0]) + 0.7152 * f32::from(color[1]) + 0.0722 * f32::from(color[2]);
    if luma < 150.0 { rgb_hsla(248, 250, 252) } else { rgb_hsla(15, 23, 42) }
}

/// 色盘是固定的小型 UI 资源：一次生成抗锯齿圆面，避免逐帧拼接色带产生接缝。
fn rainbow_swatch() -> gpui::Div {
    static IMAGE: std::sync::OnceLock<std::sync::Arc<gpui::RenderImage>> =
        std::sync::OnceLock::new();
    let image = IMAGE.get_or_init(|| {
        std::sync::Arc::new(gpui::RenderImage::new([image::Frame::new(rainbow_pixels())]))
    });
    div().size_full().child(gpui::img(image.clone()).size_full())
}

fn rainbow_pixels() -> image::RgbaImage {
    const SIZE: u32 = 96;
    let center = (SIZE as f32 - 1.0) * 0.5;
    let radius = center - 0.5;
    image::RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let dx = x as f32 - center;
        let dy = y as f32 - center;
        let distance = dx.hypot(dy);
        let alpha = ((radius - distance + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
        let hue = (dy.atan2(dx).to_degrees() + 90.0).rem_euclid(360.0);
        let saturation = (distance / radius).clamp(0.0, 1.0) * 0.85;
        let rgb = crate::display::hsv_to_rgb(hue, saturation, 0.96);
        // RenderImage 的上传顺序为 BGRA，透明圆角不能依赖 GPUI 的矩形裁剪。
        image::Rgba([rgb.b, rgb.g, rgb.r, alpha])
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn color_wheel_has_transparent_corners_and_bgra_red_at_the_top() {
        let pixels = super::rainbow_pixels();
        assert_eq!(pixels.dimensions(), (96, 96));
        assert_eq!(pixels.get_pixel(0, 0).0[3], 0);
        assert_eq!(pixels.get_pixel(47, 47).0[3], 255);
        let top = pixels.get_pixel(47, 4).0;
        assert!(top[2] > top[0] + 100, "BGRA upload must keep the red hue red");
        let neighbor = pixels.get_pixel(48, 4).0;
        for channel in 0..3 {
            assert!(top[channel].abs_diff(neighbor[channel]) < 12, "no striped hue seams");
        }
    }
}

#[derive(Clone, Copy)]
struct ThemeSampleColors {
    background: [u8; 3],
    foreground: [u8; 3],
    accent: [u8; 3],
    modified: [u8; 3],
    ready: [u8; 3],
    cursor: [u8; 3],
}

pub(super) fn theme_sample(name: ThemeName, token: bool, compact: bool) -> gpui::Div {
    theme_sample_with_foreground(name, None, token, compact)
}

pub(super) fn theme_sample_with_foreground(
    name: ThemeName,
    foreground_override: Option<[u8; 3]>,
    token: bool,
    compact: bool,
) -> gpui::Div {
    let theme = name.term_theme();
    let palette = name.reviewed_palette();
    let foreground = foreground_override.unwrap_or_else(|| theme_foreground(name));
    theme_sample_values(
        ThemeSampleColors {
            background: theme.background,
            foreground,
            accent: palette.accent,
            modified: theme.exact.map_or_else(
                || if theme.is_light { nebula_settings::LIGHT_ANSI[3] } else { palette.yellow },
                |exact| exact.ansi[3],
            ),
            ready: theme.exact.map_or_else(
                || if theme.is_light { nebula_settings::LIGHT_ANSI[2] } else { palette.green },
                |exact| exact.ansi[2],
            ),
            cursor: theme.exact.and_then(|exact| exact.cursor).unwrap_or(foreground),
        },
        token,
        compact,
    )
}

/// Render a small theme thumbnail from a complete custom snapshot.  Keeping
/// this renderer beside the built-in card prevents the appearance trigger and
/// the picker from drifting into two different miniature representations.
pub(super) fn theme_definition_sample(
    definition: &nebula_settings::ThemeDefinition,
    foreground_override: Option<[u8; 3]>,
    token: bool,
    compact: bool,
) -> gpui::Div {
    let ui = definition.resolved_ui();
    theme_sample_values(
        ThemeSampleColors {
            background: definition.terminal.background,
            foreground: foreground_override.unwrap_or(definition.terminal.foreground),
            accent: ui.accent,
            modified: definition.terminal.ansi(3).unwrap_or(ui.warning),
            ready: definition.terminal.ansi(2).unwrap_or(ui.success),
            cursor: definition.terminal.cursor.unwrap_or(definition.terminal.foreground),
        },
        token,
        compact,
    )
}

fn theme_sample_values(colors: ThemeSampleColors, token: bool, compact: bool) -> gpui::Div {
    let color = |rgb: [u8; 3]| rgb_hsla(rgb[0], rgb[1], rgb[2]);
    let ink = color(colors.foreground);
    let accent = color(colors.accent);
    let background = color(colors.background);
    if !token {
        let row = || h_flex().w_full().min_w_0().gap(px(4.0)).overflow_hidden();
        return v_flex()
            .w_full()
            .h(px(if compact { 68.0 } else { 76.0 }))
            .px(px(8.0))
            .py(px(7.0))
            .rounded(px(3.0))
            .border_1()
            .border_color(ink.opacity(0.10))
            .bg(background)
            .font(crate::font_install::gpui_font_with_fallbacks(
                "Maple Mono Normal NF CN, Cascadia Code, Consolas, monospace",
            ))
            .text_size(px(if compact { 9.5 } else { 10.0 }))
            .line_height(gpui::relative(1.5))
            .text_color(ink)
            .justify_between()
            .child(row().child(div().text_color(accent).child("❯")).child("git status"))
            .child(
                row()
                    .child(div().text_color(color(colors.modified)).child("M"))
                    .child(div().min_w_0().truncate().child("src/main.rs")),
            )
            .child(
                row()
                    .child(div().text_color(color(colors.ready)).child("✓"))
                    .child(div().flex_1().text_color(ink.opacity(0.75)).child("ready"))
                    .child(div().w(px(4.0)).h(px(8.0)).bg(color(colors.cursor))),
            );
    }
    // 设置入口中的小图标仍保留色带；完整主题卡使用上面的终端内容。
    let line = |width, color| {
        div().w(gpui::relative(width)).h(px(if token { 3.0 } else { 4.0 })).rounded_full().bg(color)
    };
    v_flex()
        .w_full()
        .h(px(if token {
            45.0
        } else if compact {
            60.0
        } else {
            76.0
        }))
        .p(px(0.0))
        .rounded(px(8.0))
        .bg(background)
        .child(
            v_flex()
                .size_full()
                .justify_center()
                .gap(px(if token { 4.0 } else { 6.0 }))
                .p(px(if token { 5.0 } else { 8.0 }))
                .rounded(px(0.0))
                .bg(background)
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(5.0))
                        .child(line(0.15, accent))
                        .child(line(0.49, ink.opacity(0.8))),
                )
                .child(line(0.79, ink.opacity(0.55)))
                .child(line(0.43, accent.opacity(0.8))),
        )
}

impl SettingsPane {
    pub(super) fn terminal_appearance_preview(
        &self,
        name: ThemeName,
        typography: bool,
        compact: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::Div {
        self.terminal_appearance_preview_with_foreground(
            name, typography, compact, None, window, cx,
        )
    }

    pub(super) fn terminal_appearance_preview_with_foreground(
        &self,
        name: ThemeName,
        typography: bool,
        compact: bool,
        foreground_override: Option<[u8; 3]>,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let term = name.term_theme();
        let background = if typography && !self.runtime.follow_system_theme {
            self.runtime.background.unwrap_or(term.background)
        } else {
            term.background
        };
        let accent = chrome_theme(name).accent();
        let accent = rgb_hsla(accent.r, accent.g, accent.b);
        let foreground = foreground_override.unwrap_or_else(|| theme_foreground(name));
        let ink = rgb_hsla(foreground[0], foreground[1], foreground[2]);
        let colors = AppearanceColors::current(cx);
        let family = self.current_font_chain(cx);
        let size = self.terminal_font_size_px(cx);
        v_flex()
            .w_full()
            .min_w_0()
            .rounded(px(9.0))
            .overflow_hidden()
            .border_1()
            .border_color(colors.control)
            .bg(rgb_hsla(background[0], background[1], background[2]))
            .text_color(ink)
            .child(
                h_flex()
                    .w_full()
                    .h(px(34.0))
                    .px(px(12.0))
                    .justify_between()
                    .flex_shrink_0()
                    // GPUI's overflow mask is rectangular; each painted surface
                    // must carry the radius where it meets the outer border.
                    .rounded_t(px(8.0))
                    .bg(rgb_hsla(background[0], background[1], background[2]))
                    .child(
                        h_flex()
                            .gap(px(6.0))
                            .text_size(px(10.0))
                            .child(super::app_icon::icon_image(
                                crate::app_icon::selected(),
                                16.0,
                                window,
                            ))
                            .child(if cfg!(windows) { "PowerShell" } else { "Terminal" }),
                    )
                    .child(div().text_size(px(9.0)).text_color(ink.opacity(0.6)).child(language.text(Message::ThemeTransferPreview))),
            )
            .child(
                v_flex()
                    .id(if typography {
                        "appearance-font-preview"
                    } else {
                        "appearance-theme-preview"
                    })
                    .w_full()
                    .min_h(px(if compact { 110.0 } else { 181.0 }))
                    .px(px(if typography { 16.0 } else { 13.0 }))
                    .py(px(if compact { 12.0 } else { 19.0 }))
                    .overflow_x_scroll()
                    .font(crate::font_install::gpui_font_with_fallbacks(&family))
                    .text_size(px(size * 0.82))
                    .line_height(gpui::relative(1.75))
                    .child(
                        h_flex()
                            .gap(px(7.0))
                            .child(div().text_color(accent).child("❯"))
                            .child("pebrel --version"),
                    )
                    .child(div().mt(px(if compact { 6.0 } else { 10.0 })).text_size(px(size * 0.75)).child(format!(
                        "{} · {}",
                        crate::brand::NAME,
                        std::env::consts::OS
                    )))
                    .child(
                        h_flex()
                            .gap(px(7.0))
                            .text_size(px(size * 0.73))
                            .child(Icon::new(IconName::Check).size(px(12.0)).text_color(accent))
                            .child(language.text(Message::ThemePickerPreviewReady)),
                    )
                    .child(
                        h_flex()
                            .mt(px(if compact { 8.0 } else { 17.0 }))
                            .gap(px(7.0))
                            .child(div().text_color(accent).child("❯"))
                            .child(div().w(px(7.0)).h(px(13.0)).bg(ink.opacity(0.85))),
                    ),
            )
    }

    pub(super) fn theme_picker_body(
        &self,
        width: f32,
        compact: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let picker = self.appearance_picker.as_ref().unwrap();
        let draft = picker.draft;
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let compact_preview = compact || f32::from(window.viewport_size().height) < 740.0;
        let columns = picker_columns(true, f32::from(window.viewport_size().width));
        let grid_width = if compact { width } else { width - 230.0 - 25.0 };
        let gap = if compact { 6.0 } else { 9.0 };
        let option_width = (grid_width - (columns - 1) as f32 * gap) / columns as f32;
        let grid = h_flex()
            .id("appearance-theme-grid")
            .debug_selector(|| "appearance-theme-grid".to_owned())
            .w(px(grid_width))
            .flex_shrink_0()
            .flex_wrap()
            .items_start()
            .gap_x(px(gap))
            .gap_y(px(12.0))
            .role(gpui::accesskit::Role::RadioGroup)
            .aria_label(language.text(Message::ThemePickerTitle))
            .children(picker.choices().into_iter().map(|choice| {
                let selected = draft == choice;
                let sample = match choice {
                    AppearanceSelection::Theme(name) => theme_sample_with_foreground(
                        name,
                        if selected { picker.foreground_override } else { None },
                        false,
                        compact,
                    ),
                    AppearanceSelection::Custom(index) => picker
                        .custom_definitions
                        .get(index)
                        .map(|definition| {
                            theme_definition_sample(
                                definition,
                                if selected { picker.foreground_override } else { None },
                                false,
                                compact,
                            )
                        })
                        .unwrap_or_else(div),
                    AppearanceSelection::Icon(_) => div(),
                };
                let content =
                    v_flex().w_full().p(px(if compact { 6.0 } else { 8.0 })).child(sample).child(
                        h_flex()
                            .mt(px(8.0))
                            .justify_between()
                            .gap(px(3.0))
                            .text_size(px(if compact { 10.0 } else { 11.5 }))
                            .text_color(colors.ink)
                            .font_medium()
                            .when(selected, |caption| caption.font_semibold())
                            .child(div().truncate().child(picker.choice_label(choice, language)))
                            .child(if selected {
                                Icon::new(IconName::Check)
                                    .size(px(12.0))
                                    .text_color(colors.primary)
                                    .into_any_element()
                            } else {
                                let light = match choice {
                                    AppearanceSelection::Theme(name) => name.term_theme().is_light,
                                    AppearanceSelection::Custom(index) => picker
                                        .custom_definitions
                                        .get(index)
                                        .is_some_and(|definition| definition.appearance.is_light()),
                                    AppearanceSelection::Icon(_) => false,
                                };
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(10.0))
                                    .text_color(colors.secondary)
                                    .child(language.text(if light {
                                        Message::ThemePickerLight
                                    } else {
                                        Message::ThemePickerDark
                                    }))
                                    .into_any_element()
                            }),
                    );
                self.appearance_option(choice, option_width, content, window, cx)
            }));
        let picker_status = if picker.custom_loading {
            Some(
                div()
                    .text_size(px(10.5))
                    .text_color(colors.secondary)
                    .child(language.text(Message::ThemePickerLoading)),
            )
        } else {
            picker.custom_load_error.as_ref().map(|error| {
                div()
                    .text_size(px(10.5))
                    .text_color(colors.secondary)
                    .child(language.format(Message::ThemeEditorLibraryError, &[("error", error)]))
            })
        };
        let grid_column = v_flex()
            .w(px(grid_width))
            .gap(px(8.0))
            .when_some(picker_status, |column, status| column.child(status))
            .child(grid);
        let (is_light, original_foreground, preview) = match draft {
            AppearanceSelection::Theme(name) => (
                chrome_theme(name).palette().is_light,
                theme_foreground(name),
                self.terminal_appearance_preview_with_foreground(
                    name,
                    false,
                    compact_preview,
                    Some(picker.foreground_override.unwrap_or_else(|| theme_foreground(name))),
                    window,
                    cx,
                ),
            ),
            AppearanceSelection::Custom(index) => {
                let definition = picker.custom_definitions.get(index);
                let foreground = definition
                    .map(|definition| definition.terminal.foreground)
                    .unwrap_or([255, 255, 255]);
                (
                    definition.is_some_and(|definition| definition.appearance.is_light()),
                    foreground,
                    definition
                        .map(|definition| {
                            theme_definition_sample(
                                definition,
                                Some(picker.foreground_override.unwrap_or(foreground)),
                                false,
                                compact,
                            )
                        })
                        .unwrap_or_else(div),
                )
            },
            AppearanceSelection::Icon(_) => unreachable!(),
        };
        let recommendations = nebula_settings::foreground_recommendations(
            match draft {
                AppearanceSelection::Theme(name) => name.term_theme().background,
                AppearanceSelection::Custom(index) => picker
                    .custom_definitions
                    .get(index)
                    .map(|definition| definition.terminal.background)
                    .unwrap_or([15, 23, 42]),
                AppearanceSelection::Icon(_) => [15, 23, 42],
            },
            original_foreground,
        );
        let selected_foreground = picker.foreground_override.unwrap_or(original_foreground);
        let preview = v_flex()
            .w(px(if compact { width } else { 230.0 }))
            .flex_shrink_0()
            .child(preview)
            .child(
                div()
                    .mt(px(if compact_preview { 11.0 } else { 19.0 }))
                    .text_size(px(13.0))
                    .font_semibold()
                    .child(picker.choice_label(draft, language)),
            )
            .child(div().mt(px(5.0)).text_size(px(10.5)).text_color(colors.secondary).child(
                if is_light {
                    language.text(Message::ThemePickerLight)
                } else {
                    language.text(Message::ThemePickerDark)
                },
            ))
            .child(
                h_flex()
                    .mt(px(if compact_preview { 9.0 } else { 20.0 }))
                    .pt(px(if compact_preview { 9.0 } else { 17.0 }))
                    .gap(px(8.0))
                    .border_t_1()
                    .border_color(colors.line)
                    .children(recommendations.iter().copied().enumerate().map(|(index, color)| {
                        let selected = selected_foreground == color;
                        let focus = picker.foreground_focus[index].clone();
                        let description = format!(
                            "{} {}",
                            match index {
                                0 => language.text(Message::ThemePickerTextOriginal),
                                1 => language.text(Message::ThemePickerTextCool),
                                _ => language.text(Message::ThemePickerTextWarm),
                            },
                            format_hex_rgb(color)
                        );
                        div()
                            .id(("theme-foreground-swatch", index))
                            .debug_selector(move || format!("theme-foreground-swatch-{index}"))
                            .size(px(32.0))
                            .rounded(px(6.0))
                            .p(px(3.0))
                            .track_focus(&focus.clone().tab_stop(true))
                            .role(gpui::accesskit::Role::RadioButton)
                            .aria_label(description.clone())
                            .aria_toggled(if selected {
                                gpui::accesskit::Toggled::True
                            } else {
                                gpui::accesskit::Toggled::False
                            })
                            .border_1()
                            .border_color(if selected { colors.primary } else { colors.control })
                            .when(focus.is_focused(window), |swatch| {
                                swatch.border_color(colors.ink)
                            })
                            .hover(move |swatch| swatch.border_color(colors.primary))
                            .child(
                                div()
                                    .size_full()
                                    .rounded(px(3.0))
                                    .bg(rgb_hsla(color[0], color[1], color[2])),
                            )
                            .cursor_pointer()
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(description.clone())
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_theme_foreground(color, window, cx);
                            }))
                            .on_key_down(cx.listener(
                                move |this, event: &KeyDownEvent, window, cx| {
                                    if event.keystroke.key.eq_ignore_ascii_case("enter")
                                        || event.keystroke.key.eq_ignore_ascii_case("space")
                                    {
                                        cx.stop_propagation();
                                        this.set_theme_foreground(color, window, cx);
                                    }
                                },
                            ))
                    }))
                    .child({
                        let custom = picker.foreground_override.is_some()
                            && !recommendations.contains(&selected_foreground);
                        let description = language.text(Message::ThemePickerTextCustom);
                        let focus = picker.foreground_focus[3].clone();
                        div()
                            .id("theme-foreground-custom-swatch")
                            .debug_selector(|| "theme-foreground-custom-swatch".to_owned())
                            .size(px(32.0))
                            .rounded_full()
                            .track_focus(&focus.clone().tab_stop(true))
                            .role(gpui::accesskit::Role::Button)
                            .aria_label(description)
                            .overflow_hidden()
                            .border_1()
                            .p(px(3.0))
                            .border_color(if custom { colors.primary } else { colors.control })
                            .when(focus.is_focused(window), |swatch| {
                                swatch.border_color(colors.ink)
                            })
                            .cursor_pointer()
                            .hover(move |swatch| swatch.border_color(colors.primary))
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(description).build(window, cx)
                            })
                            .child(rainbow_swatch())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_theme_foreground_picker(window, cx);
                            }))
                            .on_key_down(cx.listener(
                                move |this, event: &KeyDownEvent, window, cx| {
                                    if event.keystroke.key.eq_ignore_ascii_case("enter")
                                        || event.keystroke.key.eq_ignore_ascii_case("space")
                                    {
                                        cx.stop_propagation();
                                        this.open_theme_foreground_picker(window, cx);
                                    }
                                },
                            ))
                    }),
            )
            .child(
                div()
                    .mt(px(if compact_preview { 9.0 } else { 20.0 }))
                    .text_size(px(10.5))
                    .line_height(gpui::relative(1.7))
                    .text_color(colors.secondary)
                    .child(if self.runtime.follow_system_theme {
                        language.text(Message::ThemePickerManualHint)
                    } else {
                        language.text(Message::ThemePickerPreviewHint)
                    }),
            );
        if compact {
            v_flex().w(px(width)).gap(px(20.0)).child(preview).child(grid_column)
        } else {
            h_flex().w(px(width)).items_start().gap(px(25.0)).child(grid_column).child(preview)
        }
    }
}
