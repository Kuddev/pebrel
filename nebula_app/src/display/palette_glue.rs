//! Command-palette glue: toggling and opening the palette, the AI-session
//! and shell menus, the directory picker, and the palette input/scroll/
//! click/hover/confirm plumbing that drives `command_palette`.

use super::command_palette;

use super::Display;

impl Display {
    /// Toggle the command palette (Ctrl+Shift+P). `profiles` are the config's
    /// quick-launch profile names, refreshed on every open so live config
    /// reloads are reflected.
    pub fn toggle_command_palette(&mut self, profiles: &[crate::config::ui_config::Profile]) {
        self.nebula_palette.set_profiles(profiles);
        // 打开这一刻取一次窗口状态：「工作目录」组作用在哪个目录上、两个
        // 开关命令各自的勾选态。取样而不是每帧回读，见 `PaletteContext`。
        self.nebula_palette.set_context(command_palette::PaletteContext {
            cwd: self.nebula_focused_cwd.clone(),
            sidebar: !self.nebula_sidebar_collapsed,
            panel_resize: self.nebula_panel_resize,
            new_tab_inherits_cwd: self.startup_directory().is_none(),
        });
        self.nebula_palette.toggle();
        self.pending_update.dirty = true;
    }

    /// 在系统文件管理器里打开聚焦 pane 的工作目录。目录未知时什么也不做
    /// ——命令面板里那条命令此时根本不出现，这里只是兜底。
    pub fn reveal_focused_cwd(&mut self) {
        let Some(path) = self.nebula_focused_cwd.clone() else { return };
        #[cfg(windows)]
        let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&path).spawn();
        #[cfg(all(not(windows), not(target_os = "macos")))]
        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
        self.pending_update.dirty = true;
    }

    /// 聚焦 pane 的工作目录，给「复制路径」用（剪贴板在输入层，不在这里）。
    pub fn focused_cwd_string(&self) -> Option<String> {
        self.nebula_focused_cwd.as_ref().map(|path| path.display().to_string())
    }

    /// 默认 shell 的短标（settings 覆盖优先），给 `TabLaunch::Default` 的行用。
    pub fn default_shell_tag(&self) -> String {
        let id =
            self.nebula_shell_id.as_deref().unwrap_or_else(|| self.nebula_shell.settings_value());
        crate::shell_detect::shell_short_tag(id)
    }

    /// 「恢复 AI 会话」面板：原生 Claude/Codex 档案与 Nebula hook 索引
    /// 合并去重；只展示已验证有 resume 语法的来源。
    pub fn open_ai_session_palette(&mut self) {
        let mut rows = Vec::new();
        for session in crate::ai_sessions::scan(30) {
            // 右列 = 「位置 · 相对时间」。来源不再挤进这段文字——行首
            // 品牌 logo + 右缘 chip 已经把 claude/codex 标满了。
            let time = crate::ai_sessions::relative_label(session.modified);
            let place = session.place_label();
            let hint = if place.is_empty() { time } else { format!("{place} · {time}") };
            let search =
                format!("{} {} {}", session.title, session.project, session.source.label());
            let Some(resume) = session.resume_command() else {
                continue;
            };
            rows.push(command_palette::AiSessionRow {
                label: session.title.clone(),
                hint: hint.clone(),
                search: format!("恢复 resume {search}"),
                command: resume,
                source: session.source,
            });
            if let Some(command) = session.fork_command() {
                rows.push(command_palette::AiSessionRow {
                    label: format!("分叉 · {}", session.title),
                    hint,
                    search: format!("分叉 fork {search}"),
                    command,
                    source: session.source,
                });
            }
        }
        self.nebula_palette.open_ai_sessions(rows);
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// Open the new-tab dropdown: detected shells (installed-shell order) plus
    /// any config profiles. Detection runs once and is cached — the chevron
    /// beside the "+" opens this — the familiar profile menu.
    pub fn open_shell_menu(&mut self, profiles: &[crate::config::ui_config::Profile]) {
        let ssh_rows: Vec<(String, String, String)> = self
            .nebula_ssh_hosts
            .iter()
            .map(|host| {
                let label =
                    self.nebula_ssh_labels.get(host).cloned().unwrap_or_else(|| host.clone());
                let icon = self
                    .nebula_ssh_icons
                    .get(host)
                    .cloned()
                    .unwrap_or_else(|| crate::display::ui::os_icons::DEFAULT_ID.to_owned());
                (label, host.clone(), icon)
            })
            .collect();
        let shells =
            self.nebula_detected_shells.get_or_insert_with(crate::shell_detect::detect_shells);
        let default_shell =
            self.nebula_shell_id.as_deref().unwrap_or_else(|| self.nebula_shell.settings_value());
        self.nebula_palette.set_shell_menu(shells, profiles, default_shell);
        self.nebula_palette.set_ssh_hosts_with_icons(&ssh_rows);
        self.nebula_palette.open_profiles();
        self.pending_update.dirty = true;
    }

    /// Ctrl+K：开/关 shell picker——与 "+" 旁 chevron 打开的是同一份列表
    /// （settings 页的 shell 下拉是另一回事，见 `toggle_shell_picker`）。
    /// 已开着的 shell picker 再按一次收起；其他 palette 模式则切换过来。
    pub fn toggle_shell_menu(&mut self, profiles: &[crate::config::ui_config::Profile]) {
        let picker_open = self.nebula_palette.is_picker()
            && !self.nebula_palette.is_picking_default()
            && !self.nebula_palette.is_picking_directory();
        if picker_open {
            self.nebula_palette.close();
            self.pending_update.dirty = true;
        } else {
            self.open_shell_menu(profiles);
        }
    }

    /// Open a terminal-directory picker backed by the same frecency model as
    /// ghost text and filesystem completion. No shell command is installed.
    pub fn open_directory_picker(&mut self) {
        let paths = self.directory_history.search("", 128);
        self.nebula_palette.set_directories(paths);
        self.nebula_palette.open_directories();
        self.pending_update.dirty = true;
    }

    fn refresh_directory_picker(&mut self) {
        if !self.nebula_palette.is_picking_directory() {
            return;
        }
        let query = self.nebula_palette.query().to_owned();
        let paths = self.directory_history.search(&query, 128);
        self.nebula_palette.set_directories(paths);
    }

    pub fn command_palette_open(&self) -> bool {
        self.nebula_palette.is_open()
    }

    /// One geometry contract for palette rendering and pointer input. Picker
    /// height depends on the live filtered row count, so callers must not
    /// reconstruct this from window dimensions alone.
    pub(super) fn command_palette_workspace_bounds(&self) -> Option<(f32, f32)> {
        if !self.nebula_palette.is_open() {
            return None;
        }
        let scale = self.window.scale_factor as f32;
        let width = self.ui_size_info().width();
        // 快捷面板族只占默认终端工作区：左侧 Tabs 与右侧文件抽屉都保留。
        // 三个面板共用这条边界，切换快捷键时宽度与水平基准才不会跳变。
        let sidebar = (self.sidebar_w_visual() * scale).round();
        let left = (sidebar - 4.0 * scale).round().clamp(0.0, width);
        let drawer = (self.drawer_w_visual() * scale).min(width * 0.42);
        let right = (width - drawer - 8.0 * scale).round().clamp(left, width);
        (right > left).then_some((left, right))
    }

    pub fn command_palette_layout(&self) -> command_palette::PaletteLayout {
        let size = self.ui_size_info();
        command_palette::palette_layout_with_workspace_bounds(
            &self.nebula_palette,
            size.width(),
            size.height(),
            self.window.scale_factor as f32,
            size.cell_width(),
            self.nebula_density,
            self.command_palette_workspace_bounds(),
        )
    }

    pub fn command_palette_picking_default(&self) -> bool {
        self.nebula_palette.is_picking_default()
    }

    pub fn command_palette_picker_open(&self) -> bool {
        self.nebula_palette.is_picker()
    }

    pub fn close_command_palette(&mut self) {
        self.nebula_palette.close();
        self.pending_update.dirty = true;
    }

    pub fn palette_input_char(&mut self, c: char) {
        self.nebula_palette.input_char(c);
        self.refresh_directory_picker();
        self.pending_update.dirty = true;
    }

    pub fn palette_input_text(&mut self, text: &str) {
        self.nebula_palette.input_text(text);
        self.refresh_directory_picker();
        self.pending_update.dirty = true;
    }

    pub fn palette_select_all(&mut self) {
        self.nebula_palette.select_all();
        self.pending_update.dirty = true;
    }

    pub fn palette_selected_text(&self) -> Option<String> {
        self.nebula_palette.selected_text()
    }

    pub fn palette_backspace(&mut self) {
        self.nebula_palette.backspace();
        self.refresh_directory_picker();
        self.pending_update.dirty = true;
    }

    pub fn palette_move(&mut self, delta: i32) {
        let max_rows = self.command_palette_layout().max_rows;
        self.nebula_palette.move_selection(delta, max_rows);
        self.pending_update.dirty = true;
    }

    pub fn palette_tab(&mut self, delta: i32) {
        if !self.nebula_palette.cycle_launcher_filter(delta) {
            self.palette_move(delta);
            return;
        }
        self.pending_update.dirty = true;
    }

    pub fn palette_select_launcher_filter(
        &mut self,
        filter: command_palette::LauncherFilter,
    ) -> bool {
        if self.nebula_palette.set_launcher_filter(filter) {
            self.pending_update.dirty = true;
            return true;
        }
        false
    }

    pub fn palette_scroll_by(&mut self, rows: i32, max_rows: usize) -> bool {
        if self.nebula_palette.scroll_by(rows, max_rows) {
            self.pending_update.dirty = true;
            return true;
        }
        false
    }

    pub fn palette_scrollbar_press(
        &mut self,
        x: f32,
        y: f32,
        layout: &command_palette::PaletteLayout,
    ) -> bool {
        let Some(scrollbar) = layout.scrollbar else { return false };
        if self.nebula_palette.scrollbar_press(x, y, layout.max_rows, scrollbar) {
            self.pending_update.dirty = true;
            return true;
        }
        false
    }

    pub fn palette_scrollbar_dragging(&self) -> bool {
        self.nebula_palette.scrollbar_dragging()
    }

    pub fn palette_scrollbar_drag_to(&mut self, y: f32) -> bool {
        let layout = self.command_palette_layout();
        let Some(scrollbar) = layout.scrollbar else { return false };
        if self.nebula_palette.scrollbar_drag_to(y, layout.max_rows, scrollbar) {
            self.pending_update.dirty = true;
            return true;
        }
        false
    }

    pub fn end_palette_scrollbar_drag(&mut self) -> bool {
        self.nebula_palette.end_scrollbar_drag()
    }

    /// Confirm the palette selection; returns the action for the input layer to
    /// dispatch (only it can reach both the display and the window context).
    pub fn palette_confirm(&mut self) -> Option<command_palette::PaletteAction> {
        let action = self.nebula_palette.confirm();
        self.pending_update.dirty = true;
        action
    }

    /// Mouse click on the palette's visible row `row` (0 = topmost visible):
    /// select and confirm it, returning the action to dispatch.
    pub fn palette_click(
        &mut self,
        row: usize,
        max_rows: usize,
    ) -> Option<command_palette::PaletteAction> {
        let action = self.nebula_palette.click(row, max_rows);
        self.pending_update.dirty = true;
        action
    }

    /// Update palette hover state. `row` is the visual row index, or `None` when
    /// the mouse left the palette area.
    pub fn palette_hover(
        &mut self,
        pos: (f32, f32),
        row: Option<usize>,
        chip: Option<command_palette::LauncherFilter>,
    ) -> bool {
        if self.nebula_palette.pointer_hover(pos, row, chip) {
            self.pending_update.dirty = true;
            return true;
        }
        false
    }
}
