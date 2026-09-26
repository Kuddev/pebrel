//! 本机命令输出沿用会话恢复设置区域；清除成功必须等磁盘操作完成。
use super::*;
use crate::i18n::Message;

#[derive(Default)]
pub(super) enum ClearState {
    #[default]
    Idle,
    Busy,
    Done,
    Failed,
}

impl SettingsPane {
    pub(super) fn command_output_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let label = match self.command_output_clear {
            ClearState::Idle => Message::SettingsCommandOutputClear,
            ClearState::Busy => Message::SettingsCommandOutputClearing,
            ClearState::Done => Message::SettingsCommandOutputCleared,
            ClearState::Failed => Message::SettingsCommandOutputClearFailed,
        };
        let control = crate::gpui_shell::widgets::NebulaButton::new("clear-command-output")
            .label(language.text(label))
            .disabled(matches!(self.command_output_clear, ClearState::Busy))
            .on_click(cx.listener(|this, _, _, cx| this.clear_command_output(cx)));
        #[cfg(any(test, feature = "gpui-test-support"))]
        let control = div().debug_selector(|| "clear-command-output".to_owned()).child(control);
        div()
            .child(self.switch_row(
                "save_command_output",
                language.text(Message::SettingsCommandOutputSave),
                language.text(Message::SettingsCommandOutputHelp),
                self.runtime.save_command_output,
                cx,
            ))
            .child(self.row(
                language.text(Message::SettingsCommandOutputClear),
                language.text(Message::SettingsCommandOutputClearHelp),
                control,
                cx,
            ))
    }

    fn clear_command_output(&mut self, cx: &mut Context<Self>) {
        if matches!(self.command_output_clear, ClearState::Busy) {
            return;
        }
        self.command_output_clear = ClearState::Busy;
        let clear = crate::gpui_shell::workspace::windowing::clear_command_output(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = clear.await;
            let state = match result {
                Ok(true) => ClearState::Done,
                Ok(false) => ClearState::Failed,
                Err(error) => {
                    log::warn!("Could not clear command output: {error}");
                    ClearState::Failed
                },
            };
            let _ = this.update(cx, |this, cx| {
                this.command_output_clear = state;
                cx.notify();
            });
        })
        .detach();
    }
}
