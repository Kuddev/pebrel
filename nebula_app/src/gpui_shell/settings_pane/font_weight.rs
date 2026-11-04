use super::*;

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;

impl SettingsPane {
    pub(super) fn set_font_weight(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(weight) = nebula_settings::TerminalFontWeight::from_settings(value) else {
            return;
        };
        let result = self.try_persist(&[("font_weight", weight.settings_value().to_owned())], cx);
        // A failed selection or reset must keep showing the last saved weight.
        self.sync_select("font_weight", self.runtime.font_weight.settings_value(), window, cx);
        if let Err(error) = result {
            let language = crate::gpui_shell::config::ui_language(cx);
            super::super::toast::toast(
                window,
                cx,
                super::super::toast::ToastKind::Warning,
                language.format(
                    crate::i18n::Message::SettingsSaveFailed,
                    &[("error", &error.to_string())],
                ),
            );
            cx.notify();
        }
    }
}
