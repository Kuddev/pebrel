//! Interactive split requests and immediate, pane-scoped split creation.

use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use crate::runtime_api::ApiError;
use crate::session::LaunchSession;

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingSplit {
    pane_id: u64,
    direction: SplitDirection,
}

impl NebulaWorkspace {
    /// UI entry point. Runtime API callers use `split_focused` without a picker.
    pub(super) fn request_split(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(WorkspaceTab::Terminal { focused, .. }) = self.tabs.get(self.active) else {
            return;
        };
        let request = PendingSplit { pane_id: *focused, direction };
        if cx
            .try_global::<crate::gpui_shell::config::Settings>()
            .is_some_and(|settings| settings.split_shell_picker)
        {
            self.open_shell_palette(window, cx);
            self.pending_split = Some(request);
        } else if let Err(error) = self.split_at(request, None, window, cx) {
            self.report_split_error(error, window, cx);
        }
    }

    fn report_split_error(&self, error: ApiError, window: &mut Window, cx: &mut Context<Self>) {
        crate::gpui_shell::toast::toast(
            window,
            cx,
            crate::gpui_shell::toast::ToastKind::Warning,
            workspace_ui_language()
                .format(crate::i18n::Message::WorkspaceSplitFailed, &[("error", &error.message)]),
        );
    }

    /// Consume the captured anchor before dismissing the shared launcher. A closed
    /// source never falls back to the current focus or creates an unrelated tab.
    pub(super) fn finish_split_choice(
        &mut self,
        launch: LaunchSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(request) = self.pending_split.take() else { return false };
        self.dismiss_palette_state();
        if let Err(error) = self.split_at(request, Some(launch), window, cx) {
            self.report_split_error(error, window, cx);
            self.focus_active(window, cx);
        }
        true
    }

    pub(super) fn launch_palette_ssh(
        &mut self,
        host: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finish_split_choice(LaunchSession::Ssh { host: host.clone() }, window, cx) {
            return;
        }
        self.dismiss_palette_state();
        self.add_ssh_terminal(host, window, cx);
    }

    pub(super) fn split_focused(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, ApiError> {
        let Some(WorkspaceTab::Terminal { focused, .. }) = self.tabs.get(self.active) else {
            return Err(ApiError::new("invalid_state", "the active tab cannot be split"));
        };
        self.split_at(PendingSplit { pane_id: *focused, direction }, None, window, cx)
    }

    fn split_at(
        &mut self,
        request: PendingSplit,
        selected: Option<LaunchSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, ApiError> {
        let PendingSplit { pane_id, direction } = request;
        let active = self
            .tab_of_pane(pane_id)
            .ok_or_else(|| ApiError::new("invalid_state", "the source pane has closed"))?;
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(active) else {
            return Err(ApiError::new("invalid_state", "the source tab cannot be split"));
        };
        let anchor = panes.iter().find(|pane| pane.id == pane_id).ok_or_else(|| {
            ApiError::new("action_failed", "the source pane is missing from the split tree")
        })?;
        let (cols, rows, identity, launch) = {
            let view = anchor.view.read(cx);
            let explicit = selected.is_some();
            let mut identity = selected.unwrap_or_else(|| view.session_launch.clone());
            // Inheritance uses the live pane directory, not a profile's startup directory.
            if !explicit && let LaunchSession::Profile { cwd, .. } = &mut identity {
                *cwd = None;
            }
            let (identity, launch) = match &identity {
                LaunchSession::Ssh { host } => {
                    let cwd = (view.ssh_destination.as_deref() == Some(host.as_str()))
                        .then(|| {
                            view.remote_cwd().or_else(|| {
                                self.remote_browser.path_for(pane_id, host).map(ToOwned::to_owned)
                            })
                        })
                        .flatten();
                    let launch = TerminalLaunch::Ssh { destination: host.clone(), cwd };
                    (identity, launch)
                },
                _ => {
                    // An explicitly selected target follows its own distro/user/directory.
                    let kind = if explicit {
                        tab_duplication::CopyKind::Selected
                    } else {
                        tab_duplication::CopyKind::Split
                    };
                    let (identity, cwd) = tab_duplication::copy_launch(
                        identity,
                        kind,
                        tab_duplication::PaneOrigin::of(view),
                    );
                    let launch = Self::terminal_launch_from_session(&identity, cwd);
                    (identity, launch)
                },
            };
            (view.grid_cols() as u16, view.grid_rows() as u16, identity, launch)
        };
        let grid = match direction {
            SplitDirection::LeftRight => ((cols / 2).max(2), rows.max(2)),
            SplitDirection::TopBottom => (cols.max(2), (rows / 2).max(2)),
        };
        let pane = self.new_pane(grid, launch, None, window, cx);
        if !matches!(identity, LaunchSession::Default) {
            pane.view.update(cx, |view, _| view.session_launch = identity);
        }
        let new_id = pane.id;
        let Some(WorkspaceTab::Terminal { panes, tree, focused, zoomed, .. }) =
            self.tabs.get_mut(active)
        else {
            pane.view.read(cx).shutdown();
            return Err(ApiError::new("action_failed", "the source terminal tab changed"));
        };
        if !tree.split_leaf(pane_id, new_id, direction, 0.5) {
            pane.view.read(cx).shutdown();
            return Err(ApiError::new("action_failed", "the source pane could not be split"));
        }
        panes.push(pane);
        *focused = new_id;
        *zoomed = false;
        self.active = active;
        self.reveal_active_tab();
        self.mark_structural_resize(active, cx);
        self.focus_active(window, cx);
        self.sync_side_panel_to_active(true, cx);
        cx.notify();
        Ok(new_id)
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
