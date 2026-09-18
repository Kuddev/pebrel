//! Cached host identity and live task details for terminal tab chrome.

use super::{TerminalView, last_path_component};

fn ssh_label(destination: Option<&str>, directory: &std::path::Path) -> Option<String> {
    let destination = destination?;
    let profiles = crate::ssh_profiles::SshProfiles::load(&directory.join("ssh_profiles.json"))
        .map_err(|error| log::warn!("Could not load SSH tab names: {error}"))
        .ok()?;
    profiles.labels().remove(destination)
}

impl TerminalView {
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
                let title = self.title.trim();
                let task = (!title.is_empty()
                    && title != "shell"
                    && title != program
                    && title != name
                    && title != self.cwd
                    && Some(title) != self.ssh_destination.as_deref()
                    && !title.starts_with("NEBULA|"))
                .then_some(title);
                let detail = match task.filter(|_| agent.is_some()) {
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
}
