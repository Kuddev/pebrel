use super::*;

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;

impl SettingsPane {
    pub(super) fn set_cursor_motion(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.try_persist(&[("cursor_motion", value.to_owned())], cx) {
            self.sync_select(
                "cursor_motion",
                self.runtime.cursor_motion.settings_value(),
                window,
                cx,
            );
            let language = crate::gpui_shell::config::ui_language(cx);
            super::super::toast::toast(
                window,
                cx,
                super::super::toast::ToastKind::Warning,
                language.format(
                    crate::i18n::Message::SettingsCursorMotionSaveFailed,
                    &[("error", &error.to_string())],
                ),
            );
            cx.notify();
        }
    }
}
