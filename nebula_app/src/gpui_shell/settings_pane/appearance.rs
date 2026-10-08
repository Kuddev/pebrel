use super::appearance_picker::AppearanceColors;
use super::*;

impl SettingsPane {
    pub(super) fn finish_font_size_edit(
        &mut self,
        apply: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = self.font_size_editing.take() else { return };
        if apply {
            let value = self.font_size_input.read(cx).value();
            if let Ok(size) = value.trim().parse::<f32>() {
                if size.is_finite() {
                    let (key, min, max) =
                        if ui { ("ui_font_size", 10.0, 24.0) } else { ("font_size", 4.0, 96.0) };
                    self.persist(&[(key, format!("{:.2}", size.clamp(min, max)))], cx);
                }
            }
        }
        if self.font_size_input.read(cx).focus_handle(cx).is_focused(window) {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    fn appearance_trigger(
        &self,
        theme: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let base_px = self.font_size_px(cx);
        let description_px = base_px * super::design::DESC_SCALE;
        let resolved = crate::gpui_shell::theme::resolved_theme(cx);
        let name = resolved.base_name();
        let icon = crate::app_icon::selected();
        let label: SharedString = if theme {
            resolved
                .definition()
                .map(|definition| definition.name.clone().into())
                .unwrap_or_else(|| chrome_theme(name).short_label().into())
        } else {
            language.pick(icon.palette().name_zh, icon.palette().name_en).into()
        };
        let action = if theme {
            language.pick("更换主题", "Change theme")
        } else {
            language.pick("更换图标", "Change icon")
        };
        let focus = if theme { &self.theme_picker_trigger } else { &self.icon_picker_trigger };
        let sample = if theme {
            let thumbnail = if let Some(definition) = resolved.definition() {
                super::theme_picker::theme_definition_sample(
                    definition,
                    Some(resolved.terminal_foreground()),
                    true,
                    false,
                )
            } else {
                super::theme_picker::theme_sample_with_foreground(
                    name,
                    resolved.foreground_override(),
                    true,
                    false,
                )
            };
            div().size(px(45.0)).flex_shrink_0().child(thumbnail)
        } else {
            super::app_icon::icon_image(icon, 45.0, window)
        };
        h_flex()
            .id(if theme { "open-theme-picker" } else { "open-icon-picker" })
            .debug_selector(move || {
                if theme { "open-theme-picker" } else { "open-icon-picker" }.to_owned()
            })
            .track_focus(&focus.clone().tab_stop(true))
            .role(gpui::accesskit::Role::Button)
            .aria_label(format!("{action}: {label}"))
            .w_full()
            .min_w(px(166.0))
            .max_w(px(250.0))
            .flex_shrink_0()
            .gap(px(11.0))
            .py(px(8.0))
            .pl(px(8.0))
            .pr(px(10.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(if focus.is_focused(window) {
                colors.control
            } else {
                gpui::transparent_black()
            })
            .hover(move |button| button.bg(colors.subtle).border_color(colors.line))
            .cursor_pointer()
            .child(sample)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(div().text_size(px(base_px)).font_medium().truncate().child(label))
                    .child(
                        div()
                            .text_size(px(description_px))
                            .text_color(colors.secondary)
                            .child(action),
                    ),
            )
            .child(Icon::new(IconName::ChevronRight).size(px(14.0)).text_color(colors.secondary))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_appearance_picker(theme, window, cx)
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    this.open_appearance_picker(theme, window, cx);
                }
            }))
            .into_any_element()
    }

    pub(super) fn font_size_row(&self, ui: bool, cx: &Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let size = if ui { self.font_size_px(cx) } else { self.terminal_font_size_px(cx) };
        let (key, min, max) =
            if ui { ("ui_font_size", 10.0, 24.0) } else { ("font_size", 4.0, 96.0) };
        let stepper = h_flex()
            .w(px(142.0))
            .h(px(36.0))
            .items_center()
            .child(
                Button::new(SharedString::from(format!("{key}-smaller")))
                    .icon(IconName::Minus)
                    .ghost()
                    .size(px(34.0))
                    .disabled(size <= min)
                    .tooltip(language.pick("减小字号", "Decrease font size"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let size =
                            if ui { this.font_size_px(cx) } else { this.terminal_font_size_px(cx) };
                        let next = (size.ceil() - 1.0).clamp(min, max);
                        this.persist(&[(key, format!("{next:.2}"))], cx);
                    })),
            )
            .child(if self.font_size_editing == Some(ui) {
                Input::new(&self.font_size_input)
                    .appearance(false)
                    .focus_bordered(false)
                    .cleanable(false)
                    .w(px(74.0))
                    .h(px(34.0))
                    .aria_label(language.text(if ui {
                        crate::i18n::Message::SettingsFontUiSize
                    } else {
                        crate::i18n::Message::CommonFontSize
                    }))
                    .into_any_element()
            } else {
                Button::new(SharedString::from(format!("{key}-edit")))
                    .debug_selector(move || format!("{key}-edit"))
                    .ghost()
                    .w(px(74.0))
                    .h(px(34.0))
                    .label(format!("{size} px"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.font_size_editing = Some(ui);
                        this.font_size_input.update(cx, |input, cx| {
                            let value = size.to_string();
                            input.set_value(value.clone(), window, cx);
                            input.focus(window, cx);
                            input.set_selected_range(0..value.len(), cx);
                        });
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .child(
                Button::new(SharedString::from(format!("{key}-larger")))
                    .icon(IconName::Plus)
                    .ghost()
                    .size(px(34.0))
                    .disabled(size >= max)
                    .tooltip(language.pick("增大字号", "Increase font size"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let size =
                            if ui { this.font_size_px(cx) } else { this.terminal_font_size_px(cx) };
                        let next = (size.floor() + 1.0).clamp(min, max);
                        this.persist(&[(key, format!("{next:.2}"))], cx);
                    })),
            );
        self.row(
            if ui {
                language.text(crate::i18n::Message::SettingsFontUiSize)
            } else {
                language.pick("终端字号（Ctrl+滚轮缩放）", "Terminal font size (Ctrl+wheel)")
            },
            if ui {
                language.text(crate::i18n::Message::SettingsFontUiSizeDescription)
            } else {
                language.pick("只调整终端文字大小。", "Changes only the terminal text size.")
            },
            stepper,
            cx,
        )
        .into_any_element()
    }

    pub(super) fn section_appearance(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = self.appearance_trigger(true, window, cx);
        let icon = self.appearance_trigger(false, window, cx);
        // 与开关、字体等设置共用行原语，字号、字重和说明缩放只维护一份。
        let selectors = v_flex()
            .w_full()
            .child(self.row(
                language.pick("主题", "Theme"),
                language.pick(
                    "终端与界面的配色，统一选择。",
                    "One palette for the terminal and interface.",
                ),
                theme,
                cx,
            ))
            .child(self.switch_row(
                "follow_system_theme",
                language.pick("跟随系统", "Follow system"),
                language.pick(
                    "随系统自动切换深浅色。",
                    "Switches between light and dark with the system.",
                ),
                self.runtime.follow_system_theme,
                cx,
            ))
            .child(self.row(
                language.pick("应用图标", "App icon"),
                language.pick(
                    "独立于主题，切换配色时保持不变。",
                    "Independent of the theme; changing colors keeps the icon.",
                ),
                icon,
                cx,
            ));
        let settings = self.appearance_advanced_settings(window, cx);
        v_flex().w_full().gap(px(GROUP_GAP)).child(selectors).child(settings)
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;

    #[gpui::test]
    fn font_size_click_input_commits_cancels_and_bounds_values(cx: &mut gpui::TestAppContext) {
        use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        persist_keys(&[("font_size", "15".into()), ("ui_font_size", "14".into())]).unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
        });
        let mut pane = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| SettingsPane::new(window, cx));
            pane = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let pane = pane.unwrap();
        cx.simulate_resize(gpui::size(px(1280.0), px(1800.0)));
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        for (key, text, action, expected) in [
            ("font_size", "18.5", "enter", 18.5),
            ("font_size", "27", "escape", 18.5),
            ("font_size", "invalid", "enter", 18.5),
            ("font_size", "NaN", "enter", 18.5),
            ("font_size", "999", "enter", 96.0),
            ("font_size", "-5", "enter", 4.0),
            ("ui_font_size", "17", "tab", 17.0),
            ("ui_font_size", "18", "shift-tab", 18.0),
            ("ui_font_size", "16", "blur", 16.0),
        ] {
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
            let selector = if key == "font_size" { "font_size-edit" } else { "ui_font_size-edit" };
            let bounds = cx.debug_bounds(selector).unwrap();
            // Increasing the interface size can move this row below the fold.
            // Reveal it with the same wheel path a user takes before clicking.
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: gpui::point(bounds.center().x, px(900.0)),
                delta: gpui::ScrollDelta::Pixels(gpui::point(
                    px(0.0),
                    px(900.0) - bounds.center().y,
                )),
                touch_phase: gpui::TouchPhase::Moved,
                modifiers: gpui::Modifiers::default(),
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!(
                bounds.top() >= px(0.0) && bounds.bottom() <= px(1800.0),
                "{key}: the clicked size must be inside the viewport: {bounds:?}"
            );
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            cx.run_until_parked();
            assert!(
                pane.read_with(cx, |pane, _| pane.font_size_editing.is_some()),
                "{key}: {text} via {action} must enter editing at {bounds:?}"
            );
            cx.update(|window, cx| {
                // Mount the editor before dispatching text: the native test host
                // does not draw a notified frame automatically.
                let _ = window.draw(cx);
                assert!(
                    pane.read(cx).font_size_input.read(cx).focus_handle(cx).is_focused(window),
                    "{key}: input must receive focus after clicking its value"
                );
            });
            cx.simulate_input(text);
            assert_eq!(
                pane.read_with(cx, |pane, cx| pane.font_size_input.read(cx).value().to_string()),
                text,
                "{key}: typing must replace the selected current value"
            );
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
            if action == "blur" {
                cx.update(|window, cx| {
                    pane.read(cx).settings_search_input.read(cx).focus_handle(cx).focus(window, cx);
                });
            } else {
                cx.simulate_keystrokes(action);
            }
            cx.run_until_parked();
            // Focus/blur notifications are dispatched when the new frame is drawn.
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
            cx.run_until_parked();
            pane.read_with(cx, |pane, cx| {
                assert!(
                    pane.font_size_editing.is_none(),
                    "{key}: {text} via {action} must finish editing"
                );
                let actual = if key == "font_size" {
                    pane.terminal_font_size_px(cx)
                } else {
                    pane.font_size_px(cx)
                };
                assert_eq!(actual, expected, "{key}: {text} via {action}");
            });
            let saved = RuntimeSettings::load();
            if key == "font_size" {
                assert_eq!(saved.font_size_px, Some(expected));
                assert_eq!(saved.ui_font_size_px, Some(14.0));
            } else {
                assert_eq!(saved.font_size_px, Some(4.0));
                assert_eq!(saved.ui_font_size_px, Some(expected));
            }
        }
    }
}
