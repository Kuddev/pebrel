//! Cached host identity and live task details for terminal tab chrome.

use super::{TerminalView, last_path_component};

pub(super) struct ManualSshLabel {
    destination: String,
    label: String,
}

fn ssh_label(destination: Option<&str>, directory: &std::path::Path) -> Option<String> {
    let destination = destination?;
    let profiles = crate::ssh_profiles::SshProfiles::load(&directory.join("ssh_profiles.json"))
        .map_err(|error| log::warn!("Could not load SSH tab names: {error}"))
        .ok()?;
    profiles.labels().remove(destination)
}

/// Name for a typed connection already parsed by the shared SSH authority:
/// the saved host label, else `user@host[:port]`.
fn manual_ssh_label(destination: &str, directory: &std::path::Path) -> Option<String> {
    // The parser returns `ssh://` only for ports; profiles store `host:port`.
    let (address, port) = crate::display::split_destination_port(
        destination.strip_prefix("ssh://").unwrap_or(destination),
    );
    let destination = crate::display::join_destination_port(&address, &port);
    if destination.is_empty() || destination.chars().any(char::is_control) {
        return None;
    }
    ssh_label(Some(&destination), directory).or(Some(destination))
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

    /// The existing connection stack owns the lifetime; OSC 133;D only ends a
    /// foreground command. Load saved labels only when the destination changes.
    pub(super) fn refresh_manual_ssh_label(&mut self) -> bool {
        let destination = self
            .ssh_destination
            .is_none()
            .then(|| self.suggest.completion_context.ssh_destination())
            .flatten();
        if self.manual_ssh_label.as_ref().map(|host| host.destination.as_str()) == destination {
            return false;
        }
        self.manual_ssh_label = destination.and_then(|destination| {
            Some(ManualSshLabel {
                destination: destination.to_owned(),
                label: manual_ssh_label(destination, &crate::display::nebula_data_dir())?,
            })
        });
        true
    }

    /// Native AI task titles name the focused pane; ordinary shells retain their
    /// directory or saved SSH host name, including a typed SSH connection.
    /// Workspace manual names take precedence.
    pub fn tab_label(&self) -> String {
        if let Some(task) = self.reported_ai_task() {
            return task.to_owned();
        }
        if let Some(destination) = &self.ssh_destination {
            return self.ssh_label.as_ref().unwrap_or(destination).clone();
        }
        if self.exited.is_none()
            && let Some(host) = &self.manual_ssh_label
        {
            return host.label.clone();
        }
        let local = self.local_tab_label();
        if self.running_program.as_deref() != Some("ssh") {
            return local;
        }
        // An unparsed ssh line still must not look like a plain local tab.
        format!("{local} · ssh")
    }

    fn local_tab_label(&self) -> String {
        last_path_component(&self.cwd)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|path| last_path_component(&path.to_string_lossy()))
            })
            .unwrap_or_else(|| ".".to_owned())
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
    fn typed_ssh_names_follow_saved_labels_and_destinations() {
        let directory = tempfile::tempdir().unwrap();
        let mut profiles = crate::ssh_profiles::SshProfiles::default();
        let mut host = profiles.for_destination("root@192.0.2.10:2222");
        host.label = Some("SG-1 新加坡".into());
        profiles.upsert(host);
        profiles.save(&directory.path().join("ssh_profiles.json")).unwrap();
        let label = |line| {
            crate::ssh::ssh_destination(line)
                .and_then(|destination| manual_ssh_label(&destination, directory.path()))
        };

        for line in [
            "ssh -p 2222 root@192.0.2.10",
            "ssh -l root -p2222 192.0.2.10",
            "/usr/bin/ssh ssh://root@192.0.2.10:2222",
        ] {
            assert_eq!(label(line).as_deref(), Some("SG-1 新加坡"), "{line}");
        }
        for (port, name) in [(2222, "IPv6 production"), (2223, "IPv6 staging")] {
            let mut host = profiles.for_destination(&format!("root@[2001:db8::10]:{port}"));
            host.label = Some(name.into());
            profiles.upsert(host);
        }
        profiles.save(&directory.path().join("ssh_profiles.json")).unwrap();
        assert_eq!(label("ssh -p 2222 root@2001:db8::10").as_deref(), Some("IPv6 production"));
        assert_eq!(label("ssh -p 2223 root@2001:db8::10").as_deref(), Some("IPv6 staging"));
        assert_eq!(label("ssh -p 22 deploy@example.com").as_deref(), Some("deploy@example.com"));
        assert_eq!(label("ssh.exe -t build-box").as_deref(), Some("build-box"));
        for line in ["ssh host uptime", "ssh -N -L 8080:localhost:80 host", "ssh host | tee log"] {
            assert_eq!(label(line), None, "{line}");
        }
    }
}
