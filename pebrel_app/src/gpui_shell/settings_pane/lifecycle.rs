//! Session lifecycle controls and process-specific availability.

use super::*;

impl SettingsPane {
    pub(super) fn section_advanced(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        // 平台没实现的开关整行不画：开着却没效果比没有更糟（见
        // `platform::capabilities` 的说明）。
        let caps = crate::platform::CAPABILITIES;
        let lifecycle =
            self.group(language.pick("会话生命周期", "Session lifecycle"), cx)
                .when(caps.launch_at_login, |group| {
                    group.child(self.switch_row(
                        "launch_at_login",
                        language.text(crate::i18n::Message::SettingsStartupLaunchAtLogin),
                        language.text(crate::i18n::Message::SettingsStartupLaunchAtLoginHelp),
                        self.launch_at_login,
                        cx,
                    ))
                })
                .when(caps.hide_window_on_close, |group| {
                    group.child(self.switch_row(
                        "silent_start",
                        language.text(crate::i18n::Message::SettingsStartupSilentStart),
                        language.text(crate::i18n::Message::SettingsStartupSilentStartHelp),
                        self.runtime.silent_start,
                        cx,
                    ))
                })
                .when(caps.hide_window_on_close, |group| {
                    group.child(self.keep_session_row(
                        crate::platform::startup::instance::can_keep_session(),
                        cx,
                    ))
                })
                .child(self.switch_row(
                    "restore_session",
                    language.pick("启动时恢复上次标签", "Restore previous tabs at startup"),
                    help("restore_session", language),
                    self.runtime.restore_session,
                    cx,
                ))
                .child(self.switch_row(
                    "resume_ai",
                    language.pick("恢复时接续 AI 对话", "Resume AI conversations"),
                    help("resume_ai", language),
                    self.runtime.resume_ai,
                    cx,
                ))
                .when(caps.system_tray, |group| {
                    group.child(self.switch_row(
                        "tray",
                        language.pick("常驻系统托盘图标", "Keep an icon in the system tray"),
                        help("tray", language),
                        self.runtime.tray,
                        cx,
                    ))
                });
        v_flex().w_full().child(lifecycle)
    }

    fn keep_session_row(&self, available: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let label =
            language.pick("关窗后保留后台会话", "Keep sessions running after the window closes");
        if available {
            return self
                .switch_row(
                    "keep_session",
                    label,
                    help("keep_session", language),
                    self.runtime.keep_session,
                    cx,
                )
                .into_any_element();
        }
        // Show the effective state without changing the preference used by ordinary instances.
        self.row(
            label,
            language.text(crate::i18n::Message::SettingsStartupKeepSessionUnavailable),
            crate::gpui_shell::widgets::NebulaSwitch::new("keep_session")
                .checked(false)
                .disabled(true),
            cx,
        )
        .into_any_element()
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;
