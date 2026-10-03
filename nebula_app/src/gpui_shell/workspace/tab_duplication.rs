//! Duplicate launch identity and location without sharing a live session.

use gpui::{Context, Window};

use super::{NebulaWorkspace, WorkspaceTab, new_tab_insert_index};
use crate::session::{LaunchSession, TabSession};

pub(super) fn inherit_guest_directory(launch: &mut LaunchSession, cwd: &str) -> bool {
    let (program, args) = match launch {
        LaunchSession::Shell { program, args, .. } => (program, args),
        LaunchSession::Profile { command, args, .. } => (command, args),
        _ => return false,
    };
    let Some(updated) = crate::shell_detect::wsl_args_at(program, args, cwd) else {
        return false;
    };
    *args = updated;
    true
}

impl NebulaWorkspace {
    pub(super) fn duplicate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(WorkspaceTab::Terminal { panes, tree, focused, .. }) = self.tabs.get(ix) else {
            return;
        };
        let meta = self.meta(ix);
        let layout = crate::gpui_shell::session_restore::layout_from_tree(tree, &|id| {
            let pane = panes.iter().find(|pane| pane.id == id).expect("split leaf owns a pane");
            let view = pane.view.read(cx);
            let mut launch = view.session_launch.clone();
            let cwd = if let Some(destination) = &view.ssh_destination {
                launch = LaunchSession::Ssh { host: destination.clone() };
                view.remote_cwd()
                    .or_else(|| {
                        self.remote_browser.path_for(id, destination).map(ToOwned::to_owned)
                    })
                    .unwrap_or_default()
            } else {
                // The current pane directory takes precedence over a profile's startup directory.
                if let LaunchSession::Profile { cwd, .. } = &mut launch {
                    *cwd = None;
                }
                view.cwd.clone()
            };
            (cwd, None, Some(launch), pane.custom_name.clone())
        });
        let duplicate = TabSession {
            cwd: String::new(),
            custom_name: meta.custom_name,
            color: meta.color,
            launch: meta.launch,
            active_pane: tree.leaves().iter().position(|id| id == focused).unwrap_or(0),
            layout: Some(layout),
        };
        if self.settings_open {
            self.leave_settings(window, cx);
        }
        let position = nebula_settings::RuntimeSettings::load().new_tab_position;
        let at = new_tab_insert_index(position, self.active, self.tabs.len());
        if self.restore_tab_at(&duplicate, false, at, window, cx) {
            self.active = at;
            self.reveal_active_tab();
            self.focus_active(window, cx);
            self.sync_side_panel_to_active(true, cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_wsl_shell_uses_guest_directory_even_when_it_is_not_on_the_host() {
        use crate::gpui_shell::terminal::view::TerminalLaunch;

        let mut launch = LaunchSession::Shell {
            name: "Debian".into(),
            program: "wsl.exe".into(),
            args: vec!["-d".into(), "Debian".into(), "--cd".into(), "/old".into()],
        };
        assert!(inherit_guest_directory(&mut launch, "/home/guest/new project"));
        let TerminalLaunch::Local { cwd, shell: Some(shell), .. } =
            NebulaWorkspace::terminal_launch_from_session(&launch, None)
        else {
            panic!("duplicate must use a new local WSL process");
        };
        assert!(cwd.is_none(), "the guest cwd must not be passed as a host directory");
        assert_eq!(shell.program(), "wsl.exe");
        assert_eq!(shell.args(), ["--cd", "/home/guest/new project", "-d", "Debian"]);
        let LaunchSession::Shell { args, .. } = launch else { panic!("shell identity lost") };
        assert_eq!(args, ["--cd", "/home/guest/new project", "-d", "Debian"]);
    }

    #[test]
    fn imported_wsl_profile_keeps_its_user_and_shell() {
        let mut launch = LaunchSession::Profile {
            name: "Debian Zsh".into(),
            command: "wsl.exe".into(),
            args: ["-d", "Debian", "-u", "guest", "--exec", "zsh", "-l"].map(String::from).to_vec(),
            cwd: None,
            shell_id: None,
        };
        assert!(inherit_guest_directory(&mut launch, "/home/guest"));
        let LaunchSession::Profile { args, .. } = launch else { panic!("profile identity lost") };
        assert_eq!(
            args,
            ["--cd", "/home/guest", "-d", "Debian", "-u", "guest", "--exec", "zsh", "-l"]
        );
    }

    #[test]
    fn invalid_guest_directory_leaves_the_original_profile_untouched() {
        let original = LaunchSession::Shell {
            name: "Debian".into(),
            program: "wsl.exe".into(),
            args: ["-d", "Debian", "--cd", "/original"].map(String::from).to_vec(),
        };
        let mut launch = original.clone();
        assert!(!inherit_guest_directory(&mut launch, "/home/guest\r"));
        assert_eq!(launch, original);
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod layout_tests;
