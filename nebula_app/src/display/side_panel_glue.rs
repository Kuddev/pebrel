//! Side-panel glue: the right-side drawer (directory tree / git status)
//! toggle, tab-rename caret editing, panel routing/layout/sync, and the SFTP
//! panel open/close/hit/click/upload/download/rename/delete plumbing.

use unicode_width::UnicodeWidthChar;
use winit::dpi::PhysicalSize;

use super::NebulaConfirm;
use super::chrome_reserve;
use super::{file_dialog, sftp_panel, side_panel};

use super::Display;

impl Display {
    /// Toggle the right-side drawer (directory tree / git status).
    ///
    /// Special tabs (settings / document / image) own the whole content area,
    /// so the drawer stays shut there. Guarding here rather than at each of the
    /// callers — sidebar button, panel header, keybinding, command palette —
    /// is what keeps a newly added entry point from reintroducing the squeeze.
    pub fn toggle_side_panel(&mut self, view: side_panel::PanelView) {
        if self.nebula_special_tab_active {
            return;
        }
        let was_open = self.nebula_side_panel.open;
        if self.nebula_sftp_panel.take().is_some() {
            self.nebula_side_panel.open = true;
            self.nebula_side_panel.view = view;
        } else {
            self.nebula_side_panel.toggle(view);
        }
        // The drawer reserves real grid width, so opening/closing it (not
        // just switching views) must reflow the grid like the left sidebar.
        if self.nebula_side_panel.open != was_open {
            let size =
                PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
            self.pending_update.set_dimensions(size);
        }
        self.window.request_redraw();
        self.pending_update.dirty = true;
    }

    // ---- tab rename caret editing (the rename box is a real text field) ----

    /// Insert `text` at the caret. A pending select-all is replaced wholesale
    /// (type-to-overwrite), matching every native text field.
    pub fn tab_rename_insert(&mut self, text: &str) {
        let text: String = text.chars().filter(|character| !character.is_control()).collect();
        if text.is_empty() {
            return;
        }
        let select_all = self.nebula_tab_rename_select_all;
        let caret = self.nebula_tab_rename_caret;
        let Some((_, buf)) = self.nebula_tab_rename.as_mut() else { return };
        if select_all {
            buf.clear();
            self.nebula_tab_rename_select_all = false;
            self.nebula_tab_rename_caret = 0;
        }
        let caret = if select_all { 0 } else { caret.min(buf.chars().count()) };
        let byte = buf.char_indices().nth(caret).map(|(b, _)| b).unwrap_or(buf.len());
        buf.insert_str(byte, &text);
        self.nebula_tab_rename_caret = caret + text.chars().count();
        self.pending_update.dirty = true;
    }

    /// Backspace at the caret; a pending select-all clears the whole name.
    pub fn tab_rename_backspace(&mut self) {
        let select_all = self.nebula_tab_rename_select_all;
        let caret = self.nebula_tab_rename_caret;
        let Some((_, buf)) = self.nebula_tab_rename.as_mut() else { return };
        if select_all {
            buf.clear();
            self.nebula_tab_rename_select_all = false;
            self.nebula_tab_rename_caret = 0;
        } else if caret > 0 {
            let caret = caret.min(buf.chars().count());
            if let Some((byte, _)) = buf.char_indices().nth(caret - 1) {
                buf.remove(byte);
                self.nebula_tab_rename_caret = caret - 1;
            }
        }
        self.pending_update.dirty = true;
    }

    pub fn tab_rename_select_all(&mut self) {
        if let Some((_, text)) = self.nebula_tab_rename.as_ref() {
            self.nebula_tab_rename_select_all = !text.is_empty();
            self.nebula_tab_rename_caret = text.chars().count();
            self.pending_update.dirty = true;
        }
    }

    pub fn tab_rename_selected_text(&self) -> Option<String> {
        self.nebula_tab_rename_select_all
            .then(|| self.nebula_tab_rename.as_ref().map(|(_, text)| text.clone()))
            .flatten()
    }

    /// Move the caret by `delta` chars. A select-all collapses to the matching
    /// end first (left → start, right → end) without moving further.
    pub fn tab_rename_move_caret(&mut self, delta: i32) {
        let Some((_, buf)) = self.nebula_tab_rename.as_ref() else { return };
        let len = buf.chars().count();
        if self.nebula_tab_rename_select_all {
            self.nebula_tab_rename_select_all = false;
            self.nebula_tab_rename_caret = if delta < 0 { 0 } else { len };
        } else {
            let caret = self.nebula_tab_rename_caret.min(len) as i64 + delta as i64;
            self.nebula_tab_rename_caret = caret.clamp(0, len as i64) as usize;
        }
        self.pending_update.dirty = true;
    }

    /// Jump the caret to the start/end (Home/End).
    pub fn tab_rename_caret_edge(&mut self, end: bool) {
        let Some((_, buf)) = self.nebula_tab_rename.as_ref() else { return };
        self.nebula_tab_rename_select_all = false;
        self.nebula_tab_rename_caret = if end { buf.chars().count() } else { 0 };
        self.pending_update.dirty = true;
    }

    /// Place the caret from a pointer press at window-space `x`: map the
    /// pixel offset from the buffer's first glyph (stashed by `draw_chrome`)
    /// into a char index, honoring CJK double-width glyphs. This is what lets
    /// users click where they want to edit instead of retyping the name.
    pub fn tab_rename_click(&mut self, x: f32) {
        let text_x = self.nebula_tab_rename_text_x;
        let cell_w = self.size_info.cell_width();
        let Some((_, buf)) = self.nebula_tab_rename.as_ref() else { return };
        let mut col = ((x - text_x) / cell_w).round().max(0.0) as usize;
        let mut caret = 0usize;
        for c in buf.chars() {
            let w = c.width().unwrap_or(0).max(1);
            if col < w {
                break;
            }
            col -= w;
            caret += 1;
        }
        self.nebula_tab_rename_select_all = false;
        self.nebula_tab_rename_caret = caret;
        self.pending_update.dirty = true;
    }

    /// Adopt the focused pane's cwd into the drawer (per drawn frame; cheap
    /// no-op unless the drawer is open and something changed).
    pub fn side_panel_sync(&mut self, cwd: Option<std::path::PathBuf>) {
        if self.sftp_view_active() {
            return;
        }
        if self.nebula_side_panel.sync(cwd) {
            self.pending_update.dirty = true;
        }
    }

    /// Whether the drawer currently shows the SFTP view. The SFTP panel is a
    /// window-level object, but its VIEW follows the focused pane: focusing a
    /// local tab flips the drawer back to the directory tree (the SFTP
    /// connection stays warm for the next switch), so the tree keeps
    /// following tab switches instead of being captured by one SSH session.
    pub(crate) fn sftp_view_active(&self) -> bool {
        self.nebula_sftp_panel.is_some() && self.nebula_sftp_routed
    }

    /// Re-route the drawer to the focused pane's identity, called every draw.
    /// `focused_ssh` is the pane's stable SSH destination, `None` for local
    /// panes.
    pub fn route_side_panel(&mut self, focused_ssh: Option<&str>) {
        let routed = match (self.nebula_sftp_panel.as_ref(), focused_ssh) {
            (Some(panel), Some(destination)) => panel.snapshot().destination == destination,
            _ => false,
        };
        if self.nebula_sftp_routed != routed {
            self.nebula_sftp_routed = routed;
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    pub fn choose_side_panel_directory(&mut self) {
        let Some(path) = file_dialog::pick_side_panel_directory(&self.window) else {
            return;
        };
        if self.nebula_side_panel.set_custom_root(path) {
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    pub fn follow_focused_directory(&mut self) {
        if self.nebula_side_panel.clear_custom_root() {
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    /// Geometry of the drawer for the current window size.
    pub fn side_panel_layout(&self) -> side_panel::PanelLayout {
        let size = self.size_info;
        let scale = self.window.scale_factor as f32;
        let reserve = chrome_reserve(scale);
        side_panel::panel_layout(
            size.width(),
            size.height(),
            reserve,
            reserve,
            scale,
            self.nebula_ui_anims.right_drawer.value(),
            self.drawer_w_visual(),
        )
    }

    pub fn open_sftp_panel(
        &mut self,
        destination: String,
        proxy: winit::event_loop::EventLoopProxy<crate::event::Event>,
    ) -> Result<(), String> {
        // Same content-area contract as `toggle_side_panel`: a special tab is
        // never the right place to raise the remote browser, and opening it
        // here would also strand the controller behind a hidden drawer.
        if self.nebula_special_tab_active {
            return Ok(());
        }
        let was_open = self.nebula_side_panel.open;
        // 控制器只拿一个"响一声"的闭包；把事件代理和窗口 id 捆进闭包是宿主的
        // 活儿，远端浏览器本身对消息循环一无所知。
        let window_id = self.window.id();
        let wake: crate::ssh_sftp::WakeFn = std::sync::Arc::new(move || {
            let _ = proxy.send_event(crate::event::Event::new(
                crate::event::EventType::SftpUpdated,
                window_id,
            ));
        });
        let controller = crate::ssh_sftp::SftpController::new(destination, wake)
            .map_err(|err| format!("无法打开 SFTP: {err}"))?;
        self.nebula_side_panel.search_unfocus(false);
        self.nebula_side_panel.commit_unfocus();
        self.nebula_side_panel.open = true;
        self.nebula_sftp_panel = Some(sftp_panel::SftpPanel::new(controller));
        if !was_open {
            let size =
                PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
            self.pending_update.set_dimensions(size);
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
        Ok(())
    }

    pub fn close_sftp_panel(&mut self) {
        if let Some(panel) = self.nebula_sftp_panel.take() {
            panel.cancel_transfer();
        }
        if self.nebula_side_panel.open {
            self.nebula_side_panel.open = false;
            let size =
                PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
            self.pending_update.set_dimensions(size);
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn sftp_layout(&self) -> sftp_panel::SftpLayout {
        sftp_panel::layout(&self.side_panel_layout(), self.window.scale_factor as f32)
    }

    pub fn sftp_hit(&self, x: f32, y: f32) -> sftp_panel::SftpHit {
        // A hidden SFTP view (drawer re-routed to a local pane) must not eat
        // clicks that belong to the directory tree drawn in its place.
        if !self.sftp_view_active() {
            return sftp_panel::SftpHit::None;
        }
        let Some(panel) = self.nebula_sftp_panel.as_ref() else {
            return sftp_panel::SftpHit::None;
        };
        let working = panel.snapshot().phase == crate::ssh_sftp::SftpPhase::Working;
        sftp_panel::hit_test(&self.sftp_layout(), working, x, y)
    }

    pub fn sftp_set_hover(&mut self, hit: sftp_panel::SftpHit) -> bool {
        self.nebula_sftp_panel.as_mut().is_some_and(|panel| panel.set_hover(hit))
    }

    pub fn sftp_click(&mut self, hit: sftp_panel::SftpHit) {
        use sftp_panel::SftpHit;
        match hit {
            SftpHit::Close => self.close_sftp_panel(),
            SftpHit::Path => {
                if let Some(panel) = self.nebula_sftp_panel.as_mut() {
                    panel.begin_path();
                }
            },
            SftpHit::Filter => {
                if let Some(panel) = self.nebula_sftp_panel.as_mut() {
                    panel.begin_filter();
                }
            },
            SftpHit::Row(index) => {
                let selected =
                    self.nebula_sftp_panel.as_mut().and_then(|panel| panel.select_row(index));
                if let Some((entry, true)) = selected {
                    let navigated =
                        self.nebula_sftp_panel.as_mut().is_some_and(|panel| panel.navigate(&entry));
                    if !navigated {
                        self.sftp_download_entry(entry);
                    }
                }
            },
            SftpHit::Cancel => {
                if let Some(panel) = self.nebula_sftp_panel.as_ref() {
                    panel.cancel_transfer();
                }
            },
            SftpHit::None | SftpHit::Inside => {},
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn sftp_refresh(&mut self) {
        if let Some(panel) = self.nebula_sftp_panel.as_ref() {
            panel.refresh();
        }
    }

    pub fn sftp_pick_upload_files(&mut self) {
        let paths = file_dialog::pick_upload_files(&self.window);
        if !paths.is_empty()
            && let Some(panel) = self.nebula_sftp_panel.as_ref()
        {
            panel.upload_paths(paths);
        }
    }

    pub fn sftp_pick_upload_directory(&mut self) {
        if let Some(path) = file_dialog::pick_upload_directory(&self.window)
            && let Some(panel) = self.nebula_sftp_panel.as_ref()
        {
            panel.upload_paths(vec![path]);
        }
    }

    pub fn sftp_begin_create_directory(&mut self) {
        if let Some(panel) = self.nebula_sftp_panel.as_mut() {
            panel.begin_create_directory();
        }
    }

    pub fn sftp_upload_dropped_paths(&mut self, paths: Vec<std::path::PathBuf>) -> bool {
        if paths.is_empty() {
            return false;
        }
        let Some(panel) = self.nebula_sftp_panel.as_ref() else { return false };
        panel.upload_paths(paths);
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    pub fn sftp_download_row(&mut self, index: usize) {
        if let Some(entry) =
            self.nebula_sftp_panel.as_ref().and_then(|panel| panel.visible_entry(index))
        {
            self.sftp_download_entry(entry);
        }
    }

    fn sftp_download_entry(&mut self, entry: crate::ssh_sftp::SftpEntry) {
        let Some(directory) = file_dialog::pick_download_directory(&self.window) else {
            return;
        };
        if let Some(panel) = self.nebula_sftp_panel.as_ref() {
            panel.download(entry, directory);
        }
    }

    pub fn sftp_begin_rename_row(&mut self, index: usize) {
        let entry = self.nebula_sftp_panel.as_ref().and_then(|panel| panel.visible_entry(index));
        if let (Some(panel), Some(entry)) = (self.nebula_sftp_panel.as_mut(), entry) {
            panel.begin_rename(entry);
        }
    }

    pub fn sftp_request_delete_row(&mut self, index: usize) {
        if let Some(entry) =
            self.nebula_sftp_panel.as_ref().and_then(|panel| panel.visible_entry(index))
        {
            self.nebula_confirm = Some(NebulaConfirm::DeleteSftp { entry });
        }
    }

    pub fn sftp_confirm_delete(&mut self, entry: crate::ssh_sftp::SftpEntry) {
        self.nebula_confirm = None;
        if let Some(panel) = self.nebula_sftp_panel.as_ref() {
            panel.delete(entry);
        }
    }
}
