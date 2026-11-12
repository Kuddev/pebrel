use crate::display::UiLanguage;

#[derive(Clone, Copy)]
pub(super) struct SettingHelp {
    pub summary: &'static str,
    pub details: Option<&'static str>,
}

impl From<&'static str> for SettingHelp {
    fn from(summary: &'static str) -> Self {
        Self { summary, details: None }
    }
}

pub(super) fn help(key: &str, language: UiLanguage) -> SettingHelp {
    let (summary, details) = match key {
        "background" => (
            language.text(crate::i18n::Message::SettingsHelpChangesOnlyTheTerminalBackground),
            None,
        ),
        "background_image" => (
            language.text(crate::i18n::Message::SettingsHelpShowsAnImageBehindTerminal),
            Some(language.text(crate::i18n::Message::SettingsHelpTextColorsStillComeFrom)),
        ),
        "background_image_fit" => (
            language.text(crate::i18n::Message::SettingsHelpChoosesHowTheBackgroundImage),
            Some(language.text(crate::i18n::Message::SettingsHelpFillStretchesTheImageUniform)),
        ),
        "background_image_alignment" => {
            (language.text(crate::i18n::Message::SettingsHelpPositionsTheImageWithinThe), None)
        },
        "background_image_opacity" => {
            (language.text(crate::i18n::Message::SettingsHelpLowerValuesMakeTheImage), None)
        },
        "background_image_cover_chrome" => {
            (language.text(crate::i18n::Message::SettingsHelpWhenDisabledTheImageAppears), None)
        },
        "cursor_shape" => ("", None),
        "cursor_blink" => {
            (language.text(crate::i18n::Message::SettingsHelpWhenDisabledTheCursorStays), None)
        },
        "tab_close_visible" => {
            (language.text(crate::i18n::Message::SettingsHelpMiddleClickStillClosesTabs), None)
        },
        "language" => (
            language.text(crate::i18n::Message::SettingsHelpChangesOnlyTheApplicationInterface),
            Some(language.text(crate::i18n::Message::SettingsHelpProgramsInsideTheTerminalChoose)),
        ),
        "density" => (
            language.text(crate::i18n::Message::SettingsHelpAdjustsInterfaceSpacingWithoutChanging),
            None,
        ),
        "opacity" => (language.text(crate::i18n::Message::SettingsOpacityDescription), None),
        "blur" => (
            language.text(crate::i18n::Message::SettingsHelpChoosesTheWindowBackgroundEffect),
            Some(language.text(crate::i18n::Message::SettingsHelpMicaEffectsUseFewerResources)),
        ),
        "cell_width_mode" => (
            language.text(crate::i18n::Message::SettingsHelpAdjustsTerminalCharacterCellWidth),
            Some(language.text(crate::i18n::Message::SettingsHelpCompactRoundsDownRelaxedRounds)),
        ),
        "fetch" => (
            language.text(crate::i18n::Message::SettingsHelpShowsSystemInformationWhenA),
            Some(language.text(crate::i18n::Message::SettingsHelpUsesFastfetchToDisplaySystem)),
        ),
        "powerline" => (
            language.text(crate::i18n::Message::SettingsHelpShowsASegmentedPromptRequires),
            Some(language.text(crate::i18n::Message::SettingsHelpTheFontMustIncludePowerline)),
        ),
        "shell" => (language.text(crate::i18n::Message::SettingsHelpUsedForNewTabsExisting), None),
        "startup_directory" => (
            language.text(crate::i18n::Message::SettingsHelpWhenUnsetInheritsTheDirectory),
            Some(language.text(crate::i18n::Message::SettingsHelpLaunchingFromAFileExplorer)),
        ),
        "bell" => {
            (language.text(crate::i18n::Message::SettingsHelpChoosesSoundOrVisualFeedback), None)
        },
        "ai_toasts" => (
            language.text(crate::i18n::Message::SettingsNotificationsAiMessagesDescription),
            Some(language.text(crate::i18n::Message::SettingsNotificationsAiMessagesDetails)),
        ),
        "notification_duration" => (
            language.text(crate::i18n::Message::SettingsNotificationsDurationDescription),
            Some(language.text(crate::i18n::Message::SettingsNotificationsDurationDetails)),
        ),
        "font_family" => (
            language.text(crate::i18n::Message::SettingsHelpUsesTheFontGroupIn),
            Some(language.text(crate::i18n::Message::SettingsHelpWhenAFontLacksA)),
        ),
        "ghost" => {
            (language.text(crate::i18n::Message::SettingsHelpSuggestsCommandsFromHistoryAnd), None)
        },
        "accept" => (
            language.text(crate::i18n::Message::SettingsHelpChoosesTheKeyUsedTo),
            Some(language.text(crate::i18n::Message::SettingsHelpUseRightArrowIfTab)),
        ),
        "completion_style" => (
            language.text(crate::i18n::Message::SettingsCompletionDescription),
            Some(language.text(crate::i18n::Message::SettingsCompletionDetails)),
        ),
        "copy_on_select" => (
            language.text(crate::i18n::Message::SettingsHelpCopiesOnMouseReleaseAnd),
            Some(language.text(crate::i18n::Message::SettingsHelpWhenDisabledSelectionDoesNot)),
        ),
        "multiline_paste_confirm" => (
            language.text(crate::i18n::Message::SettingsHelpAsksBeforeMultilineOrRisky),
            Some(
                language.text(crate::i18n::Message::SettingsHelpChecksLineBreaksPrivilegedCommands),
            ),
        ),
        "panel_resize" => {
            (language.text(crate::i18n::Message::SettingsHelpLocksTheDividerWhenDisabled), None)
        },
        "cjk_bold_regular" => (
            language.text(crate::i18n::Message::SettingsHelpBrightensDenseGlyphsInsteadOf),
            Some(language.text(crate::i18n::Message::SettingsHelpOnlyCjkBoldGlyphsAre)),
        ),
        "tabs_position" => ("", None),
        "tab_reveal" => {
            (language.text(crate::i18n::Message::SettingsHelpSlidesNewTabsIntoView), None)
        },
        "new_tab_position" => {
            (language.text(crate::i18n::Message::SettingsHelpAffectsNewTabsOnlyNot), None)
        },
        "windowing_behavior" => {
            (language.text(crate::i18n::Message::SettingsHelpOpensANewWindowOr), None)
        },
        "vcs_display" => (
            language.text(crate::i18n::Message::SettingsHelpChoosesWhetherTheSidebarShows),
            Some(language.text(crate::i18n::Message::SettingsHelpAutoDetectChecksForGit)),
        ),
        "keep_session" => (
            language.text(crate::i18n::Message::SettingsHelpKeepsTerminalsRunningAfterClosing),
            Some(language.text(crate::i18n::Message::SettingsHelpWhenDisabledClosingTheWindow)),
        ),
        "restore_session" => (
            language.text(crate::i18n::Message::SettingsHelpRestoresTabsLayoutAndDirectories),
            Some(language.text(crate::i18n::Message::SettingsHelpRestoredTabsStartNewShells)),
        ),
        "resume_ai" => (
            language.text(crate::i18n::Message::SettingsHelpContinuesThePreviousConversationWhen),
            None,
        ),
        "tray" => (language.text(crate::i18n::Message::SettingsHelpShowsAiSessionStatusIn), None),
        _ => unreachable!("unknown settings help key: {key}"),
    };
    SettingHelp { summary, details }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_stay_short_in_both_interface_languages() {
        let keys = [
            "background",
            "background_image",
            "background_image_fit",
            "background_image_alignment",
            "background_image_opacity",
            "background_image_cover_chrome",
            "cursor_shape",
            "cursor_blink",
            "tab_close_visible",
            "language",
            "density",
            "opacity",
            "blur",
            "cell_width_mode",
            "fetch",
            "powerline",
            "shell",
            "startup_directory",
            "bell",
            "ai_toasts",
            "notification_duration",
            "font_family",
            "ghost",
            "accept",
            "completion_style",
            "copy_on_select",
            "multiline_paste_confirm",
            "panel_resize",
            "cjk_bold_regular",
            "tabs_position",
            "tab_reveal",
            "new_tab_position",
            "windowing_behavior",
            "vcs_display",
            "keep_session",
            "restore_session",
            "resume_ai",
            "tray",
        ];
        for key in keys {
            for language in [UiLanguage::ZhCn, UiLanguage::EnUs] {
                let description = help(key, language);
                let limit = if language == UiLanguage::ZhCn { 32 } else { 100 };
                assert!(description.summary.chars().count() <= limit, "long summary: {key}");
                assert!(description.details.is_none_or(|details| !details.is_empty()));
            }
        }
    }

    #[test]
    fn critical_limits_remain_in_the_short_description() {
        let language = UiLanguage::ZhCn;
        assert!(help("keep_session", language).summary.contains("结束会话"));
        assert!(help("restore_session", language).summary.contains("不恢复原进程"));
        assert!(help("multiline_paste_confirm", language).details.unwrap().contains("全屏程序"));
    }
}
