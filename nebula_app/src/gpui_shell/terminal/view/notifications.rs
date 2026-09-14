use crate::ai_agents::{AgentKind, AgentStatus};
use gpui::{Context, EventEmitter as _};

impl super::TerminalView {
    pub(crate) fn is_reading_answer(&self) -> bool {
        self.answer_reader.is_some()
    }

    pub(super) fn notify_command_done(&self, cx: &mut Context<Self>) {
        if let Some(started) = self.command_started
            && started.elapsed() >= crate::notify::COMMAND_NOTIFY_MIN
        {
            cx.emit(super::TerminalViewEvent::Notification(
                crate::notify::Notification::CommandDone {
                    duration: started.elapsed(),
                    program: self.running_program.clone(),
                },
            ));
        }
    }
}

/// 屏幕推导出的状态变化要不要弹提示。
///
/// `hooks` 表示 agent CLI 的 hook 是否在场。在场时 `TurnDone` 才是权威终态，
/// 屏幕静默只是「迟迟没等到 hook」的兜底猜测——它可以照旧更新状态（侧栏蓝点
/// 不受影响，状态赋值在调用方），但不该对用户宣称「回合完成」。
///
/// 2026-09-14：此前无条件对 `Working -> Done` 弹提示，而这条转换只要屏幕静默
/// 5 拍（看门狗 1 Hz）就会发生。Claude 思考时、或跑一条不输出的长命令（编译等）
/// 时终端本就没有输出，于是任务还在跑却反复弹「回合完成」。没有 hook 的客户端
/// 屏幕仍是唯一证据，保持原样。
pub(super) fn screen_notification(previous: AgentStatus, next: AgentStatus, hooks: bool) -> Option<bool> {
    match next {
        AgentStatus::Blocked if previous != AgentStatus::Blocked => Some(true),
        AgentStatus::Done if matches!(previous, AgentStatus::Working | AgentStatus::Blocked) => {
            (!hooks).then_some(false)
        },
        _ => None,
    }
}

pub(super) fn screen_program(
    current: Option<&str>,
    identified: Option<AgentKind>,
) -> Option<String> {
    match current {
        Some(program) if AgentKind::parse(program).is_some() => Some(program.to_owned()),
        Some(program) if !screen_identity_allowed(Some(program)) => None,
        _ => identified.map(|agent| agent.slug().to_owned()),
    }
}

pub(super) fn screen_identity_allowed(current: Option<&str>) -> bool {
    current.is_none_or(|program| {
        crate::process_tree::is_interactive_shell_command(program)
            || crate::process_tree::display_name(program).eq_ignore_ascii_case("ssh")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_completion_and_attention_are_edges_not_idle_polling() {
        // 无 hook 时屏幕是唯一证据，「完成」照旧提示。
        assert_eq!(screen_notification(AgentStatus::Working, AgentStatus::Done, false), Some(false));
        assert_eq!(screen_notification(AgentStatus::Blocked, AgentStatus::Done, false), Some(false));
        assert_eq!(screen_notification(AgentStatus::Working, AgentStatus::Blocked, false), Some(true));
        assert_eq!(screen_notification(AgentStatus::Unknown, AgentStatus::Blocked, false), Some(true));
        assert_eq!(screen_notification(AgentStatus::Done, AgentStatus::Done, false), None);
        assert_eq!(screen_notification(AgentStatus::Blocked, AgentStatus::Blocked, false), None);
        assert_eq!(screen_notification(AgentStatus::Unknown, AgentStatus::Idle, false), None);
        assert_eq!(screen_notification(AgentStatus::Idle, AgentStatus::Idle, false), None);
    }

    /// hook 在场时 `TurnDone` 才是权威终态：屏幕静默推出的 `Done` 不弹提示，
    /// 否则 Claude 思考或长命令运行期间会反复误报「回合完成」。
    #[test]
    fn screen_inferred_completion_stays_silent_while_hooks_are_live() {
        assert_eq!(screen_notification(AgentStatus::Working, AgentStatus::Done, true), None);
        assert_eq!(screen_notification(AgentStatus::Blocked, AgentStatus::Done, true), None);
        // 等输入是真事件，与完成无关，不受 hook 影响。
        assert_eq!(screen_notification(AgentStatus::Working, AgentStatus::Blocked, true), Some(true));
    }

    #[test]
    fn wsl_and_ssh_wrappers_allow_identifying_the_guest_agent() {
        for wrapper in [None, Some("wsl"), Some("ssh"), Some("bash")] {
            assert_eq!(screen_program(wrapper, Some(AgentKind::Claude)), Some("claude".into()));
        }
        assert_eq!(screen_program(Some("codex"), Some(AgentKind::Claude)), Some("codex".into()));
        assert_eq!(screen_program(Some("vim"), Some(AgentKind::Claude)), None);
        assert_eq!(screen_program(Some("wsl"), None), None);
    }
}
