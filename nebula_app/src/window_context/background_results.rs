//! Deliver backup and sync worker results to the legacy window.

use super::WindowContext;
use crate::i18n::t;

impl WindowContext {
    /// 远程备份线程收尾：设置页状态行是第一现场，message bar 兜底通知
    /// 没开设置页的窗口。
    pub fn handle_backup_remote_done(&mut self, message: &str, error: bool) {
        self.display.backup_remote_done(message, error);
        let ty = if error {
            crate::message_bar::MessageType::Error
        } else {
            crate::message_bar::MessageType::Warning
        };
        self.message_buffer.push(crate::message_bar::Message::new(
            t!("backup.notice", message = message).to_string(),
            ty,
        ));
        self.dirty = true;
        self.display.window.request_redraw();
    }

    /// 同步线程收尾（spec 003）：消息进 message bar；拉到新历史时热加载
    /// （ghost 补全立即吃到另一台机器的命令）。settings 变化不用管——
    /// mtime 监视在下一帧自动 reload。
    pub fn handle_sync_done(&mut self, message: &str, error: bool, history_changed: bool) {
        // 设置页的状态行（按钮下方）是第一现场；message_bar 兜底通知
        // 没开设置页的窗口。
        self.display.sync_action_done(message, error);
        let ty = if error {
            crate::message_bar::MessageType::Error
        } else {
            crate::message_bar::MessageType::Warning
        };
        self.message_buffer.push(crate::message_bar::Message::new(
            t!("sync.notice", message = message).to_string(),
            ty,
        ));
        if history_changed {
            self.display.reload_nebula_history();
        }
        self.dirty = true;
        self.display.window.request_redraw();
    }
}
