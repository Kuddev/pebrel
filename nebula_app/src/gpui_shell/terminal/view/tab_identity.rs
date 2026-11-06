//! Cached host identity and live task details for terminal tab chrome.

use super::{TerminalView, last_path_component};

fn ssh_label(destination: Option<&str>, directory: &std::path::Path) -> Option<String> {
    let destination = destination?;
    let profiles = crate::ssh_profiles::SshProfiles::load(&directory.join("ssh_profiles.json"))
        .map_err(|error| log::warn!("Could not load SSH tab names: {error}"))
        .ok()?;
    profiles.labels().remove(destination)
}

/// 标签标题的唯一决策点：AI 任务标题（可选）、SSH 备注名、目录末级。
/// 收成纯函数，开关两侧才能在无 gpui 上下文时直接断言。
pub(super) fn resolve_label(
    static_titles: bool,
    ai_task: Option<&str>,
    ssh: Option<(&str, Option<&str>)>,
    directory: Option<&str>,
) -> String {
    if !static_titles && let Some(task) = ai_task {
        return task.to_owned();
    }
    if let Some((destination, saved)) = ssh {
        return saved.unwrap_or(destination).to_owned();
    }
    directory.map(str::to_owned).unwrap_or_else(|| ".".to_owned())
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

    /// Native AI task titles name the focused pane; ordinary shells retain their
    /// directory or saved SSH host name. Workspace manual names take precedence.
    pub fn tab_label(&self, cx: &gpui::App) -> String {
        let directory = last_path_component(&self.cwd).or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|path| last_path_component(&path.to_string_lossy()))
        });
        resolve_label(
            crate::gpui_shell::config::static_tab_titles(cx),
            self.reported_ai_task(),
            self.ssh_destination
                .as_deref()
                .map(|destination| (destination, self.ssh_label.as_deref())),
            directory.as_deref(),
        )
    }

    fn reported_ai_task(&self) -> Option<&str> {
        if self.exited.is_some()
            || matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
        {
            return None;
        }
        let program = self
            .running_program
            .as_deref()
            .or_else(|| self.ai_session.as_ref().map(|session| session.source.as_str()))?;
        let agent = crate::ai_agents::AgentKind::parse(program)?;
        let title = crate::ai_agents::native_title::task_name(agent, &self.title);
        (!title.is_empty()
            && title != "shell"
            && title != program
            && title != agent.label()
            && title != agent.display_name()
            && title != self.cwd
            && Some(title) != self.ssh_destination.as_deref()
            && !title.starts_with("NEBULA|"))
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
                let name = crate::ai_agents::AgentKind::parse(program)
                    .map_or(program, |agent| agent.label());
                let detail = match self.reported_ai_task() {
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

    #[test]
    fn static_titles_keep_the_directory_and_following_mode_prefers_the_ai_task() {
        // 上游行为：任务标题优先。
        assert_eq!(resolve_label(false, Some("pi@iy4"), None, Some("pebrel")), "pi@iy4");
        // 固定标题：任务名完全不参与，只留目录名。
        assert_eq!(resolve_label(true, Some("pi@iy4"), None, Some("pebrel")), "pebrel");
        // 固定标题不影响 SSH 备注名的既有优先级。
        assert_eq!(
            resolve_label(true, Some("pi@iy4"), Some(("deploy@host", Some("prod"))), Some("app")),
            "prod"
        );
        // 没有备注名时用目标地址。
        assert_eq!(
            resolve_label(true, None, Some(("deploy@host", None)), Some("app")),
            "deploy@host"
        );
        // 目录未知时回落到 `.`。
        assert_eq!(resolve_label(false, None, None, None), ".");
        assert_eq!(resolve_label(true, None, None, None), ".");
        // 非 AI 程序没有任务标题，两条路径同结果。
        assert_eq!(resolve_label(false, None, None, Some("VALE")), "VALE");
        assert_eq!(resolve_label(true, None, None, Some("VALE")), "VALE");
    }
}
