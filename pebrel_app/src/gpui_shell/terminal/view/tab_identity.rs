//! Cached host identity and live task details for terminal tab chrome.

use super::{TerminalView, last_path_component};

fn ssh_label(destination: Option<&str>, directory: &std::path::Path) -> Option<String> {
    let destination = destination?;
    let profiles = crate::ssh_profiles::SshProfiles::load(&directory.join("ssh_profiles.json"))
        .map_err(|error| log::warn!("Could not load SSH tab names: {error}"))
        .ok()?;
    profiles.labels().remove(destination)
}

/// Resolve a tab title without looking up the directory if a title is already known.
fn resolve_label<'a>(
    static_titles: bool,
    ai_task: impl FnOnce() -> Option<&'a str>,
    ssh: Option<(&str, Option<&str>)>,
    directory: impl FnOnce() -> Option<String>,
) -> String {
    if !static_titles && let Some(task) = ai_task() {
        return task.to_owned();
    }
    if let Some((destination, saved)) = ssh {
        return saved.unwrap_or(destination).to_owned();
    }
    directory().unwrap_or_else(|| ".".to_owned())
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

    /// Follow the current AI task unless the user keeps the directory or SSH
    /// name. Workspace manual names take precedence over either mode.
    pub fn tab_label(&self, cx: &gpui::App) -> String {
        let static_titles = crate::gpui_shell::config::static_tab_titles(cx);
        resolve_label(
            static_titles,
            || self.reported_ai_task(),
            self.ssh_destination
                .as_deref()
                .map(|destination| (destination, self.ssh_label.as_deref())),
            || {
                last_path_component(&self.cwd).or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .and_then(|path| last_path_component(&path.to_string_lossy()))
                })
            },
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
            && pebrel_terminal::event::shell_title_payload(title).is_none())
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
        let local = || Some("pebrel".into());
        assert_eq!(
            resolve_label(true, || panic!("fixed titles do not read the task"), None, local),
            "pebrel"
        );
        for static_titles in [false, true] {
            assert_eq!(resolve_label(static_titles, || None, None, local), "pebrel");
            assert_eq!(resolve_label(static_titles, || None, None, || None), ".");
        }
    }

    #[test]
    fn task_and_ssh_labels_do_not_resolve_the_directory() {
        let unused = || panic!("the directory is only needed for a local fallback");
        let host = Some(("deploy@host", Some("prod")));
        assert_eq!(resolve_label(false, || Some("Build fix"), host, unused), "Build fix");
        assert_eq!(resolve_label(true, || Some("Build fix"), host, unused), "prod");
        assert_eq!(
            resolve_label(false, || None, Some(("deploy@host", None)), unused),
            "deploy@host"
        );
    }
}
