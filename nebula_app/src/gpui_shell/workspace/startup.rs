//! Initial tab policy and promotion of a welcome tab into its chosen session.

use super::welcome::{WelcomeEvent, WelcomeView};
use super::*;
use nebula_settings::{RuntimeSettings, TerminalStartup};

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;
#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

impl NebulaWorkspace {
    pub(super) fn initialize_tabs(
        &mut self,
        startup: windowing::WorkspaceStartup,
        runtime: &RuntimeSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use windowing::WorkspaceStartup;

        match startup {
            WorkspaceStartup::RestoreUpdate(session) => {
                if !self.restore_update_session(&session, runtime.resume_ai, window, cx) {
                    self.add_configured_tab(Self::cold_start_cwd(), runtime, window, cx);
                }
            },
            WorkspaceStartup::RestoreOrDefault => {
                // Only the first window restores the saved session.
                if !runtime.restore_session
                    || !self.try_restore_session(runtime.resume_ai, window, cx)
                {
                    self.add_configured_tab(Self::cold_start_cwd(), runtime, window, cx);
                }
            },
            WorkspaceStartup::Configured { cwd } => {
                self.add_configured_tab(cwd, runtime, window, cx);
            },
            WorkspaceStartup::NewTerminal { cwd } => {
                self.add_terminal_at(cwd, None, window, cx);
            },
            WorkspaceStartup::LaunchTerminal { cwd, launch } => {
                self.add_terminal_with(launch, cwd, None, window, cx);
            },
            WorkspaceStartup::Empty => {},
        }
    }

    pub(super) fn add_configured_tab(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        runtime: &RuntimeSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if runtime.terminal_startup == TerminalStartup::Welcome
            && self.window_role == windowing::WindowRole::Regular
        {
            self.add_welcome(cwd, window, cx);
        } else {
            self.add_terminal_at(cwd, None, window, cx);
        }
    }

    pub(super) fn add_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let runtime = RuntimeSettings::load();
        if runtime.terminal_startup == TerminalStartup::Welcome
            && self.window_role == windowing::WindowRole::Regular
        {
            // The local choice must not inherit an SSH connection identity.
            let cwd = Self::startup_directory().or_else(|| self.active_local_cwd(cx));
            self.add_welcome(cwd, window, cx);
            return;
        }
        // Preserve the existing shell/WSL directory policy in direct mode.
        let (launch, cwd) = match Self::startup_directory() {
            Some(dir) => (shell_launch::configured_local_launch(cx), Some(dir)),
            None => self.new_tab_from_focused(cx),
        };
        self.add_terminal_with(launch, cwd, None, window, cx);
    }

    pub(super) fn add_welcome(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_settings(window, cx);
        let view = cx.new(WelcomeView::new);
        let subscription = cx.subscribe_in(&view, window, Self::on_welcome_event);
        self.insert_new_tab(WorkspaceTab::Welcome { view, cwd, _subscription: subscription });
        self.focus_active(window, cx);
        self.sync_side_panel_to_active(true, cx);
        cx.notify();
    }

    pub(super) fn on_welcome_event(
        &mut self,
        view: &Entity<WelcomeView>,
        event: &WelcomeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.welcome_index(view) else { return };
        self.activate_tab(index, window, cx);
        match event {
            WelcomeEvent::Local => {
                let WorkspaceTab::Welcome { cwd, .. } = &self.tabs[index] else { return };
                self.add_terminal_at(cwd.clone(), None, window, cx);
                self.finish_welcome_tab(Some(view.clone()), window, cx);
            },
            WelcomeEvent::Ssh => {
                self.open_settings(window, cx);
                if let Some((settings, _)) = &self.settings_surface {
                    settings.update(cx, |settings, cx| settings.show_ssh_hosts(window, cx));
                }
            },
        }
    }

    pub(super) fn active_welcome(&self) -> Option<Entity<WelcomeView>> {
        match self.tabs.get(self.active) {
            Some(WorkspaceTab::Welcome { view, .. }) => Some(view.clone()),
            _ => None,
        }
    }

    fn welcome_index(&self, target: &Entity<WelcomeView>) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| matches!(tab, WorkspaceTab::Welcome { view, .. } if view == target))
    }

    /// Launch through the existing local/SSH entry, then adopt its tab in place.
    /// This synchronous transition preserves ordering, labels and Runtime tab ID.
    pub(super) fn finish_welcome_tab(
        &mut self,
        welcome: Option<Entity<WelcomeView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = welcome.as_ref().and_then(|view| self.welcome_index(view)) else {
            return;
        };
        let created = self.active;
        if !self.tabs.get(created).is_some_and(WorkspaceTab::is_terminal) {
            return;
        }
        let (tab, launch_meta) = self.remove_tab_at(created).expect("new terminal exists");
        let index = index - usize::from(created < index);
        let (_, mut meta) = self.remove_tab_at(index).expect("welcome still exists");
        meta.launch = launch_meta.launch;
        meta.shell_tag = launch_meta.shell_tag;
        self.insert_tab_at(index, tab, meta);
        self.active = index;
        self.reveal_active_tab();
        self.focus_active(window, cx);
        self.sync_side_panel_to_active(true, cx);
        cx.notify();
    }
}
