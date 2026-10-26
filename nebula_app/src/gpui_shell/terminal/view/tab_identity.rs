//! Cached host identity and live task details for terminal tab chrome.

use super::{TerminalView, last_path_component};

/// title 是否构成「任务标题」：排除空串、shell 名、进程名、品牌名、cwd、
/// SSH 目的地与 `NEBULA|` 协议前缀。独立成自由函数便于单测；tooltip 与
/// 侧栏任务副行共用这同一条判据。
fn qualifies_as_task_title(
    title: &str,
    program: &str,
    brand_name: &str,
    cwd: &str,
    ssh_destination: Option<&str>,
) -> bool {
    !title.is_empty()
        && title != "shell"
        && title != program
        && title != brand_name
        && title != cwd
        && Some(title) != ssh_destination
        && !title.starts_with("NEBULA|")
}

fn ssh_label(destination: Option<&str>, directory: &std::path::Path) -> Option<String> {
    let destination = destination?;
    let profiles = crate::ssh_profiles::SshProfiles::load(&directory.join("ssh_profiles.json"))
        .map_err(|error| log::warn!("Could not load SSH tab names: {error}"))
        .ok()?;
    profiles.labels().remove(destination)
}

impl TerminalView {
    pub(crate) fn inherits_windows_host_token(&self) -> bool {
        self.ssh_destination.is_none()
            && self
                .exec_context
                .as_ref()
                .is_some_and(|context| context.wsl_distribution().is_none())
            && matches!(
                self.suggest.suggest_env.history_scope(),
                crate::nebula_history::HistoryScope::Local
            )
    }

    /// 目录来自 shell 上报；不要从带 Powerline 图标的提示符或选区反推路径。
    pub fn working_directory(&self) -> Option<&str> {
        let path = self.cwd.as_str();
        ((path.starts_with('/') || std::path::Path::new(path).is_absolute())
            && !path.chars().any(char::is_control))
        .then_some(path)
    }

    pub fn copy_working_directory(
        &self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        use crate::i18n::Message;

        let Some(path) = self.working_directory() else { return false };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.to_owned()));
        crate::gpui_shell::toast::toast(
            window,
            cx,
            crate::display::ToastKind::Info,
            super::ui_language().text(Message::CommonCwdCopied),
        );
        true
    }

    pub(super) fn refresh_ssh_label(&mut self) {
        self.ssh_label =
            ssh_label(self.ssh_destination.as_deref(), &crate::display::nebula_data_dir());
    }

    /// A remote directory or OSC title must never replace a saved host name.
    /// Local tabs continue to use their working directory, not program titles.
    pub fn tab_label(&self) -> String {
        if let Some(destination) = &self.ssh_destination {
            return self.ssh_label.as_ref().unwrap_or(destination).clone();
        }
        last_path_component(&self.cwd)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|path| last_path_component(&path.to_string_lossy()))
            })
            .unwrap_or_else(|| ".".to_owned())
    }

    /// pane 创建时刻的挂钟时间（侧栏任务条目的 HH:MM 展示）。`spawn_at` 是
    /// 单调时钟，挂钟由 now 反推；只供显示，不参与任何计时判断。
    pub fn spawned_at(&self) -> std::time::SystemTime {
        std::time::SystemTime::now()
            .checked_sub(self.spawn_at.elapsed())
            .unwrap_or_else(std::time::SystemTime::now)
    }

    /// 标题里属于「任务」的部分：排除 shell 名、程序名、cwd、SSH 目的地与
    /// `NEBULA|` 协议前缀后剩下的才是用户视角的一行摘要（侧栏任务副行与
    /// tooltip 共用同一过滤，两边不能各解释一套）。
    pub(crate) fn task_summary(&self) -> Option<&str> {
        let program = self
            .running_program
            .as_deref()
            .or_else(|| self.ai_session.as_ref().map(|session| session.source.as_str()))?;
        let agent = crate::ai_agents::AgentKind::parse(program);
        let name = agent.map_or(program, |agent| agent.label());
        let title = self.title.trim();
        qualifies_as_task_title(title, program, name, &self.cwd, self.ssh_destination.as_deref())
            .then_some(title)
    }

    /// Hover uses only already observed state: no filesystem scans, SSH calls,
    /// or process enumeration on the render path.
    pub(crate) fn tab_tooltip(&self, tab_name: &str) -> String {
        let mut lines = vec![tab_name.to_owned()];
        if let Some(destination) = &self.ssh_destination {
            if let Some(label) = &self.ssh_label
                && label != tab_name
            {
                lines.push(label.clone());
            }
            if destination != tab_name {
                lines.push(destination.clone());
            }
        }
        if !self.cwd.trim().is_empty() && !lines.contains(&self.cwd) {
            lines.push(self.cwd.clone());
        }
        if self.exited.is_none()
            && !matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
        {
            let program = self
                .running_program
                .as_deref()
                .or_else(|| self.ai_session.as_ref().map(|session| session.source.as_str()));
            if let Some(program) = program {
                let agent = crate::ai_agents::AgentKind::parse(program);
                let name = agent.map_or(program, |agent| agent.label());
                let detail = match self.task_summary().filter(|_| agent.is_some()) {
                    Some(task) => format!("{name} · {task}"),
                    None => name.to_owned(),
                };
                if !lines.contains(&detail) {
                    lines.push(detail);
                }
            }
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_titles_exclude_identity_noise() {
        let cwd = "D:\\repo";
        // 真任务：用户输入的回合提示词残留。
        assert!(qualifies_as_task_title("去改，然后固定包名", "codex", "Codex", cwd, None));
        // 身份噪音：shell、进程名、品牌名、cwd、SSH 目的地、协议前缀、空串。
        for noise in [
            "",
            "shell",
            "codex",
            "Codex",
            "D:\\repo",
            "root@host:2222",
            "NEBULA|D:\\repo|main|claude",
        ] {
            assert!(
                !qualifies_as_task_title(
                    noise.trim(),
                    "codex",
                    "Codex",
                    cwd,
                    Some("root@host:2222")
                ),
                "{noise:?} is identity noise, not a task"
            );
        }
    }

    #[test]
    fn saved_ssh_names_are_loaded_from_the_selected_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let mut profiles = crate::ssh_profiles::SshProfiles::default();
        let mut host = profiles.for_destination("root@192.0.2.10:2222");
        host.label = Some("  SG-1 新加坡  ".into());
        profiles.upsert(host);
        profiles.save(&directory.path().join("ssh_profiles.json")).unwrap();
        assert_eq!(
            ssh_label(Some("root@192.0.2.10:2222"), directory.path()).as_deref(),
            Some("SG-1 新加坡")
        );
        assert_eq!(ssh_label(Some("root@192.0.2.11"), directory.path()), None);
        assert_eq!(ssh_label(None, directory.path()), None);
        assert_eq!(
            ssh_label(Some("root@192.0.2.10:2222"), &directory.path().join("isolated")),
            None
        );
    }
}
