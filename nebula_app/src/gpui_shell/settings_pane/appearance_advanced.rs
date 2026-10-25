use super::*;

impl SettingsPane {
    pub(super) fn appearance_advanced_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let font_picker = self.font_picker_dropdown(window, cx);
        let cjk_font_picker = self.cjk_font_picker_dropdown(window, cx);
        let tab_font_picker = self.tab_font_picker_dropdown(window, cx);
        let opacity: SharedString = format!("{:.0}%", self.runtime.opacity * 100.0).into();
        let wallpaper_opacity: SharedString =
            format!("{:.0}%", self.runtime.background_image_opacity * 100.0).into();
        let custom_background = self
            .group(language.text(crate::i18n::Message::SettingsAppearanceCustomBackground), cx)
            .child(self.background_color_row(cx))
            .child(self.select_row(
                "background_media_kind",
                language.text(crate::i18n::Message::WallpaperKind),
                language.text(crate::i18n::Message::WallpaperKindDescription),
                cx,
            ))
            .child(self.background_image_row(cx))
            .child(self.select_row(
                "background_shader_preset",
                language.text(crate::i18n::Message::WallpaperShader),
                language.text(crate::i18n::Message::WallpaperShaderDescription),
                cx,
            ))
            .when(self.shader_custom_open || self.runtime.background_effects.wgsl, |group| {
                group.child(self.shader_source_row(cx))
            })
            .child(self.select_row(
                "background_image_fit",
                language.text(crate::i18n::Message::SettingsAppearanceBackgroundImageFit),
                help("background_image_fit", language),
                cx,
            ))
            .child(self.select_row(
                "background_image_alignment",
                language.text(crate::i18n::Message::SettingsAppearanceBackgroundImageAlignment),
                help("background_image_alignment", language),
                cx,
            ))
            .child(self.slider_row(
                language.text(crate::i18n::Message::SettingsAppearanceBackgroundImageOpacity),
                help("background_image_opacity", language),
                &self.wallpaper_opacity_slider,
                wallpaper_opacity,
                cx,
            ))
            .child(self.switch_row(
                "background_image_cover_chrome",
                language.text(crate::i18n::Message::SettingsAppearanceExtendTheBackgroundImageInto),
                help("background_image_cover_chrome", language),
                self.runtime.background_image_cover_chrome,
                cx,
            ))
            .child(self.custom_effect_settings(window, cx));
        let cursor = self
            .group(language.text(crate::i18n::Message::CommonCursor), cx)
            .child(self.select_row(
                "cursor_motion",
                language.text(crate::i18n::Message::SettingsCursorMotionTitle),
                language.text(crate::i18n::Message::SettingsCursorMotionDescription),
                cx,
            ))
            .child(self.select_row(
                "cursor_shape",
                language.text(crate::i18n::Message::SettingsAppearanceCursorShape),
                help("cursor_shape", language),
                cx,
            ))
            .child(self.switch_row(
                "cursor_blink",
                language.text(crate::i18n::Message::SettingsAppearanceBlinkCursor),
                help("cursor_blink", language),
                self.runtime.cursor_blink.unwrap_or(DEFAULT_CURSOR_BLINK),
                cx,
            ));
        let interface = self
            .group(language.text(crate::i18n::Message::ThemeEditorPreviewInterface), cx)
            .child(self.font_size_row(true, cx))
            .child(self.switch_row(
                "dim_inactive_panes",
                language.text(crate::i18n::Message::SettingsPanesDimInactive),
                language.text(crate::i18n::Message::SettingsPanesDimInactiveDescription),
                self.runtime.dim_inactive_panes,
                cx,
            ))
            .child(self.switch_row(
                "tab_close_visible",
                language.text(crate::i18n::Message::SettingsAppearanceShowTabCloseButtons),
                help("tab_close_visible", language),
                self.runtime.tab_close_visible,
                cx,
            ))
            .child(self.select_row(
                "language",
                language.text(crate::i18n::Message::CommonLanguage),
                help("language", language),
                cx,
            ))
            .child(self.select_row(
                "density",
                language.text(crate::i18n::Message::SettingsAppearanceInterfaceDensity),
                help("density", language),
                cx,
            ))
            .child(self.slider_row(
                language.text(crate::i18n::Message::SettingsOpacityTitle),
                help("opacity", language),
                &self.opacity_slider,
                opacity,
                cx,
            ))
            .child(self.select_row(
                "blur",
                language.text(crate::i18n::Message::SettingsAppearanceBackgroundBlur),
                help("blur", language),
                cx,
            ));
        let terminal = self
            .group(language.text(crate::i18n::Message::SettingsAppearanceTerminalAppearance), cx)
            .child(self.row(
                language.text(crate::i18n::Message::SettingsFontEnglish),
                help("font_family", language),
                font_picker,
                cx,
            ))
            .child(self.row(
                language.text(crate::i18n::Message::SettingsFontChinese),
                language.text(crate::i18n::Message::SettingsFontChineseDescription),
                cjk_font_picker,
                cx,
            ))
            .child(self.row(
                language.text(crate::i18n::Message::SettingsFontTabFamily),
                language.text(crate::i18n::Message::SettingsFontTabFamilyDescription),
                tab_font_picker,
                cx,
            ))
            .child(self.font_size_row(false, cx))
            .child(self.switch_row(
                "ctrl_wheel_font_zoom",
                language.text(crate::i18n::Message::SettingsFontCtrlWheelZoom),
                language.text(crate::i18n::Message::SettingsFontCtrlWheelZoomDescription),
                self.runtime.ctrl_wheel_font_zoom,
                cx,
            ))
            .child(self.select_row(
                "ligatures",
                language.text(crate::i18n::Message::SettingsFontLigatures),
                language.text(crate::i18n::Message::SettingsFontLigaturesDescription),
                cx,
            ))
            .child(self.select_row(
                "cell_width_mode",
                language.text(crate::i18n::Message::SettingsAppearanceCharacterSpacing),
                help("cell_width_mode", language),
                cx,
            ))
            .child(self.select_row(
                "scrollback_lines",
                language.text(crate::i18n::Message::SettingsScrollingHistory),
                language.text(crate::i18n::Message::SettingsScrollingHistoryDescription),
                cx,
            ))
            .child(self.scroll_speed_row(cx))
            .child(self.select_row(
                "scrollbar_visibility",
                language.text(crate::i18n::Message::SettingsScrollingScrollbar),
                language.text(crate::i18n::Message::SettingsScrollingScrollbarDescription),
                cx,
            ))
            .child(self.switch_row(
                "fetch",
                language.text(crate::i18n::Message::SettingsAppearanceStartupSystemInformation),
                help("fetch", language),
                self.runtime.fetch,
                cx,
            ))
            .child(self.switch_row(
                "powerline",
                language.text(crate::i18n::Message::SettingsAppearancePowerlinePrompt),
                help("powerline", language),
                self.runtime.powerline,
                cx,
            ));

        v_flex()
            .w_full()
            .gap(px(GROUP_GAP))
            .child(terminal)
            .child(cursor)
            .child(interface)
            .child(custom_background)
    }

    pub(super) fn custom_effect_settings(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        use crate::i18n::Message;
        let language = crate::gpui_shell::config::ui_language(cx);
        let expanded = self.effect_settings_open;
        // 展开状态只属于设置页；收起高级入口不能停用或改写用户的效果配置。
        v_flex()
            .w_full()
            .child(
                Button::new("custom-effects-disclosure")
                    .debug_selector(|| "custom-effects-disclosure".to_owned())
                    .ghost()
                    .icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight })
                    .label(language.text(if self.runtime.terminal_effects.enabled {
                        Message::TerminalEffectAdvancedActive
                    } else {
                        Message::TerminalEffectAdvanced
                    }))
                    .toggled(expanded)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.effect_settings_open = !this.effect_settings_open;
                        cx.notify();
                    })),
            )
            .when(expanded, |group| {
                group.child(self.terminal_effect_row(window, cx)).child(self.select_row(
                    "terminal_effect_animation",
                    language.text(Message::TerminalEffectAnimation),
                    language.text(Message::TerminalEffectAnimationDescription),
                    cx,
                ))
            })
    }
}
