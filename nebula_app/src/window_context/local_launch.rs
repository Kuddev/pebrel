//! Local child spawn options and profile overlays.

use super::*;

impl WindowContext {
    pub(super) fn default_profile(config: &UiConfig, explicit_command: bool) -> Option<Profile> {
        if explicit_command {
            return None;
        }
        let requested = crate::display::nebula_settings_value("shell")
            .or_else(|| crate::display::nebula_settings_value("executor"));
        // A long-lived legacy window must see edits made by the local editor.
        let profiles = crate::terminal_profiles::TerminalProfiles::load()
            .map(|store| store.as_config_profiles())
            .unwrap_or_else(|_| config.profiles.clone());
        crate::profile_launch::selected_local_profile(&profiles, requested.as_deref(), false)
    }

    /// Spawn a new pane into the pool without attaching it to any tab. `cwd`
    /// overrides the shell's startup directory when set. Returns the new pane's
    /// id, or `None` if the shell failed to start.
    pub(super) fn spawn_pane_detached(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        size_info: crate::display::SizeInfo,
    ) -> Option<PaneId> {
        self.spawn_pane_detached_with(cwd, size_info, None)
    }

    /// Like [`Self::spawn_pane_detached`] with an optional shell override
    /// (quick-launch profiles run their own command instead of the default).
    pub(super) fn spawn_pane_detached_with(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        size_info: crate::display::SizeInfo,
        shell: Option<nebula_terminal::tty::Shell>,
    ) -> Option<PaneId> {
        self.spawn_local_pane(cwd, size_info, shell, None)
    }

    pub(super) fn spawn_profile_pane(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        size_info: crate::display::SizeInfo,
        shell: tty::Shell,
        environment: &crate::profile_environment::ProfileEnvironment,
    ) -> Option<PaneId> {
        self.spawn_local_pane(cwd, size_info, Some(shell), Some(environment))
    }

    pub(super) fn spawn_pane_for_profile(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        size_info: crate::display::SizeInfo,
    ) -> Option<PaneId> {
        match self.tabs.get(self.active_tab).map(|tab| tab.launch.clone()) {
            Some(TabLaunch::Profile(profile)) => {
                self.spawn_profile_pane(cwd, size_info, profile.shell(), &profile.env)
            },
            _ => self.spawn_pane_detached(cwd, size_info),
        }
    }

    fn spawn_local_pane(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        size_info: crate::display::SizeInfo,
        shell: Option<tty::Shell>,
        environment: Option<&crate::profile_environment::ProfileEnvironment>,
    ) -> Option<PaneId> {
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        let window_id = self.display.window.id();
        let mut pty_config = self.config.pty_config();
        // NOTE: the executor choice (PowerShell/Bash) is applied inside
        // `tty::windows::cmdline` from `nebula_settings.txt` whenever
        // `pty_config.shell` is `None` — it must NOT be overridden here, or the
        // bash path would lose its Nebula rcfile (OSC 7 cwd / prompt contract).
        // A profile override (`shell` param) intentionally bypasses that.
        if shell.is_some() {
            pty_config.shell = shell;
        }
        if cwd.is_some() {
            pty_config.working_directory = cwd;
        }
        if let Some(environment) = environment {
            crate::profile_launch::apply_local_profile_options(&mut pty_config, environment);
        }
        match Self::create_pane(
            &size_info,
            window_id,
            &self.config,
            pty_config,
            &self.proxy,
            pane_id,
        ) {
            Ok(pane) => {
                self.panes.push(pane);
                Some(pane_id)
            },
            Err(err) => {
                error!("Failed to spawn pane: {err}");
                None
            },
        }
    }
}
