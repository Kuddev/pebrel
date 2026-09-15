//! Tab lifecycle: creation, selection, spawning by type (shell/SSH/doc/image/settings).

use super::*;

impl WindowContext {
    pub fn handle_tab_request(&mut self, request: TabRequest) -> bool {
        use crate::display::NebulaConfirm;
        match request {
            TabRequest::New => {
                self.spawn_tab();
                false
            },
            TabRequest::NewAtDirectory(path) => {
                if valid_new_tab_directory(&path) {
                    self.spawn_tab_at(Some(path), TabPlacement::Created);
                } else {
                    warn!(
                        "Refusing to open a terminal at a missing or non-directory tree root: \
                         {path:?}"
                    );
                }
                false
            },
            TabRequest::NewProfile(profile) => {
                self.spawn_tab_profile_value(profile, TabPlacement::Created);
                false
            },
            TabRequest::NewShell { name, shell } => {
                self.spawn_tab_shell(name, shell, TabPlacement::Created);
                false
            },
            TabRequest::NewSsh(host) => {
                self.spawn_tab_ssh(host, TabPlacement::Created);
                false
            },
            TabRequest::RetrySsh(host) => {
                self.retry_focused_ssh(host);
                false
            },
            TabRequest::OpenDoc(path) => {
                if crate::display::image_viewer::viewable_file(&path) {
                    self.open_image_tab(path);
                } else {
                    self.open_doc_tab(path);
                }
                false
            },
            TabRequest::OpenSettings => {
                self.open_settings_tab();
                false
            },
            TabRequest::Close => {
                if self
                    .tabs
                    .get(self.active_tab)
                    .is_some_and(|tab| tab.doc.is_some() || tab.image.is_some() || tab.settings)
                {
                    return self.close_tab(self.active_tab);
                }
                let id = self.focused_pane_id();
                // A pending confirm for this pane means the user re-triggered
                // the close (or pressed Enter, which re-dispatches this
                // request): proceed for real.
                let confirmed = matches!(
                    self.display.nebula_confirm,
                    Some(NebulaConfirm::ClosePane { pane_id, .. }) if pane_id == id
                );
                if confirmed {
                    self.display.nebula_confirm = None;
                } else if let Some(process) = self.busy_process_in(&[id]) {
                    self.display.nebula_confirm =
                        Some(NebulaConfirm::ClosePane { pane_id: id, process });
                    self.dirty = true;
                    return false;
                }
                self.close_focused_pane()
            },
            TabRequest::CloseIndex(index) => {
                let confirmed = matches!(
                    self.display.nebula_confirm,
                    Some(NebulaConfirm::CloseTab { index: i, .. }) if i == index
                );
                if confirmed {
                    self.display.nebula_confirm = None;
                } else {
                    let mut ids = Vec::new();
                    if let Some(tab) = self.tabs.get(index) {
                        tab.layout.leaves(&mut ids);
                    }
                    if let Some(process) = self.busy_process_in(&ids) {
                        self.display.nebula_confirm =
                            Some(NebulaConfirm::CloseTab { index, process });
                        self.dirty = true;
                        return false;
                    }
                }
                self.close_tab(index)
            },
            TabRequest::Duplicate(index) => {
                self.duplicate_tab(index);
                false
            },
            TabRequest::ForkAiSession(index) => {
                self.fork_ai_session(index);
                false
            },
            TabRequest::ExportWorkspace => {
                self.export_workspace(None);
                false
            },
            TabRequest::ExportTab(index) => {
                self.export_workspace(Some(index));
                false
            },
            TabRequest::ImportWorkspace => {
                self.import_workspace();
                false
            },
            TabRequest::CloseWindow => {
                // A normal window close DETACHES: the PTYs live on in the
                // resident process, so a running claude/build is not lost
                // and needs no confirmation. When the close actually KILLS
                // the shells — the quick terminal (session_exempt), or the
                // user turned residency off in 设置→高级 — a busy process
                // (claude, a build…) gets the confirm dialog first.
                if self.session_exempt || !self.display.nebula_keep_session {
                    let confirmed = matches!(
                        self.display.nebula_confirm,
                        Some(NebulaConfirm::CloseWindow { .. })
                    );
                    if confirmed {
                        self.display.nebula_confirm = None;
                    } else {
                        let ids: Vec<_> = self.panes.iter().map(|p| p.id).collect();
                        if let Some(process) = self.busy_process_in(&ids) {
                            self.display.nebula_confirm =
                                Some(NebulaConfirm::CloseWindow { process });
                            self.dirty = true;
                            return false;
                        }
                    }
                }
                self.display.window.hold = false;
                true
            },
            TabRequest::SelectNext => {
                if !self.tabs.is_empty() {
                    self.select_tab((self.active_tab + 1) % self.tabs.len());
                }
                false
            },
            TabRequest::SelectPrev => {
                if !self.tabs.is_empty() {
                    let n = self.tabs.len();
                    self.select_tab((self.active_tab + n - 1) % n);
                }
                false
            },
            TabRequest::Select(index) => {
                self.select_tab(index);
                false
            },
            TabRequest::SelectLast => {
                if !self.tabs.is_empty() {
                    self.select_tab(self.tabs.len() - 1);
                }
                false
            },
            TabRequest::Move { from, to } => {
                self.move_tab(from, to);
                false
            },
            TabRequest::SplitToggle(direction) => {
                self.split_focused(direction);
                false
            },
            TabRequest::SplitIndex { index, direction } => {
                if self
                    .tabs
                    .get(index)
                    .is_some_and(|tab| tab.doc.is_none() && tab.image.is_none() && !tab.settings)
                {
                    self.select_tab(index);
                    self.split_focused(direction);
                }
                false
            },
            TabRequest::DockSplit { source, nav } => {
                self.dock_tab_into_active(source, nav);
                false
            },
            TabRequest::FocusSplit(nav) => {
                self.focus_split(nav);
                false
            },
            TabRequest::ToggleZoom => {
                self.toggle_zoom();
                false
            },
            TabRequest::BeginRename(index) => {
                if index < self.tabs.len() {
                    // Start editing: grab the current label (either custom name or cwd-derived)
                    let current_label = if let Some(custom) = &self.tabs[index].custom_name {
                        custom.clone()
                    } else {
                        self.pane(self.tabs[index].active_pane)
                            .map(Self::chrome_tab_label)
                            .unwrap_or_else(|| "Tab".to_owned())
                    };
                    self.display.nebula_tab_rename_caret = current_label.chars().count();
                    self.display.nebula_tab_rename = Some((index, current_label));
                    self.display.nebula_tab_rename_select_all = true;
                    self.dirty = true;
                }
                false
            },
            TabRequest::CommitRename(new_name) => {
                self.display.nebula_tab_rename_select_all = false;
                if let Some((index, _)) = self.display.nebula_tab_rename.take() {
                    if index < self.tabs.len() {
                        let trimmed = new_name.trim().to_owned();
                        self.tabs[index].custom_name = if trimmed.is_empty() {
                            None // Empty name reverts to auto-label
                        } else {
                            Some(trimmed)
                        };
                        self.sync_chrome_tabs();
                        self.dirty = true;
                    }
                }
                false
            },
            TabRequest::SetColor { index, color } => {
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.custom_color = color;
                    self.sync_chrome_tabs();
                    self.mark_session_dirty();
                    self.dirty = true;
                }
                false
            },
            TabRequest::CancelRename => {
                self.display.nebula_tab_rename_select_all = false;
                if self.display.nebula_tab_rename.take().is_some() {
                    self.dirty = true;
                }
                false
            },
        }
    }

    /// Number of tabs in this window.
    #[inline]
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Index of the active tab.
    #[inline]
    pub fn active_tab_index(&self) -> usize {
        self.active_tab
    }

    /// 把一个新标签实体加入标签顺序并激活它，返回它的落点。
    ///
    /// 这是标签实体进入顺序的唯一入口（拖拽重排的 `move_tab` 除外——那里的
    /// 落点由用户手势直接给出）。`placement` 由调用方声明意图：真正创建标签
    /// 传 [`TabPlacement::Created`]，会话恢复与工作区导入传
    /// [`TabPlacement::AfterActive`]。
    pub(super) fn insert_tab(&mut self, entry: TabEntry, placement: TabPlacement) -> usize {
        let at = tab_insert_index(
            placement,
            self.display.nebula_new_tab_position,
            self.active_tab,
            self.tabs.len(),
        );
        self.tabs.insert(at, entry);
        self.active_tab = at;
        at
    }

    /// Spawn and activate a new tab (a single-pane layout) using the default shell.
    fn spawn_tab(&mut self) {
        self.spawn_tab_at(
            self.display.startup_directory().or_else(|| self.focused_cwd()),
            TabPlacement::Created,
        );
    }

    /// Spawn a default-shell tab at an already validated explicit directory,
    /// or inherit the caller-provided cwd. Keeping this as the only insertion
    /// path guarantees tree-created terminals behave exactly like Ctrl+Shift+T.
    pub(super) fn spawn_tab_at(&mut self, cwd: Option<std::path::PathBuf>, placement: TabPlacement) {
        // The default-shell setting (`shell=<id>` in nebula_settings.txt) may
        // name a detected shell the PTY layer doesn't bootstrap itself (cmd,
        // pwsh, nushell, a WSL distro). `resolve_id` returns `None` for the two
        // PTY-integrated executors (powershell/bash) so those keep their prompt
        // injection; anything else spawns as an explicit override here.
        let override_shell = Self::default_shell_override(&self.config);
        let spawned = match override_shell {
            Some(shell) => self.spawn_pane_detached_with(cwd, self.display.size_info, Some(shell)),
            None => self.spawn_pane_detached(cwd, self.display.size_info),
        };
        if let Some(id) = spawned {
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
                placement,
            );
            self.resize_active_layout();
            self.dirty = true;
            self.run_fastfetch_intro(id);
        }
    }

    /// The default-shell override for a plain new tab, or `None` to use the PTY
    /// layer's own default (which owns the powershell/bash prompt bootstrap).
    pub(super) fn default_shell_override(
        config: &crate::config::ui_config::UiConfig,
    ) -> Option<nebula_terminal::tty::Shell> {
        let id = crate::display::nebula_settings_value("shell")
            .or_else(|| crate::display::nebula_settings_value("executor"))?;
        if let Some(profile) = config
            .profiles
            .iter()
            .find(|profile| profile.settings_id().as_deref() == Some(id.as_str()))
        {
            return Some(profile.shell());
        }
        crate::shell_detect::resolve_id(&id).map(|shell| shell.shell())
    }

    /// Open a new tab running the quick-launch profile at `index` (custom
    /// command instead of the default shell). The tab is pre-named after the
    /// profile so an `ssh host` entry reads as its destination, not "ssh".
    fn spawn_tab_profile(&mut self, index: usize) {
        let Some(profile) = self.config.profiles.get(index).cloned() else { return };
        self.spawn_tab_profile_value(profile, TabPlacement::Created);
    }

    pub(super) fn spawn_tab_profile_value(&mut self, profile: Profile, placement: TabPlacement) {
        // Profile cwd wins when it exists; else inherit the focused pane's.
        let cwd = preferred_tab_cwd(
            profile.cwd.as_ref().filter(|p| p.is_dir()).cloned(),
            self.display.startup_directory(),
            self.focused_cwd(),
        );
        let shell = profile.shell();
        if let Some(id) = self.spawn_pane_detached_with(cwd, self.display.size_info, Some(shell)) {
            self.insert_tab(
                TabEntry {
                    layout: Layout::Leaf(id),
                    active_pane: id,
                    has_bell: false,
                    custom_name: Some(profile.name.clone()),
                    custom_color: None,
                    launch: TabLaunch::Profile(profile),
                    doc: None,
                    image: None,
                    settings: false,
                },
                placement,
            );
            self.resize_active_layout();
            self.dirty = true;
        }
    }

    /// Open a new tab running a detected shell (the new-tab dropdown). Like
    /// `spawn_tab_profile` but the spec is passed in rather than looked up in
    /// the config, and the cwd inherits the focused pane's.
    pub(super) fn spawn_tab_shell(
        &mut self,
        name: String,
        shell: nebula_terminal::tty::Shell,
        placement: TabPlacement,
    ) {
        if let Some(id) = self.spawn_pane_detached_with(
            self.display.startup_directory().or_else(|| self.focused_cwd()),
            self.display.size_info,
            Some(shell.clone()),
        ) {
            self.insert_tab(
                TabEntry {
                    layout: Layout::Leaf(id),
                    active_pane: id,
                    has_bell: false,
                    custom_name: Some(name.clone()),
                    custom_color: None,
                    launch: TabLaunch::Shell { name, shell },
                    doc: None,
                    image: None,
                    settings: false,
                },
                placement,
            );
            self.resize_active_layout();
            self.dirty = true;
        }
    }

    /// Open a saved SSH destination inside the configured default shell.
    /// `nebula ssh` is typed into that shell's PTY so OpenSSH remains inside
    /// Nebula's ConPTY instead of becoming the pane's GUI-subsystem root.
    pub(super) fn spawn_tab_ssh(&mut self, host: String, placement: TabPlacement) {
        self.spawn_tab_ssh_at(host, None, placement);
    }

    pub(super) fn spawn_tab_ssh_at(
        &mut self,
        host: String,
        remote_cwd: Option<String>,
        placement: TabPlacement,
    ) {
        #[cfg(windows)]
        {
            let pane_id = self.next_pane_id;
            match Self::create_ssh_pane(
                &self.display.size_info,
                self.display.window.id(),
                &self.config,
                &self.proxy,
                pane_id,
                host.clone(),
                remote_cwd,
            ) {
                Ok(pane) => {
                    self.next_pane_id += 1;
                    self.panes.push(pane);
                    self.insert_tab(
                        TabEntry {
                            layout: Layout::Leaf(pane_id),
                            active_pane: pane_id,
                            has_bell: false,
                            custom_name: Some(host.clone()),
                            custom_color: None,
                            launch: TabLaunch::Ssh(host),
                            doc: None,
                            image: None,
                            settings: false,
                        },
                        placement,
                    );
                    self.resize_active_layout();
                    self.dirty = true;
                    return;
                },
                Err(err) => {
                    error!("创建直连 SSH Pane 失败: {err}");
                    let user_error = crate::ux::UserFacingError::new(
                        format!("SSH {host} 连接创建失败"),
                        "无法创建 SSH 会话，地址、认证方式或本机 SSH 配置可能无效。",
                        "检查主机地址和认证配置，右键编辑该主机后重试。",
                    )
                    .retry(crate::ux::RetryAction::Retry)
                    .details(err.to_string());
                    self.message_buffer.push(crate::message_bar::Message::user_error(&user_error));
                    self.dirty = true;
                    self.display.window.request_redraw();
                    return;
                },
            }
        }

        #[cfg(not(windows))]
        {
            let _ = remote_cwd;
            let Ok(exe) = std::env::current_exe() else {
                error!("Cannot locate the Pebrel executable for the SSH AskPass helper");
                return;
            };
            let shell_id = self.display.nebula_shell_id.clone().unwrap_or_else(|| {
                match self.display.nebula_shell {
                    crate::display::NebulaShell::PowerShell => "powershell".into(),
                    crate::display::NebulaShell::Bash => "bash".into(),
                }
            });
            let launch = match crate::ssh::build_pane_launch(&shell_id, &exe, &host) {
                Ok(launch) => launch,
                Err(err) => {
                    error!("Refusing unsafe SSH destination {host:?}: {err}");
                    return;
                },
            };
            let default_shell = Self::default_shell_override(&self.config);
            if let Some(id) = self.spawn_pane_detached_with(
                self.focused_cwd(),
                self.display.size_info,
                default_shell,
            ) {
                self.insert_tab(
                    TabEntry {
                        layout: Layout::Leaf(id),
                        active_pane: id,
                        has_bell: false,
                        custom_name: Some(host.clone()),
                        custom_color: None,
                        launch: TabLaunch::Ssh(host),
                        doc: None,
                        image: None,
                        settings: false,
                    },
                    placement,
                );
                self.resize_active_layout();
                self.dirty = true;
                if let Some(pane) = self.panes.iter().find(|pane| pane.id == id) {
                    pane.notifier.notify(launch.command);
                }
            }
        }
    }

    /// 用新的 pane id 在当前布局叶原位重建失败的 SSH 会话。先成功创建替代
    /// 会话、再一次性换树和 pane 池，避免"关闭最后一个 tab 后再新建"导致
    /// 窗口提前退出；新 id 也会隔离旧 runtime 的迟到阶段事件。
    pub(super) fn retry_focused_ssh(&mut self, destination: String) {
        #[cfg(windows)]
        {
            let old_id = self.focused_pane_id();
            let Some(old_index) = self.pane_index(old_id) else { return };
            if self.panes[old_index].ssh_destination.as_deref() != Some(destination.as_str()) {
                return;
            }
            let view = self
                .layout_geometry(false)
                .0
                .into_iter()
                .find_map(|(id, view)| (id == old_id).then_some(view))
                .unwrap_or(self.display.size_info);
            let new_id = self.next_pane_id;
            let new_pane = match Self::create_ssh_pane(
                &view,
                self.display.window.id(),
                &self.config,
                &self.proxy,
                new_id,
                destination.clone(),
                None,
            ) {
                Ok(pane) => pane,
                Err(error) => {
                    self.display.ssh_connect_stage(
                        old_id,
                        destination,
                        crate::ssh_session::SshStage::Failed(format!("无法重试 SSH 连接: {error}")),
                    );
                    self.dirty = true;
                    self.display.window.request_redraw();
                    return;
                },
            };

            let tab = &mut self.tabs[self.active_tab];
            if !tab.layout.replace_leaf(old_id, new_id) {
                let _ = new_pane.notifier.0.send(nebula_terminal::event_loop::Msg::Shutdown);
                return;
            }
            tab.active_pane = new_id;
            if self.zoom == Some(old_id) {
                self.zoom = Some(new_id);
            }
            self.next_pane_id = self.next_pane_id.saturating_add(1);
            let old_pane = std::mem::replace(&mut self.panes[old_index], new_pane);
            self.display.forget_ssh_connect(old_id);
            self.display.ssh_connect_stage(
                new_id,
                destination,
                crate::ssh_session::SshStage::Resolve,
            );
            let _ = old_pane.notifier.0.send(nebula_terminal::event_loop::Msg::Shutdown);
            self.resize_active_layout();
            self.dirty = true;
            self.display.window.request_redraw();
        }
        #[cfg(not(windows))]
        let _ = destination;
    }

    /// Open `path` in a read-only document viewer tab. A tab already viewing
    /// this file is re-focused (and re-read, so the view is fresh) instead of
    /// duplicated — double-click twice shouldn't litter the bar.
    pub(super) fn open_doc_tab(&mut self, path: std::path::PathBuf) {
        if let Some(index) =
            self.tabs.iter().position(|tab| tab.doc.as_ref().is_some_and(|doc| doc.path == path))
        {
            if let Some(doc) = self.tabs[index].doc.as_mut() {
                doc.reload();
            }
            self.select_tab(index);
            self.dirty = true;
            return;
        }
        let doc = crate::display::markdown_view::DocView::open(path);
        // Nerd Fot markdown mark in front of the file name; the label IS the
        // tab identity for doc tabs (no cwd to derive one from). Same codicon
        // as the file tree's markdown icon, so tab and tree read as one system.
        let label = format!("\u{eb1d} {}", doc.title);
        self.insert_tab(
            TabEntry {
                layout: Layout::Leaf(DOC_PANE_ID),
                active_pane: DOC_PANE_ID,
                has_bell: false,
                custom_name: Some(label),
                custom_color: None,
                launch: TabLaunch::Document(doc.path.clone()),
                doc: Some(doc),
                image: None,
                settings: false,
            },
            TabPlacement::Created,
        );
        self.display.set_special_tab_active(true);
        self.display.set_settings_tab_active(false);
        self.dirty = true;
    }

    pub(super) fn open_image_tab(&mut self, path: std::path::PathBuf) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.image.as_ref().is_some_and(|image| image.path == path))
        {
            if let Some(image) = self.tabs[index].image.as_mut() {
                image.reload();
            }
            self.select_tab(index);
            self.dirty = true;
            return;
        }
        let image = crate::display::image_viewer::ImageView::open(path.clone());
        self.insert_tab(
            TabEntry {
                layout: Layout::Leaf(DOC_PANE_ID),
                active_pane: DOC_PANE_ID,
                has_bell: false,
                custom_name: Some(format!(
                    "{} {}",
                    crate::display::side_panel::file_type_icon(&image.title),
                    image.title
                )),
                custom_color: None,
                launch: TabLaunch::Image(path),
                doc: None,
                image: Some(image),
                settings: false,
            },
            TabPlacement::Created,
        );
        self.display.set_special_tab_active(true);
        self.display.set_settings_tab_active(false);
        self.dirty = true;
    }

    /// Open Settings as a real singleton tab. Re-focusing the existing tab
    /// avoids duplicate preference surfaces and never starts a shell process.
    pub(super) fn open_settings_tab(&mut self) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.settings) {
            self.select_tab(index);
            self.display.set_settings_tab_active(true);
            self.dirty = true;
            return;
        }

        self.insert_tab(
            TabEntry {
                layout: Layout::Leaf(DOC_PANE_ID),
                active_pane: DOC_PANE_ID,
                has_bell: false,
                custom_name: Some("\u{eb51} 设置".to_owned()),
                custom_color: None,
                launch: TabLaunch::Settings,
                doc: None,
                image: None,
                settings: true,
            },
            TabPlacement::Created,
        );
        self.display.set_settings_tab_active(true);
        self.sync_chrome_tabs();
        self.dirty = true;
    }

    /// Switch the active tab, resizing its panes to the current window.
    pub(super) fn select_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        self.active_tab = index;
        self.tabs[index].has_bell = false;
        self.display.set_special_tab_active(
            self.tabs[index].doc.is_some()
                || self.tabs[index].image.is_some()
                || self.tabs[index].settings,
        );
        self.display.set_settings_tab_active(self.tabs[index].settings);
        self.zoom = None;
        self.resize_active_layout();
        self.dirty = true;
    }

    /// Close the pane whose shell produced an `Exit` event, or the focused pane
    /// when `pane_id` is `None`. Returns `true` if the last tab closed (the
    /// window should close).
    pub fn close_tab_by_id(&mut self, pane_id: Option<u64>) -> bool {
        let id = pane_id.unwrap_or_else(|| self.focused_pane_id());
        self.close_pane(id)
    }

    /// Close an entire tab (all of its panes). Returns `true` if it was the last
    /// tab (the window should close).
    pub(super) fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }

        let entry = self.tabs.remove(index);
        let mut ids = Vec::new();
        entry.layout.leaves(&mut ids);
        for id in ids {
            // 连接中的 tab 被关掉：丢弃它的卡片状态，否则 HashMap 会随
            // 会话累积，而那个 pane 再也不会回来。
            self.display.forget_ssh_connect(id);
            if let Some(i) = self.pane_index(id) {
                let pane = self.panes.remove(i);
                let _ = pane.notifier.0.send(Msg::Shutdown);
            }
        }

        if self.tabs.is_empty() {
            return true;
        }

        if self.active_tab > index {
            self.active_tab -= 1;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        let special = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.doc.is_some() || tab.image.is_some() || tab.settings);
        self.display.set_special_tab_active(special);
        self.display.set_settings_tab_active(
            self.tabs.get(self.active_tab).is_some_and(|tab| tab.settings),
        );
        self.resize_active_layout();
        self.dirty = true;
        false
    }
}