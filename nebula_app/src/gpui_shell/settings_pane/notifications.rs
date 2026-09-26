use super::*;

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;

impl SettingsPane {
    pub(super) fn set_notification_duration(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.try_persist(&[("notification_duration", value.to_owned())], cx) {
            self.sync_select(
                "notification_duration",
                self.runtime.notification_duration.settings_value(),
                window,
                cx,
            );
            let language = crate::gpui_shell::config::ui_language(cx);
            super::super::toast::toast(
                window,
                cx,
                super::super::toast::ToastKind::Warning,
                language.format(
                    crate::i18n::Message::SettingsNotificationsSaveFailed,
                    &[("error", &error.to_string())],
                ),
            );
            cx.notify();
        }
    }
}
