use gpui::SharedString;

/// 固定下拉只在这里把稳定 value 映射为显示文案。创建和语言切换刷新共用
/// 同一入口，避免 `SelectState` 留着构造时的旧语言。
pub(super) fn localized_select_labels(
    key: &str,
    values: &[&'static str],
    language: crate::display::UiLanguage,
) -> Vec<SharedString> {
    if key == "scrollback_lines" {
        return values
            .iter()
            .map(|value| {
                let number = value.parse::<usize>().expect("validated scrollback choice");
                format!("{},000", number / 1_000).into()
            })
            .collect();
    }
    let labels: Vec<&'static str> = match key {
        "terminal_effect_animation" => vec![
            language.text(crate::i18n::Message::TerminalEffectAnimationOff),
            language.text(crate::i18n::Message::TerminalEffectAnimationFocused),
            language.text(crate::i18n::Message::TerminalEffectAnimationAlways),
        ],
        "background_shader_preset" => vec![
            language.text(crate::i18n::Message::WallpaperShaderOff),
            language.text(crate::i18n::Message::WallpaperShaderGrain),
            language.text(crate::i18n::Message::WallpaperShaderNeonVortex),
            language.text(crate::i18n::Message::WallpaperShaderAuroraRibbons),
            language.text(crate::i18n::Message::WallpaperShaderLiquidSilk),
            language.text(crate::i18n::Message::WallpaperShaderWgsl),
        ],
        "background_media_kind" => vec![
            language.text(crate::i18n::Message::WallpaperImage),
            language.text(crate::i18n::Message::WallpaperVideo),
            language.text(crate::i18n::Message::WallpaperGif),
        ],
        "language" => nebula_settings::LanguagePref::ALL
            .iter()
            .map(|preference| {
                if *preference == nebula_settings::LanguagePref::System {
                    language.tr("language.system")
                } else {
                    preference.native_name()
                }
            })
            .collect(),
        "quick_terminal_mode" => vec![
            language.text(crate::i18n::Message::SettingsQuickTerminalDedicated),
            language.text(crate::i18n::Message::SettingsQuickTerminalExisting),
        ],
        "scrollbar_visibility" => vec![
            language.text(crate::i18n::Message::SettingsScrollingScrollbarAuto),
            language.text(crate::i18n::Message::SettingsScrollingScrollbarHover),
            language.text(crate::i18n::Message::SettingsScrollingScrollbarAlways),
        ],
        "notification_duration" => vec![
            language.text(crate::i18n::Message::SettingsNotificationsDurationDefault),
            language.text(crate::i18n::Message::SettingsNotificationsDurationFive),
            language.text(crate::i18n::Message::SettingsNotificationsDurationTen),
            language.text(crate::i18n::Message::SettingsNotificationsDurationThirty),
            language.text(crate::i18n::Message::SettingsNotificationsDurationNinety),
            language.text(crate::i18n::Message::SettingsNotificationsDurationPersistent),
        ],
        "cursor_shape" => vec![
            language.text(crate::i18n::Message::SettingsOptionBar),
            language.text(crate::i18n::Message::SettingsOptionUnderscore),
            language.text(crate::i18n::Message::SettingsOptionFilledBox),
            language.text(crate::i18n::Message::SettingsOptionEmptyBox),
        ],
        "cursor_motion" => vec![
            language.text(crate::i18n::Message::SettingsCursorMotionOff),
            language.text(crate::i18n::Message::SettingsCursorMotionSmooth),
        ],
        "tabs_position" => {
            vec![
                language.text(crate::i18n::Message::CommonLeftSidebar),
                language.text(crate::i18n::Message::CommonTop),
            ]
        },
        "tab_reveal" => vec![
            language.text(crate::i18n::Message::SettingsOptionSlide),
            language.text(crate::i18n::Message::SettingsOptionInstant),
        ],
        "density" => vec![
            language.text(crate::i18n::Message::CommonStandard),
            language.text(crate::i18n::Message::CommonCompact),
        ],
        "new_tab_position" => vec![
            language.text(crate::i18n::Message::SettingsOptionAfterCurrentTab),
            language.text(crate::i18n::Message::SettingsOptionEndOfList),
        ],
        "windowing_behavior" => vec![
            language.text(crate::i18n::Message::SettingsOptionCreateANewWindow),
            language.text(crate::i18n::Message::SettingsOptionAttachToTheMostRecent),
            language.text(crate::i18n::Message::SettingsOptionAttachToTheMostRecentV2),
        ],
        "vcs_display" => vec![
            language.text(crate::i18n::Message::SettingsOptionAutoDetect),
            language.text(crate::i18n::Message::SettingsOptionGitOnly),
            language.text(crate::i18n::Message::SettingsOptionSvnOnly),
        ],
        "ligatures" => vec![
            language.text(crate::i18n::Message::SettingsFontLigaturesOn),
            language.text(crate::i18n::Message::SettingsFontLigaturesOff),
            language.text(crate::i18n::Message::SettingsFontLigaturesTheme),
        ],
        "cell_width_mode" => {
            vec![
                language.text(crate::i18n::Message::CommonCompact),
                language.text(crate::i18n::Message::CommonRelaxed),
            ]
        },
        "bell" => vec![
            language.text(crate::i18n::Message::CommonOff),
            language.text(crate::i18n::Message::CommonVisual),
            language.text(crate::i18n::Message::CommonSound),
            language.text(crate::i18n::Message::SettingsOptionVisualSound),
        ],
        "blur" => vec![
            language.text(crate::i18n::Message::CommonNone),
            language.text(crate::i18n::Message::SettingsOptionMicaLowCost),
            language.text(crate::i18n::Message::SettingsOptionMicaAltLowCost),
            language.text(crate::i18n::Message::SettingsOptionAeroGlass),
            language.text(crate::i18n::Message::SettingsOptionAcrylicHighCost),
        ],
        "accept" => vec![
            language.text(crate::i18n::Message::SettingsOptionRightArrow),
            "Tab",
            language.text(crate::i18n::Message::SettingsOptionTabOrRightArrow),
        ],
        "completion_style" => nebula_settings::CompletionStyleName::ALL
            .into_iter()
            .map(|style| language.completion_style_label(style))
            .collect(),
        "background_image_fit" => vec![
            language.text(crate::i18n::Message::SettingsOptionFill),
            language.text(crate::i18n::Message::SettingsOptionUniform),
            language.text(crate::i18n::Message::SettingsOptionUniformToFill),
            language.text(crate::i18n::Message::SettingsOptionOriginalSize),
        ],
        "background_image_alignment" => vec![
            language.text(crate::i18n::Message::SettingsOptionTopLeft),
            language.text(crate::i18n::Message::CommonTop),
            language.text(crate::i18n::Message::SettingsOptionTopRight),
            language.text(crate::i18n::Message::CommonLeft),
            language.text(crate::i18n::Message::CommonCenter),
            language.text(crate::i18n::Message::CommonRight),
            language.text(crate::i18n::Message::SettingsOptionBottomLeft),
            language.text(crate::i18n::Message::CommonBottom),
            language.text(crate::i18n::Message::SettingsOptionBottomRight),
        ],
        "ssh_proxy_mode" => vec![
            language.tr("settings.network.mode.off"),
            language.tr("settings.network.mode.system"),
            language.tr("settings.network.mode.custom"),
        ],
        _ => values.to_vec(),
    };
    debug_assert_eq!(labels.len(), values.len(), "localized select label/value mismatch: {key}");
    labels.into_iter().map(SharedString::from).collect()
}

pub(super) fn provider_input_placeholders(
    language: crate::display::UiLanguage,
) -> [&'static str; 5] {
    [
        language.text(crate::i18n::Message::SettingsOptionProviderName),
        language.text(crate::i18n::Message::SettingsOptionNoteSpacesAllowed),
        language.text(crate::i18n::Message::SettingsOptionOfficialWebsite),
        language.text(crate::i18n::Message::SettingsOptionApiEndpoint),
        language.text(crate::i18n::Message::SettingsOptionDefaultModel),
    ]
}

pub(super) fn localized_input_placeholder(
    key: &str,
    language: crate::display::UiLanguage,
) -> &'static str {
    match key {
        "ssh_label" => language.text(crate::i18n::Message::SettingsOptionEGDevelopmentServer),
        "ssh_password" => language.text(crate::i18n::Message::SettingsOptionLeaveEmptyToAskWhen),
        "ssh_proxy_username" => language.text(crate::i18n::Message::SettingsOptionOptional),
        "ssh_proxy_password" => {
            language.text(crate::i18n::Message::SettingsOptionLeaveEmptyToKeepThe)
        },
        "ssh_jump_host" => language.text(crate::i18n::Message::SettingsOptionUserBastionN22OrSsh),
        "ssh_icon_filter" => language.text(crate::i18n::Message::SettingsOptionSearchIcons),
        "font_family" => language.text(crate::i18n::Message::SettingsFontEnterAFontFamily),
        "backup_password" => {
            language.text(crate::i18n::Message::SettingsOptionBackupPasswordAtLeastN8)
        },
        "backup_secret" => language.tr("settings.input.backup_secret"),
        _ => "",
    }
}
