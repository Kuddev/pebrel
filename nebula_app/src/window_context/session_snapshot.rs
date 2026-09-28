//! Legacy shell adapter for shared session snapshots.

use super::*;

impl WindowContext {
    /// Current tab list + per-tab cwd as a persistable session.
    pub(super) fn session_snapshot(&self) -> session::Session {
        let active_tab = self
            .tabs
            .iter()
            .take(self.active_tab)
            .filter(|tab| tab.doc.is_none() && tab.image.is_none() && !tab.settings)
            .count();
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| tab.doc.is_none() && tab.image.is_none() && !tab.settings)
            .map(|tab| self.tab_session(tab))
            .collect();
        let mut session = session::Session::new(active_tab.min(tabs.len().saturating_sub(1)), tabs);
        let maximized = self.display.window.is_maximized();
        session.window = Some(if maximized {
            // Maximized: the live inner size is the whole monitor — remember
            // the last known NORMAL size instead.
            session::WindowState {
                width: self.windowed_size.width,
                height: self.windowed_size.height,
                maximized,
            }
        } else {
            // Normal state: take the current size straight from the window.
            // The cached bookkeeping once picked up a physical-domain value,
            // and a restored window then ballooned by the DPI factor on
            // every relaunch.
            let logical: LogicalSize<u32> =
                self.display.window.inner_size().to_logical(self.display.window.scale_factor);
            session::WindowState { width: logical.width, height: logical.height, maximized }
        });
        session
    }

    /// One tab as a persistable record: focused-pane cwd, launch identity and
    /// the full split tree. Shared by the session autosave and the workspace
    /// export so both always describe tabs identically.
    pub(super) fn tab_session(&self, tab: &TabEntry) -> session::TabSession {
        let pane_cwd = |id: PaneId| {
            self.pane(id).map(|p| p.nebula_state.cwd.trim().to_owned()).unwrap_or_default()
        };
        // 每个叶子除 cwd 外还记录「此刻前台的 AI 对话」：running_program 由
        // hook/OSC 设置、133;D 收尾清除，是「快照瞬间它还开着」的存活判据；
        // 会话 id 必须与它同源（id 记录后前台可能换了别的程序）。只有 hook
        // 能报 id 的 claude/codex 走精确 resume，OSC 认出的裸 claude 退化
        // `--continue`，其余来源不接续。
        let pane_agent = |id: PaneId| -> Option<session::AgentSession> {
            let state = &self.pane(id)?.nebula_state;
            let program = state.running_program.as_deref()?;
            match &state.ai_session {
                Some(identity) if identity.source == program => Some(session::AgentSession {
                    session_file: None,
                    source: identity.source.clone(),
                    session_id: Some(identity.session_id.clone()),
                }),
                _ if matches!(program, "claude" | "codex") => Some(session::AgentSession {
                    session_file: None,
                    source: program.to_owned(),
                    session_id: None,
                }),
                _ => None,
            }
        };
        let mut leaves = Vec::new();
        tab.layout.leaves(&mut leaves);
        session::TabSession {
            cwd: pane_cwd(tab.active_pane),
            custom_name: tab.custom_name.clone(),
            color: tab.custom_color,
            launch: Some(Self::launch_session(&tab.launch)),
            layout: Some(Self::layout_session(&tab.layout, &pane_cwd, &pane_agent)),
            active_pane: leaves.iter().position(|id| *id == tab.active_pane).unwrap_or(0),
        }
    }

    /// The persistable subset of a tab's launch identity. Document/settings
    /// tabs are filtered out before this is called; mapping them to `Default`
    /// keeps the function total without giving them a session meaning.
    pub(super) fn launch_session(launch: &TabLaunch) -> session::LaunchSession {
        match launch {
            TabLaunch::Default
            | TabLaunch::Document(_)
            | TabLaunch::Image(_)
            | TabLaunch::Settings => session::LaunchSession::Default,
            TabLaunch::Shell { name, shell } => session::LaunchSession::Shell {
                name: name.clone(),
                program: shell.program().to_owned(),
                args: shell.args().to_vec(),
            },
            TabLaunch::Profile(profile) => session::LaunchSession::Profile {
                name: profile.name.clone(),
                command: profile.command.clone(),
                args: profile.args.clone(),
                cwd: profile.cwd.as_ref().map(|path| path.to_string_lossy().into_owned()),
                shell_id: profile.shell_id.clone(),
            },
            TabLaunch::Ssh(host) => session::LaunchSession::Ssh { host: host.clone() },
        }
    }

    /// Serialize a layout tree, resolving each leaf to its pane's cwd plus the
    /// AI conversation running in it (if any).
    pub(super) fn layout_session(
        layout: &Layout,
        pane_cwd: &impl Fn(PaneId) -> String,
        pane_agent: &impl Fn(PaneId) -> Option<session::AgentSession>,
    ) -> session::LayoutSession {
        match layout {
            Layout::Leaf(id) => session::LayoutSession::Pane {
                launch: None,
                cwd: pane_cwd(*id),
                agent: pane_agent(*id),
            },
            Layout::Split { direction, ratio, first, second, .. } => {
                session::LayoutSession::Split {
                    axis: match direction {
                        crate::display::SplitDirection::LeftRight => session::SplitAxis::LeftRight,
                        crate::display::SplitDirection::TopBottom => session::SplitAxis::TopBottom,
                    },
                    ratio_permille: (ratio.clamp(0.0, 1.0) * 1000.0).round() as u16,
                    first: Box::new(Self::layout_session(first, pane_cwd, pane_agent)),
                    second: Box::new(Self::layout_session(second, pane_cwd, pane_agent)),
                }
            },
        }
    }
}
