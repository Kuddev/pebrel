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
            language.tr("settings.help.background"),
            None,
        ),
        "background_image" => (
            language.tr("settings.help.background_image"),
            Some(language.tr("settings.help.background_image_detail")),
        ),
        "background_image_fit" => (
            language.tr("settings.help.background_image_fit"),
            Some(language.tr("settings.help.background_image_fit_detail")),
        ),
        "background_image_alignment" => (
            language.tr("settings.help.background_image_alignment"),
            None,
        ),
        "background_image_opacity" => (
            language.tr("settings.help.background_image_opacity"),
            None,
        ),
        "background_image_cover_chrome" => (
            language.tr("settings.help.background_image_cover_chrome"),
            None,
        ),
        "cursor_shape" => ("", None),
        "cursor_blink" => (
            language.tr("settings.help.cursor_blink"),
            None,
        ),
        "tab_close_visible" => (
            language.tr("settings.help.tab_close_visible"),
            None,
        ),
        "language" => (
            language.tr("settings.help.language"),
            Some(language.tr("settings.help.language_detail")),
        ),
        "density" => (
            language.tr("settings.help.density"),
            None,
        ),
        "opacity" => (
            language.tr("settings.help.opacity"),
            None,
        ),
        "blur" => (
            language.tr("settings.help.blur"),
            Some(language.tr("settings.help.blur_detail")),
        ),
        "cell_width_mode" => (
            language.tr("settings.help.cell_width_mode"),
            Some(language.tr("settings.help.cell_width_mode_detail")),
        ),
        "fetch" => (
            language.tr("settings.help.fetch"),
            Some(language.tr("settings.help.fetch_detail")),
        ),
        "powerline" => (
            language.tr("settings.help.powerline"),
            Some(language.tr("settings.help.powerline_detail")),
        ),
        "shell" => (
            language.tr("settings.help.shell"),
            None,
        ),
        "startup_directory" => (
            language.tr("settings.help.startup_directory"),
            Some(language.tr("settings.help.startup_directory_detail")),
        ),
        "bell" => (language.tr("settings.help.bell"), None),
        "ai_toasts" => (
            language.text(crate::i18n::Message::SettingsNotificationsAiMessagesDescription),
            Some(language.text(crate::i18n::Message::SettingsNotificationsAiMessagesDetails)),
        ),
        "font_family" => (
            language.tr("settings.help.font_family"),
            Some(language.tr("settings.help.font_family_detail")),
        ),
        "ghost" => (
            language.tr("settings.help.ghost"),
            None,
        ),
        "accept" => (
            language.tr("settings.help.accept"),
            Some(language.tr("settings.help.accept_detail")),
        ),
        "completion_style" => (
            language.tr("settings.help.completion_style"),
            Some(language.tr("settings.help.completion_style_detail")),
        ),
        "copy_on_select" => (
            language.tr("settings.help.copy_on_select"),
            Some(language.tr("settings.help.copy_on_select_detail")),
        ),
        "multiline_paste_confirm" => (
            language.tr("settings.help.multiline_paste_confirm"),
            Some(language.tr("settings.help.multiline_paste_confirm_detail")),
        ),
        "panel_resize" => (
            language.tr("settings.help.panel_resize"),
            None,
        ),
        "cjk_bold_regular" => (
            language.tr("settings.help.cjk_bold_regular"),
            Some(language.tr("settings.help.cjk_bold_regular_detail")),
        ),
        "tabs_position" => ("", None),
        "tab_reveal" => (
            language.tr("settings.help.tab_reveal"),
            None,
        ),
        "new_tab_position" => (
            language.tr("settings.help.new_tab_position"),
            None,
        ),
        "windowing_behavior" => (
            language.tr("settings.help.windowing_behavior"),
            None,
        ),
        "vcs_display" => (
            language.tr("settings.help.vcs_display"),
            Some(language.tr("settings.help.vcs_display_detail")),
        ),
        "keep_session" => (
            language.tr("settings.help.keep_session"),
            Some(language.tr("settings.help.keep_session_detail")),
        ),
        "restore_session" => (
            language.tr("settings.help.restore_session"),
            Some(language.tr("settings.help.restore_session_detail")),
        ),
        "resume_ai" => (
            language.tr("settings.help.resume_ai"),
            None,
        ),
        "tray" => (
            language.tr("settings.help.tray"),
            None,
        ),
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
