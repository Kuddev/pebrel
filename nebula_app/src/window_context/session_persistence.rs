//! Session persistence and workspace import/export for window restoration.

use super::*;

impl WindowContext {
    /// Continue one live AI conversation in a fresh tab with a new session id.
    ///
    /// This deliberately recreates the shell instead of cloning a PTY/process.
    /// Profile/SSH tabs are excluded: injecting into a profile that starts the
    /// agent directly, or into an SSH authentication prompt, would turn the
    /// command into user input at the wrong protocol layer.
    pub(super) fn fork_ai_session(&mut self, index: usize) {
        let Some(tab) = self.tabs.get(index) else { return };
        let launch = match &tab.launch {
            TabLaunch::Default => TabLaunch::Default,
            TabLaunch::Shell { name, shell } => {
                TabLaunch::Shell { name: name.clone(), shell: shell.clone() }
            },
            _ => return,
        };
        let Some(pane) = self.pane(tab.active_pane) else { return };
        let Some(identity) = pane.nebula_state.ai_session.as_ref() else { return };
        let Some(agent) = crate::ai_agents::AgentKind::parse(&identity.source) else { return };
        let Some(command) = agent.fork_command(&identity.session_id) else { return };
        let cwd = (!pane.nebula_state.cwd.trim().is_empty())
            .then(|| std::path::PathBuf::from(pane.nebula_state.cwd.trim()))
            .filter(|path| path.is_dir());
        let color = tab.custom_color;

        self.select_tab(index);
        let shell = match &launch {
            TabLaunch::Default => Self::default_shell_override(&self.config),
            TabLaunch::Shell { shell, .. } => Some(shell.clone()),
            _ => unreachable!("launch was restricted above"),
        };
        let Some(pane_id) = self.spawn_pane_detached_with(cwd, self.display.size_info, shell)
        else {
            return;
        };
        self.insert_tab(
            TabEntry {
                layout: Layout::Leaf(pane_id),
                active_pane: pane_id,
                has_bell: false,
                custom_name: Some(format!("{} 分叉", agent.display_name())),
                custom_color: color,
                launch,
                doc: None,
                image: None,
                settings: false,
            },
            TabPlacement::Created,
        );
        self.resize_active_layout();
        if let Some(pane) = self.pane(pane_id) {
            pane.notifier.notify(command.into_bytes());
            pane.notifier.notify(vec![b'\r']);
        }
        self.sync_chrome_tabs();
        self.mark_session_dirty();
        self.dirty = true;
    }

    /// Export the whole window (`None`) or a single tab (`Some(index)`) as a
    /// workspace file — the same schema the crash-restore session uses, so
    /// "打开工作区" and session restore share one rebuild path.
    pub(super) fn export_workspace(&mut self, tab_index: Option<usize>) {
        let exportable =
            |tab: &&TabEntry| tab.doc.is_none() && tab.image.is_none() && !tab.settings;
        let tabs: Vec<_> = match tab_index {
            Some(index) => self
                .tabs
                .get(index)
                .filter(exportable)
                .map(|tab| self.tab_session(tab))
                .into_iter()
                .collect(),
            None => self.tabs.iter().filter(exportable).map(|tab| self.tab_session(tab)).collect(),
        };
        let Some(first) = tabs.first() else { return };

        // Single-tab exports name the file after the tab; whole-window exports
        // after the workspace. Path separators and friends must not leak into
        // the suggested file name.
        let stem = match tab_index {
            Some(_) => first
                .custom_name
                .clone()
                .or_else(|| {
                    first.cwd.rsplit(['/', '\\']).find(|part| !part.is_empty()).map(str::to_owned)
                })
                .unwrap_or_else(|| "tab".to_owned()),
            None => "workspace".to_owned(),
        };
        let stem: String = stem
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '-'
                } else {
                    c
                }
            })
            .collect();

        let Some(path) =
            self.display.save_workspace_dialog(&format!("{stem}.pebrel-workspace.json"))
        else {
            return;
        };
        let session = session::Session::new(0, tabs);
        if let Err(err) = session::save_to(&path, &session) {
            let user_error = crate::ux::UserFacingError::new(
                "工作区导出失败",
                "无法写入所选的工作区文件。",
                "确认该位置可写(或换一个目录)后重试。",
            )
            .details(err.to_string());
            self.message_buffer.push(crate::message_bar::Message::user_error(&user_error));
            self.dirty = true;
        }
    }

    /// Pick a workspace file and append its tabs — launch identity and split
    /// trees included — to this window, then focus the first imported tab.
    pub(super) fn import_workspace(&mut self) {
        let Some(path) = self.display.pick_workspace_dialog() else { return };
        let Some(session) = session::load_from(&path) else {
            let user_error = crate::ux::UserFacingError::new(
                "工作区导入失败",
                "所选文件不是可识别的 Pebrel 工作区。",
                "确认选择的是导出生成的 .pebrel-workspace.json 或旧版 .nebula-workspace.json 文件。",
            );
            self.message_buffer.push(crate::message_bar::Message::user_error(&user_error));
            self.dirty = true;
            return;
        };

        let before = self.tabs.len();
        for tab in &session.tabs {
            self.append_session_tab(tab);
        }
        let added = self.tabs.len() - before;
        if added > 0 {
            // append_session_tab leaves the last new tab active; land on the
            // first one, matching the saved workspace's reading order.
            self.select_tab(self.active_tab + 1 - added);
            self.sync_chrome_tabs();
            self.mark_session_dirty();
        } else {
            let user_error = crate::ux::UserFacingError::new(
                "工作区导入失败",
                "工作区文件里没有可恢复的标签页。",
                "该文件可能为空,或其中的会话都无法启动。",
            );
            self.message_buffer.push(crate::message_bar::Message::user_error(&user_error));
            self.dirty = true;
        }
    }

    /// Rebuild every saved tab of a restored session and refocus the tab that
    /// was active at close. The boot path only spawned a seed pane so the
    /// window exists; the real tabs — launch identity and split tree included
    /// — are appended here through the same path workspace import uses, then
    /// the seed is dismantled unless the CLI pinned its working directory.
    pub(super) fn restore_session_tabs(&mut self, session: &session::Session, keep_seed: bool) {
        let first_new = self.tabs.len();
        for tab in &session.tabs {
            self.append_session_tab(tab);
        }
        let restored = self.tabs.len() - first_new;
        if restored == 0 {
            // Every spawn failed: keep the seed rather than an empty window.
            return;
        }
        // Guarded: a failed spawn above leaves fewer tabs than were saved.
        if session.active_tab < restored {
            self.active_tab = first_new + session.active_tab;
        }
        if !keep_seed {
            // The seed is a plain single-pane tab at index 0 that never saw
            // user state — dismantle it in place instead of going through
            // close_tab's confirmation and focus logic.
            let seed = self.tabs.remove(0);
            let mut ids = Vec::new();
            seed.layout.leaves(&mut ids);
            for id in ids {
                if let Some(i) = self.pane_index(id) {
                    let pane = self.panes.remove(i);
                    let _ = pane.notifier.0.send(Msg::Shutdown);
                }
            }
            self.active_tab = self.active_tab.saturating_sub(1).min(self.tabs.len() - 1);
        }
        self.resize_active_layout();
        self.sync_chrome_tabs();
        self.dirty = true;
    }

    /// Append one saved tab: spawn its first pane per launch identity, rebuild
    /// its split tree, and re-apply name/color/focus. Returns `false` when the
    /// first pane could not spawn — the tab is skipped so the session's
    /// remaining tabs still restore.
    fn append_session_tab(&mut self, tab: &session::TabSession) -> bool {
        let before = self.tabs.len();
        let launch = tab.launch.as_ref().unwrap_or(&session::LaunchSession::Default);
        match launch {
            session::LaunchSession::Default => {
                self.append_default_tab(tab);
            },
            session::LaunchSession::Shell { name, program, args } => {
                let shell = nebula_terminal::tty::Shell::new(program.clone(), args.clone());
                self.spawn_tab_shell(name.clone(), shell, TabPlacement::AfterActive);
            },
            session::LaunchSession::Profile { name, command, args, cwd, shell_id } => {
                self.spawn_tab_profile_value(
                    crate::config::ui_config::Profile {
                        name: name.clone(),
                        command: command.clone(),
                        args: args.clone(),
                        cwd: cwd.as_ref().map(std::path::PathBuf::from),
                        shell_id: shell_id.clone(),
                        terminal_profile_id: None,
                    },
                    TabPlacement::AfterActive,
                );
            },
            session::LaunchSession::Ssh { host } => {
                self.spawn_tab_ssh(host.clone(), TabPlacement::AfterActive)
            },
        }
        if self.tabs.len() == before {
            // Cross-platform degradation: a workspace made on another OS may
            // name a program this machine lacks (wsl.exe on Linux, a distro
            // shell on Windows). The tab must survive as a default shell in
            // its saved directory rather than silently vanish; the SSH path
            // reports its own failure and gets no fallback tab.
            let is_command = matches!(
                launch,
                session::LaunchSession::Shell { .. } | session::LaunchSession::Profile { .. }
            );
            if !(is_command && self.append_default_tab(tab)) {
                return false;
            }
        }

        // The new tab is active now. Grow the saved split tree around its
        // first pane, then re-apply the saved presentation.
        let first_pane = self.tabs[self.active_tab].active_pane;
        if let Some(saved) = tab.layout.as_ref().filter(|layout| layout.pane_count() > 1) {
            let mut seed = Some(first_pane);
            if let Some(built) = self.rebuild_layout(saved, &mut seed) {
                let entry = &mut self.tabs[self.active_tab];
                entry.layout = built;
                let mut leaves = Vec::new();
                entry.layout.leaves(&mut leaves);
                entry.active_pane =
                    leaves.get(tab.active_pane).or(leaves.first()).copied().unwrap_or(first_pane);
                self.resize_active_layout();
            }
        }
        let entry = &mut self.tabs[self.active_tab];
        if tab.custom_name.is_some() {
            // `None` must not clobber the launch-derived label (SSH host,
            // shell name) that spawn_tab_* already set.
            entry.custom_name = tab.custom_name.clone();
        }
        entry.custom_color = tab.color;
        self.resume_agent_sessions(tab);
        true
    }

    /// 冷恢复接续 AI 对话（T1-2）：把保存的叶子与重建后的活动布局按同一
    /// DFS 序配对，给记录了前台对话的 pane 敲入 resume 命令。
    ///
    /// 注入走 ConPTY 输入队列（fastfetch intro 同款机制）：字节安静排队，
    /// shell 就绪后读到第一行输入才执行，不需要等提示符出现。首叶继承 tab
    /// 的 launch 身份，只有确定它是裸 shell（Default / Shell）才注入——
    /// Profile 可能直接启动任意程序（甚至就是 claude，命令会变成发给新
    /// 对话的假消息），SSH 连接期还有密码/口令交互，字节会落进错误的
    /// 输入框；其余叶子由 `rebuild_layout` 统一以默认 shell 重建，安全。
    fn resume_agent_sessions(&mut self, tab: &session::TabSession) {
        if !self.display.nebula_resume_ai {
            return;
        }
        let Some(saved) = tab.layout.as_ref() else { return };
        let mut live = Vec::new();
        self.tabs[self.active_tab].layout.leaves(&mut live);
        let launch = tab.launch.as_ref().unwrap_or(&session::LaunchSession::Default);
        let seed_is_plain_shell = matches!(
            launch,
            session::LaunchSession::Default | session::LaunchSession::Shell { .. }
        );
        // 某个叶子 spawn 失败时 rebuild_layout 会折叠父节点，后续配对右移
        // 错位——zip 在较短一侧截止。错位注入最多把 resume 敲进错的兄弟
        // pane，claude/codex 自己会报「找不到会话」，不会破坏任何数据。
        for (index, (leaf, pane_id)) in saved.leaves().iter().zip(live).enumerate() {
            let session::LayoutSession::Pane { agent: Some(agent), .. } = leaf else {
                continue;
            };
            if index == 0 && !seed_is_plain_shell {
                continue;
            }
            let Some(command) = agent.resume_command() else { continue };
            let Some(i) = self.pane_index(pane_id) else { continue };
            let pane = &mut self.panes[i];
            pane.notifier.notify(command.into_bytes());
            pane.notifier.notify(vec![b'\r']);
        }
    }

    /// Spawn a default-shell tab in a saved tab's first-leaf directory —
    /// both the `Default` launch path and the cross-platform fallback when a
    /// saved program does not exist on this machine.
    fn append_default_tab(&mut self, tab: &session::TabSession) -> bool {
        // The first pane adopts the tree's first leaf, so it must start in
        // that leaf's directory, not the focused pane's.
        let saved_cwd = tab.layout.as_ref().map(|layout| layout.first_cwd()).unwrap_or(&tab.cwd);
        let cwd = session::valid_dir(saved_cwd).or_else(|| self.display.startup_directory());
        let Some(id) = self.spawn_pane_detached(cwd, self.display.size_info) else {
            return false;
        };
        // 恢复不读新标签插入策略：保存的顺序才是权威，逐个追加在活动标签
        // 之后即可复现它。
        self.insert_tab(
            TabEntry {
                layout: Layout::Leaf(id),
                active_pane: id,
                has_bell: false,
                custom_name: None,
                custom_color: None,
                launch: TabLaunch::Default,
                doc: None,
                image: None,
                settings: false,
            },
            TabPlacement::AfterActive,
        );
        self.run_fastfetch_intro(id);
        true
    }

    /// Materialize a saved split tree. `seed` is the already-spawned first
    /// pane, adopted by the depth-first first leaf; every other leaf spawns a
    /// default shell in its saved cwd — matching live behaviour, where only a
    /// tab's first pane carries the launch identity. A leaf whose spawn fails
    /// collapses its parent to the surviving side, so a partially failed
    /// restore still yields a working tab.
    fn rebuild_layout(
        &mut self,
        node: &session::LayoutSession,
        seed: &mut Option<PaneId>,
    ) -> Option<Layout> {
        match node {
            session::LayoutSession::Pane { cwd, .. } => {
                if let Some(id) = seed.take() {
                    return Some(Layout::Leaf(id));
                }
                let cwd = session::valid_dir(cwd).or_else(|| self.display.startup_directory());
                self.spawn_pane_detached(cwd, self.display.size_info).map(Layout::Leaf)
            },
            session::LayoutSession::Split { axis, ratio_permille, first, second } => {
                let first = self.rebuild_layout(first, seed);
                let second = self.rebuild_layout(second, seed);
                match (first, second) {
                    (Some(first), Some(second)) => Some(Layout::Split {
                        direction: match axis {
                            session::SplitAxis::LeftRight => {
                                crate::display::SplitDirection::LeftRight
                            },
                            session::SplitAxis::TopBottom => {
                                crate::display::SplitDirection::TopBottom
                            },
                        },
                        ratio: (f32::from(*ratio_permille) / 1000.0).clamp(0.05, 0.95),
                        preview_ratio: None,
                        dragging: false,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                    (first, second) => first.or(second),
                }
            },
        }
    }

    /// Whether any pane (and its PTY) is still alive in this window.
    pub fn has_live_panes(&self) -> bool {
        !self.panes.is_empty()
    }

    /// Strip the live tabs off this window for mux residency (detach): the
    /// panes' PTYs keep running in-process, ready for re-attach. The final
    /// session snapshot is written here and the Drop one is suppressed —
    /// after the take below, Drop would see zero tabs and wipe the file.
    pub fn detach_panes(&mut self) -> DetachedWindow {
        session::save_final(&mut self.session_snapshot());
        self.session_exempt = true;
        DetachedWindow {
            panes: mem::take(&mut self.panes),
            tabs: mem::take(&mut self.tabs),
            active_tab: self.active_tab,
            next_pane_id: self.next_pane_id,
        }
    }

    /// Post-adoption fixups for a re-attached window.
    pub(super) fn finish_attach(&mut self) {
        // Prune stale leaves: a shell that exited during residency had its
        // pane reaped but its leaf kept. `close_pane` does the full tree
        // surgery (collapse split / drop empty tab / move focus); with the
        // pane already gone it touches no PTY.
        let live: std::collections::HashSet<PaneId> =
            self.panes.iter().map(|pane| pane.id).collect();
        let mut stale = Vec::new();
        for tab in &self.tabs {
            let mut ids = Vec::new();
            tab.layout.leaves(&mut ids);
            stale.extend(ids.into_iter().filter(|id| !live.contains(id)));
        }
        for id in stale {
            self.close_pane(id);
        }

        // Focus sanity: a tab's saved active pane may have been pruned.
        for tab in &mut self.tabs {
            let mut ids = Vec::new();
            tab.layout.leaves(&mut ids);
            if !ids.contains(&tab.active_pane) {
                if let Some(first) = ids.first() {
                    tab.active_pane = *first;
                }
            }
        }
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        }

        // The adopting window's geometry differs from the closed one's: size
        // the active tab now; background tabs resize on selection, as always.
        self.resize_active_layout();
        self.dirty = true;
    }

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
    fn tab_session(&self, tab: &TabEntry) -> session::TabSession {
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
                    source: identity.source.clone(),
                    session_id: Some(identity.session_id.clone()),
                }),
                _ if matches!(program, "claude" | "codex") => {
                    Some(session::AgentSession { source: program.to_owned(), session_id: None })
                },
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
    fn launch_session(launch: &TabLaunch) -> session::LaunchSession {
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
    fn layout_session(
        layout: &Layout,
        pane_cwd: &impl Fn(PaneId) -> String,
        pane_agent: &impl Fn(PaneId) -> Option<session::AgentSession>,
    ) -> session::LayoutSession {
        match layout {
            Layout::Leaf(id) => {
                session::LayoutSession::Pane { cwd: pane_cwd(*id), agent: pane_agent(*id) }
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

    /// 1 Hz autosave (piggybacks on the chrome clock tick): persist the session
    /// when it changed, so a crash or force-kill restores to within a second.
    /// Only the focused window writes — two open windows must not fight over
    /// the file every second; last-focused wins, which is also the window the
    /// user most plausibly wants back.
    pub fn autosave_session(&mut self) {
        if self.session_exempt {
            return;
        }
        let focused =
            self.pane(self.focused_pane_id()).is_some_and(|p| p.terminal.lock().is_focused);
        if !focused {
            return;
        }
        let snapshot = self.session_snapshot();
        if self.last_saved_session.as_ref() == Some(&snapshot) {
            return;
        }
        session::save(&snapshot);
        self.last_saved_session = Some(snapshot);
    }

    /// Drop the autosave dedup cache so the next tick rewrites the session
    /// file (another window's teardown just wrote ITS final snapshot over it).
    pub fn mark_session_dirty(&mut self) {
        self.last_saved_session = None;
    }

    /// Dock the whole layout of tab `source` into the active tab: the active
    /// layout becomes a 50/50 split with the docked tree on `nav`'s side, the
    /// source tab disappears from the bar, and focus follows the docked pane.
    /// Pure tree surgery — panes live in the window-level pool, so no PTY is
    /// touched beyond the resize at the end.
    pub(super) fn dock_tab_into_active(&mut self, source: usize, nav: crate::display::SplitNav) {
        use crate::display::{SplitDirection, SplitNav};

        if source >= self.tabs.len()
            || source == self.active_tab
            || self.tabs.len() < 2
            || self.tabs[source].doc.is_some()
            || self.tabs[source].settings
            || self.tabs[self.active_tab].doc.is_some()
            || self.tabs[self.active_tab].settings
        {
            return;
        }

        let src_entry = self.tabs.remove(source);
        if source < self.active_tab {
            self.active_tab -= 1;
        }

        let entry = &mut self.tabs[self.active_tab];
        // Temporarily park a placeholder leaf so the old tree can move.
        let old = mem::replace(&mut entry.layout, Layout::Leaf(src_entry.active_pane));
        let (direction, src_first) = match nav {
            SplitNav::Left => (SplitDirection::LeftRight, true),
            SplitNav::Right => (SplitDirection::LeftRight, false),
            SplitNav::Up => (SplitDirection::TopBottom, true),
            SplitNav::Down => (SplitDirection::TopBottom, false),
        };
        let (first, second) =
            if src_first { (src_entry.layout, old) } else { (old, src_entry.layout) };
        entry.layout = Layout::Split {
            direction,
            ratio: 0.5,
            preview_ratio: None,
            dragging: false,
            first: Box::new(first),
            second: Box::new(second),
        };
        // Focus follows the pane that accepted the dock operation.
        entry.active_pane = src_entry.active_pane;

        // A zoomed pane would hide the fresh split; drop the zoom.
        self.zoom = None;

        // Structural change: grids AND PTYs need their sizes immediately.
        self.resize_active_layout();
        self.dirty = true;
    }
}
