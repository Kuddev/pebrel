//! Opens a new tab for a non-PTY remote transport (SSH or serial).

use gpui::{Context, Window};
use nebula_split::SplitTree;

use super::{NebulaWorkspace, TabMeta, WorkspaceTab, new_tab_insert_index};
use crate::gpui_shell::terminal::view::TerminalLaunch;
use crate::session::LaunchSession;

impl NebulaWorkspace {
    pub(super) fn open_remote_tab(
        &mut self,
        launch: TerminalLaunch,
        session: LaunchSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let grid = self.inherited_grid(cx);
        let pane = self.new_pane(grid, launch, None, window, cx);
        let focused = pane.id;
        let tab = WorkspaceTab::Terminal {
            tree: SplitTree::leaf(pane.id),
            panes: vec![pane],
            focused,
            zoomed: false,
            broadcast: false,
        };
        let position = nebula_settings::RuntimeSettings::load().new_tab_position;
        let at = new_tab_insert_index(position, self.active, self.tabs.len());
        let meta = TabMeta {
            shell_tag: Self::launch_shell_tag(&session),
            launch: Some(session),
            ..TabMeta::default()
        };
        self.insert_tab_at(at, tab, meta);
        self.active = at;
        self.reveal_active_tab();
        self.focus_active(window, cx);
        self.sync_side_panel_to_active(true, cx);
        cx.notify();
    }
}
