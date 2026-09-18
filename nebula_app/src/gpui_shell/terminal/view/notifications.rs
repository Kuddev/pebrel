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
/// `hooks` 表示当前 agent 会话是否已经收到 hook。有 hook 时，完成通知应等待
/// `TurnDone`。看门狗连续识别到 Idle 界面也会把 Working / Blocked 降为 Done，
/// 但界面可能在后台任务仍运行时显示 Idle，不能据此宣称回合完成。
/// 这里仅控制通知；调用方仍保留屏幕状态自愈，无 hook 会话保留屏幕通知。
pub(super) fn screen_notification(
    previous: AgentStatus,
    next: AgentStatus,
    hooks: bool,
) -> Option<bool> {
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
        assert_eq!(
            screen_notification(AgentStatus::Working, AgentStatus::Done, false),
            Some(false)
        );
        assert_eq!(
            screen_notification(AgentStatus::Blocked, AgentStatus::Done, false),
            Some(false)
        );
        assert_eq!(
            screen_notification(AgentStatus::Working, AgentStatus::Blocked, false),
            Some(true)
        );
        assert_eq!(
            screen_notification(AgentStatus::Unknown, AgentStatus::Blocked, false),
            Some(true)
        );
        assert_eq!(screen_notification(AgentStatus::Done, AgentStatus::Done, false), None);
        assert_eq!(screen_notification(AgentStatus::Blocked, AgentStatus::Blocked, false), None);
        assert_eq!(screen_notification(AgentStatus::Unknown, AgentStatus::Idle, false), None);
        assert_eq!(screen_notification(AgentStatus::Idle, AgentStatus::Idle, false), None);
    }

    /// hook 在场时，屏幕推断的 Done 不能触发完成通知。
    #[test]
    fn screen_inferred_completion_stays_silent_while_hooks_are_live() {
        assert_eq!(screen_notification(AgentStatus::Working, AgentStatus::Done, true), None);
        assert_eq!(screen_notification(AgentStatus::Blocked, AgentStatus::Done, true), None);
        // 等输入是真事件，与完成无关，不受 hook 影响。
        assert_eq!(
            screen_notification(AgentStatus::Working, AgentStatus::Blocked, true),
            Some(true)
        );
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
