//! Chrome tab state, drag, right-click menus, hit-testing, and SSH delete flow.

use std::path::Path;
use std::time::Instant;

use super::chrome::{self, ChromeTabLayout, TabDrag, chrome_tab_layout};
use super::context_menu;
use super::context_menu_model::{ContextMenuAction, ContextMenuHit, ContextMenuTarget};
use super::file_operations::send_to_recycle_bin;
use super::sftp_panel;
use super::settings;
use super::ssh_ui::{SSH_DELETE_UNDO_DURATION, merge_ssh_hosts, SshDeleteUndo};
use super::ui::{self, widgets};
use super::{
    AiLogo, ChromeHit, NebulaConfirm, PanelDragKind, SettingsHit, SizeInfo, SplitNav,
    TabDropAction, ToastKind, chrome_hit_with_tabs, contains_rect, nebula_data_dir,
    remove_ssh_host_from_lists, restore_ssh_host_to_lists, tab_drop_index_from_visible_rows,
};

use crate::config::UiConfig;
use crate::display::color::Rgb;
use crate::renderer::ui::{Rgba, UiQuad};

use super::Display;

impl Display {
    pub fn doc_view_area(&self) -> (f32, f32, f32, f32) {
        let (cx, cy, cw, ch) = self.terminal_card_rect();
        let scale = self.window.scale_factor as f32;
        (cx + 4.0 * scale, cy + 4.0 * scale, cw - 8.0 * scale, ch - 8.0 * scale)
    }

    /// Standalone images use the complete card as their viewport. Unlike
    /// prose, media does not need a reading inset; leaving one produced a
    /// conspicuous strip beside images that were otherwise fitted to width.
    pub fn image_view_area(&self) -> (f32, f32, f32, f32) {
        self.terminal_card_rect()
    }

    /// Rows in the default-shell dropdown. MUST agree with the list the
    /// settings view renders (`SettingsView::shells` = detected shells +
    /// imported quick-launch profiles): the hit test sizes the popup from this
    /// count, and undercounting made every imported profile's row — drawn and
    /// hovered — unclickable (导入终端后选不中最后一项).
    pub fn shell_picker_count(&self) -> usize {
        self.nebula_detected_shells.as_ref().map_or(0, Vec::len)
            + self.nebula_profiles.iter().filter(|profile| profile.settings_id().is_some()).count()
    }

    /// 字体族行 + 两个固定尾行：「显示全部 / 仅等宽」过滤切换，以及「导入字体…」。
    pub fn font_picker_count(&self) -> usize {
        self.nebula_font_families.len() + 2
    }

    /// 「显示全部」当前是否开启（供设置页渲染该行的文案）。
    pub fn font_show_all(&self) -> bool {
        self.nebula_font_show_all
    }

    pub fn hidden_ssh_host_count(&self) -> usize {
        self.nebula_hidden_hosts.len()
    }

    pub fn ssh_host_count(&self) -> usize {
        self.nebula_ssh_hosts.len()
    }

    /// Re-read the user's SSH config without restarting Nebula. The merge
    /// function is deliberately shared with startup and delete/restore flows,
    /// so importing cannot create a second ordering or hidden-host policy.
    pub fn import_ssh_config(&mut self) {
        self.nebula_ssh_hosts = merge_ssh_hosts(
            &self.nebula_saved_hosts,
            &self.nebula_pinned_hosts,
            &self.nebula_hidden_hosts,
        );
        let count = crate::ssh::ssh_config_hosts()
            .into_iter()
            .filter(|host| self.nebula_ssh_hosts.iter().any(|entry| entry == host))
            .count();
        self.push_toast(format!("已导入 {count} 个 SSH 主机，立即可用"), ToastKind::Success);
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// Ask before removing a saved destination. Config aliases use different
    /// wording because Delete hides them inside Nebula and never edits
    /// `~/.ssh/config` itself.
    pub fn request_delete_ssh_host(&mut self, index: usize) {
        let Some(host) = self.nebula_ssh_hosts.get(index).cloned() else { return };
        let from_config = crate::ssh::ssh_config_hosts().contains(&host);
        self.nebula_confirm = Some(NebulaConfirm::DeleteSsh { host, from_config });
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// Apply a confirmed deletion and arm a complete, credential-safe Undo.
    /// Replacing an older Undo finalizes that older credential deletion first.
    pub fn confirm_delete_ssh_host(&mut self, host: &str) -> bool {
        if !self.nebula_ssh_hosts.iter().any(|entry| entry == host) {
            return false;
        }

        let from_config = crate::ssh::ssh_config_hosts().iter().any(|entry| entry == host);

        // Taking the previous record commits its pending Credential Manager
        // deletion through Drop. Only the most recent destructive action is
        // reversible, matching standard snackbar Undo behavior.
        self.nebula_ssh_delete_undo.take();

        let (saved_index, pinned_index, was_hidden) = remove_ssh_host_from_lists(
            host,
            from_config,
            &mut self.nebula_saved_hosts,
            &mut self.nebula_pinned_hosts,
            &mut self.nebula_hidden_hosts,
        );
        self.nebula_ssh_hosts = merge_ssh_hosts(
            &self.nebula_saved_hosts,
            &self.nebula_pinned_hosts,
            &self.nebula_hidden_hosts,
        );
        self.nebula_ssh_delete_undo = Some(SshDeleteUndo {
            host: host.to_owned(),
            saved_index,
            pinned_index,
            was_hidden,
            from_config,
            started_at: std::time::Instant::now(),
            delete_credential_on_drop: true,
        });
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    /// Reverse the complete host-list mutation. The credential was intentionally
    /// kept alive during the grace period, so disarming Drop restores it without
    /// ever copying secret bytes into the UI process.
    pub fn undo_delete_ssh_host(&mut self) -> bool {
        let Some(mut undo) = self.nebula_ssh_delete_undo.take() else { return false };
        if undo.started_at.elapsed() >= SSH_DELETE_UNDO_DURATION {
            // Drop commits the pending credential deletion.
            return false;
        }

        undo.delete_credential_on_drop = false;
        restore_ssh_host_to_lists(
            &undo.host,
            undo.saved_index,
            undo.pinned_index,
            undo.was_hidden,
            &mut self.nebula_saved_hosts,
            &mut self.nebula_pinned_hosts,
            &mut self.nebula_hidden_hosts,
        );
        self.nebula_ssh_hosts = merge_ssh_hosts(
            &self.nebula_saved_hosts,
            &self.nebula_pinned_hosts,
            &self.nebula_hidden_hosts,
        );
        self.nebula_ssh_delete_undo_rect = None;
        self.nebula_ssh_delete_undo_hover = false;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    /// Commit the pending Credential Manager deletion when the Undo timer ends.
    pub fn expire_ssh_delete_undo(&mut self) {
        self.nebula_ssh_delete_undo.take();
        self.nebula_ssh_delete_undo_rect = None;
        self.nebula_ssh_delete_undo_hover = false;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn ssh_delete_undo_available(&self) -> bool {
        self.nebula_ssh_delete_undo
            .as_ref()
            .is_some_and(|undo| undo.started_at.elapsed() < SSH_DELETE_UNDO_DURATION)
    }

    pub fn ssh_delete_undo_hit(&self, x: f32, y: f32) -> bool {
        self.ssh_delete_undo_available()
            && self.nebula_ssh_delete_undo_rect.is_some_and(|rect| {
                x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3
            })
    }

    pub fn set_ssh_delete_undo_hover(&mut self, hovered: bool) {
        if self.nebula_ssh_delete_undo_hover != hovered {
            self.nebula_ssh_delete_undo_hover = hovered;
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    pub fn set_message_close_hover(&mut self, hovered: bool) {
        if self.nebula_message_close_hover != hovered {
            self.nebula_message_close_hover = hovered;
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    /// 消息栏的关闭按钮。画在 chrome pass 里而不是随消息文本走：横幅是终端
    /// 文字管线画的，那条路只能放字符，而拼进文本的 `[X]` 会被 CJK 消息挤出
    /// 屏幕——按钮点得到却看不见，用户因此报告「无法关闭」。
    ///
    /// 配色走终端色系（横幅是 yellow/red 底 + 背景色的字），所以墨色由终端
    /// pass 一并发布，不取 Skin。
    pub(super) fn draw_message_close(&mut self) {
        // Message bars belong to terminal panes. Special tabs (settings,
        // documents, and images) do not draw the bar, so a close button
        // published by the previously visible terminal must not leak into
        // their chrome pass.
        if self.nebula_special_tab_active {
            return;
        }
        let Some((rect, ink)) = self.nebula_message_close else { return };
        let size = self.size_info;
        let scale = self.window.scale_factor as f32;
        let ink = Rgba::new(ink.r, ink.g, ink.b, 255);
        // 常态就有一层淡底，按钮才读得出"可点"；hover 加深作为反馈。
        let fill =
            Rgba::new(ink.r, ink.g, ink.b, if self.nebula_message_close_hover { 64 } else { 28 });

        let mut quads = Vec::new();
        ui::widgets::push_close_button(&mut quads, rect, scale, ink, fill);
        self.renderer.draw_ui(&size, &quads);
    }

    pub fn set_chrome_tabs(
        &mut self,
        labels: Vec<String>,
        mut colors: Vec<Option<Rgb>>,
        mut dots: Vec<bool>,
        mut running: Vec<bool>,
        mut attention: Vec<bool>,
        mut failed: Vec<bool>,
        mut flashing: Vec<bool>,
        mut logos: Vec<Option<AiLogo>>,
        mut shells: Vec<String>,
        mut ai_fork: Vec<bool>,
        active: usize,
        reorderable: bool,
    ) {
        self.nebula_tab_labels = if labels.is_empty() { vec![".".to_owned()] } else { labels };
        colors.truncate(self.nebula_tab_labels.len());
        colors.resize(self.nebula_tab_labels.len(), None);
        self.nebula_tab_colors = colors;
        dots.truncate(self.nebula_tab_labels.len());
        dots.resize(self.nebula_tab_labels.len(), false);
        self.nebula_tab_bells = dots;
        running.truncate(self.nebula_tab_labels.len());
        running.resize(self.nebula_tab_labels.len(), false);
        self.nebula_tab_running = running;
        attention.truncate(self.nebula_tab_labels.len());
        attention.resize(self.nebula_tab_labels.len(), false);
        self.nebula_tab_attention = attention;
        failed.truncate(self.nebula_tab_labels.len());
        failed.resize(self.nebula_tab_labels.len(), false);
        self.nebula_tab_failed = failed;
        flashing.truncate(self.nebula_tab_labels.len());
        flashing.resize(self.nebula_tab_labels.len(), false);
        self.nebula_tab_flashing = flashing;
        logos.truncate(self.nebula_tab_labels.len());
        logos.resize(self.nebula_tab_labels.len(), None);
        self.nebula_tab_logos = logos;
        shells.truncate(self.nebula_tab_labels.len());
        shells.resize(self.nebula_tab_labels.len(), String::new());
        self.nebula_tab_shells = shells;
        ai_fork.truncate(self.nebula_tab_labels.len());
        ai_fork.resize(self.nebula_tab_labels.len(), false);
        self.nebula_tab_ai_fork = ai_fork;
        self.nebula_active_tab = active.min(self.nebula_tab_labels.len().saturating_sub(1));
        self.nebula_tabs_reorderable = reorderable;
        // A tab count change (close/open) mid-drag invalidates the grabbed slot.
        if self.nebula_tab_drag.map_or(false, |d| d.source >= self.nebula_tab_labels.len()) {
            self.nebula_tab_drag = None;
        }
    }

    /// 有标签正在放对勾闪现。闪现靠挂钟判定，没有帧驱动它就会停在对勾上
    /// 直到下一次因为别的原因重绘——所以这段时间要让 chrome 时钟继续走。
    pub fn any_tab_flashing(&self) -> bool {
        self.nebula_tab_flashing.iter().any(|f| *f)
    }

    /// Whether any sidebar tab currently shows a running spinner. Only this
    /// state raises the chrome clock to display-rate frames.
    pub fn any_tab_running(&self) -> bool {
        self.nebula_tab_running.iter().any(|running| *running)
    }

    /// A chrome text editor (tab rename / drawer filter / commit message / SSH
    /// host editor / command palette) has keyboard focus — the window context
    /// bumps the redraw tick to the fast cadence so the insertion caret
    /// visibly blinks.
    ///
    /// 命令面板曾经漏在这个列表外：它的入场动画一结束，画面就静止了，
    /// 光标停在当时那一相里不再翻转。它自带的 `Pulse` 每帧照常累加，
    /// 但没有帧可累加——**动画状态推进和帧供给是两件事**，只做前者会得到
    /// 一个看起来"卡住"的光标。
    pub fn chrome_editor_active(&self) -> bool {
        self.nebula_tab_rename.is_some()
            || self.nebula_palette.is_open()
            || self.nebula_side_panel.search_focus
            || self.nebula_side_panel.commit_focus
            || self.nebula_sftp_panel.as_ref().is_some_and(sftp_panel::SftpPanel::editor_active)
            || self.ssh_editor_active()
    }

    /// Decoded (and theme-tinted) pixels for an AI brand logo, plus a stable
    /// Arm a potential tab drag from a press on displayed tab `source`. Always
    /// arms (even single-tab), because the release decides between click /
    /// reorder / dock — selection itself is deferred to the release.
    pub fn arm_tab_drag(&mut self, source: usize, x: f32, y: f32) {
        self.nebula_tab_drag =
            Some(TabDrag { source, origin_x: x, origin: y, current: y, active: false, dock: None });
    }

    /// Whether a tab drag is currently armed (pressed, possibly not yet moved).
    pub fn tab_drag_armed(&self) -> bool {
        self.nebula_tab_drag.is_some()
    }

    /// Feed the pointer into an armed drag. Y drives the in-sidebar reorder;
    /// crossing into the terminal area computes the dock side. Returns `true`
    /// once the drag is active (past threshold on either axis), signalling the
    /// caller to show the grab cursor and repaint.
    pub fn update_tab_drag(&mut self, x: f32, y: f32) -> bool {
        let threshold = 6.0 * self.window.scale_factor as f32;
        // Compute before the mutable borrow below.
        let dock = self.dock_nav_at(x, y);
        match self.nebula_tab_drag.as_mut() {
            Some(drag) => {
                drag.current = y;
                if !drag.active
                    && ((y - drag.origin).abs() > threshold
                        || (x - drag.origin_x).abs() > threshold)
                {
                    drag.active = true;
                }
                if drag.active {
                    drag.dock = dock;
                }
                drag.active
            },
            None => false,
        }
    }

    /// Dock side for a pointer inside the terminal area, `None` outside it.
    /// The area is quartered along its diagonals: the nearest edge wins, which
    /// gives the natural triangular dock zones.
    fn dock_nav_at(&self, x: f32, y: f32) -> Option<SplitNav> {
        let gx = self.size_info.padding_x();
        let gy = self.size_info.padding_y();
        let gw = self.size_info.width() - gx - self.size_info.padding_right();
        let gh = self.size_info.height() - gy - self.size_info.padding_bottom();
        if gw <= 0.0 || gh <= 0.0 || x < gx || y < gy || x > gx + gw || y > gy + gh {
            return None;
        }
        let nx = (x - gx) / gw;
        let ny = (y - gy) / gh;
        let (dl, dr, dt, db) = (nx, 1.0 - nx, ny, 1.0 - ny);
        let min = dl.min(dr).min(dt).min(db);
        Some(if min == dl {
            SplitNav::Left
        } else if min == dr {
            SplitNav::Right
        } else if min == dt {
            SplitNav::Up
        } else {
            SplitNav::Down
        })
    }

    /// Finish a tab drag, deciding what the release means.
    pub fn end_tab_drag(&mut self) -> Option<TabDropAction> {
        let drag = self.nebula_tab_drag.take()?;
        if !drag.active {
            // Never moved: a plain click — select on release.
            return Some(TabDropAction::Click(drag.source));
        }
        if let Some(nav) = drag.dock {
            return Some(TabDropAction::Dock { source: drag.source, nav });
        }
        if !self.nebula_tabs_reorderable || self.nebula_tab_labels.len() < 2 {
            return Some(TabDropAction::Click(drag.source));
        }
        let target = self.tab_drop_index(drag.source, drag.current);
        if target != drag.source
            && drag.source < self.nebula_tab_anim.len()
            && target < self.nebula_tab_anim.len()
        {
            // Reorder the animated draw-y values alongside the tabs so each
            // pill keeps its on-screen position and *eases* into its new slot
            // instead of snapping when the drop commits.
            let v = self.nebula_tab_anim.remove(drag.source);
            self.nebula_tab_anim.insert(target, v);
        }
        if target != drag.source {
            Some(TabDropAction::Reorder { from: drag.source, to: target })
        } else {
            Some(TabDropAction::Click(drag.source))
        }
    }

    /// Displayed slot the grabbed tab would drop into for pointer X: the number
    /// of *other* tabs whose centre the pointer has passed. This yields the
    /// correct remove-then-insert target index for a single-tab move.
    fn tab_drop_index(&self, source: usize, y: f32) -> usize {
        let scale = self.window.scale_factor as f32;
        let sidebar_expand = self.left_sidebar_progress();
        let layout =
            chrome_tab_layout(&self.ui_size_info(), scale, self.sidebar_model(), sidebar_expand);
        // `chrome_tab_layout` keeps the storage index stable by representing
        // scrolled-out rows as zero rectangles. Those placeholders are not
        // coordinates: counting them here used to move every drop target by
        // the number of hidden tabs and could even create a reversed clamp
        // interval in `tab_drag_draw_y`. Build the target in the visible row
        // coordinate space, then add the scroll window's real start index.
        let visible: Vec<_> = layout
            .tabs
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, (_, _, width, height))| *width > 0.0 && *height > 0.0)
            .collect();
        tab_drop_index_from_visible_rows(source, y, &visible, self.nebula_tab_labels.len())
    }

    /// Draw-X for a tab's pill/label during a reorder drag. The grabbed pill
    /// follows the pointer (clamped to the strip); every other tab between the
    /// grabbed slot and the current drop target slides one slot toward the
    /// vacated source, opening a gap for the drop ("让位"). No shift when idle.
    pub(super) fn tab_drag_draw_y(&self, index: usize, tab_y: f32, layout: &ChromeTabLayout) -> f32 {
        let Some(d) = self.nebula_tab_drag.filter(|d| d.active) else { return tab_y };

        // Only visible rows have meaningful screen coordinates. Hidden rows
        // are zero placeholders used by hit-testing/index bookkeeping.
        let visible: Vec<_> = layout
            .tabs
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, (_, _, width, height))| *width > 0.0 && *height > 0.0)
            .collect();
        let Some((_, first)) = visible.first() else { return tab_y };

        // The grabbed pill tracks the pointer, clamped to the tab column.
        if d.source == index {
            let lo = first.1;
            let hi = visible.last().map_or(lo, |(_, rect)| rect.1);
            return (tab_y + d.current - d.origin).clamp(lo, hi);
        }

        // Other tabs make way. Slot pitch = distance between adjacent rows
        // (uniform height + gap); needs at least two tabs, which a drag implies.
        let Some((_, second)) = visible.get(1) else { return tab_y };
        let slot = second.1 - first.1;
        let target = self.tab_drop_index(d.source, d.current);
        if d.source < target && index > d.source && index <= target {
            tab_y - slot // dragging down: rows in (source, target] slide up
        } else if d.source > target && index >= target && index < d.source {
            tab_y + slot // dragging up: rows in [target, source) slide down
        } else {
            tab_y
        }
    }

    pub fn set_chrome_hover(&mut self, chrome: ChromeHit, settings: SettingsHit) {
        if self.nebula_chrome_hover != chrome || self.nebula_settings_hover != settings {
            self.nebula_chrome_hover = chrome;
            self.nebula_settings_hover = settings;
            self.pending_update.dirty = true;
        }
    }

    /// Remember the settings control under the primary button while the
    /// pointer is held. The renderer uses this only for the toggle's active
    /// stretch; hit testing remains owned by [`settings_hit`].
    pub fn set_settings_pressed(&mut self, hit: SettingsHit) {
        if self.nebula_settings_pressed != hit {
            self.nebula_settings_pressed = hit;
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    pub fn context_menu_interactive(&self) -> bool {
        self.nebula_context_menu.as_ref().is_some_and(context_menu::ContextMenu::interactive)
    }

    /// 右键菜单是否已在同一目标上开着（开着就别重开——反复右键不该让菜单
    /// 因指针位置或底边 clamp 而漂移）。
    fn context_menu_open_for(&self, target: ContextMenuTarget) -> bool {
        self.nebula_context_menu
            .as_ref()
            .is_some_and(|menu| menu.interactive() && menu.target() == target)
    }

    /// 侧栏行右键菜单的锚点：贴行矩形右缘、与行顶对齐——菜单与被点的行
    /// 强相关，而不是跟着指针走。行不存在时回落指针位置。
    fn sidebar_row_anchor(&self, tab: bool, index: usize, fallback: (f32, f32)) -> (f32, f32) {
        let size = self.ui_size_info();
        let scale = self.window.scale_factor as f32;
        let expand = if self.nebula_sidebar_collapsed { 0.0 } else { 1.0 };
        let layout = chrome::chrome_tab_layout(&size, scale, self.sidebar_model(), expand);
        let rows = if tab { &layout.tabs } else { &layout.hosts };
        rows.get(index).map_or(fallback, |(rx, ry, rw, _)| (rx + rw + 4.0 * scale, *ry))
    }

    pub fn open_tab_context_menu(&mut self, index: usize, x: f32, y: f32) {
        if index >= self.nebula_tab_labels.len()
            || self.context_menu_open_for(ContextMenuTarget::Tab(index))
        {
            return;
        }
        let anchor = self.sidebar_row_anchor(true, index, (x, y));
        let color = self.nebula_tab_colors.get(index).copied().flatten();
        let ai_fork = self.nebula_tab_ai_fork.get(index).copied().unwrap_or(false);
        self.nebula_context_menu = Some(
            context_menu::ContextMenu::new(ContextMenuTarget::Tab(index), anchor, color)
                .with_ai_fork(ai_fork),
        );
        self.nebula_tab_drag = None;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn open_ssh_context_menu(&mut self, index: usize, x: f32, y: f32) {
        if index >= self.nebula_ssh_hosts.len()
            || self.context_menu_open_for(ContextMenuTarget::Ssh(index))
        {
            return;
        }
        let anchor = self.sidebar_row_anchor(false, index, (x, y));
        self.nebula_context_menu =
            Some(context_menu::ContextMenu::new(ContextMenuTarget::Ssh(index), anchor, None));
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn open_sftp_context_menu(&mut self, index: usize, x: f32, y: f32) {
        let Some(panel) = self.nebula_sftp_panel.as_ref() else { return };
        if panel.visible_entry(index).is_none() {
            return;
        }
        self.nebula_context_menu =
            Some(context_menu::ContextMenu::new(ContextMenuTarget::Sftp(index), (x, y), None));
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 本地文件树行的右键菜单。`..` 导航行不给菜单；打开时把该行设为
    /// 持久选中，菜单与行的关联在视觉上立得住。
    pub fn open_file_tree_context_menu(&mut self, row: usize, x: f32, y: f32) {
        let Some((path, is_dir, is_parent)) = self
            .nebula_side_panel
            .visible_row(row)
            .map(|r| (r.path.clone(), r.is_dir, r.is_parent))
        else {
            return;
        };
        if is_parent {
            return;
        }
        let target = ContextMenuTarget::FileTree { row, is_dir };
        if self.context_menu_open_for(target) {
            return;
        }
        self.nebula_side_panel.selected = Some(path);
        self.nebula_context_menu = Some(context_menu::ContextMenu::new(target, (x, y), None));
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 菜单动作执行时按行索引回查路径（树可能在菜单打开期间被节流刷新，
    /// 拿不到就当无操作，绝不落在别的行上）。
    pub fn file_tree_row_path(&self, row: usize) -> Option<(std::path::PathBuf, bool)> {
        self.nebula_side_panel
            .visible_row(row)
            .filter(|r| !r.is_parent)
            .map(|r| (r.path.clone(), r.is_dir))
    }

    pub fn request_delete_file_tree(&mut self, row: usize) {
        let Some((path, is_dir)) = self.file_tree_row_path(row) else { return };
        self.nebula_confirm = Some(NebulaConfirm::DeleteFileTreePath { path, is_dir });
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 确认后的本地删除：送回收站（`FOF_ALLOWUNDO`），不是永久删除——
    /// 树紧挨终端、误触成本高，回收站是最后一道保险。
    pub fn confirm_delete_file_tree(&mut self, path: &std::path::Path) {
        self.nebula_confirm = None;
        match send_to_recycle_bin(path) {
            Ok(()) => self.nebula_side_panel.request_refresh(),
            Err(err) => self.nebula_side_panel.set_notice(format!("删除失败：{err}")),
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn open_sftp_panel_context_menu(&mut self, x: f32, y: f32) {
        if self.nebula_sftp_panel.is_none() {
            return;
        }
        self.nebula_context_menu =
            Some(context_menu::ContextMenu::new(ContextMenuTarget::SftpPanel, (x, y), None));
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn context_menu_hit(&self, x: f32, y: f32) -> ContextMenuHit {
        self.nebula_context_menu.as_ref().map_or(ContextMenuHit::Outside, |menu| {
            context_menu::hit_test(menu, self.ui_size_info(), self.window.scale_factor as f32, x, y)
        })
    }

    pub fn context_menu_hover(&mut self, x: f32, y: f32) -> ContextMenuHit {
        let hit = self.context_menu_hit(x, y);
        let action = match hit {
            ContextMenuHit::Action(action) => Some(action),
            ContextMenuHit::Outside | ContextMenuHit::Panel => None,
        };
        if self.nebula_context_menu.as_mut().is_some_and(|menu| menu.set_hover(action)) {
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
        hit
    }

    /// Resolve one menu click and start the close animation. A click inside
    /// the panel but between targets is swallowed without dismissing it.
    pub fn context_menu_click(&mut self, x: f32, y: f32) -> ContextMenuHit {
        let hit = self.context_menu_hit(x, y);
        if matches!(hit, ContextMenuHit::Action(_) | ContextMenuHit::Outside) {
            self.close_context_menu();
        }
        hit
    }

    pub fn close_context_menu(&mut self) {
        if let Some(menu) = self.nebula_context_menu.as_mut() {
            menu.begin_close();
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    pub fn chrome_hit(&self, x: f32, y: f32) -> ChromeHit {
        // Hit-testing must read the SAME layout the chrome was drawn with —
        // the UI-anchored SizeInfo — or clicks drift off their targets as
        // soon as the terminal is zoomed away from the base font size.
        chrome_hit_with_tabs(
            &self.ui_size_info(),
            self.window.scale_factor as f32,
            self.sidebar_model(),
            self.nebula_sidebar_collapsed,
            x,
            y,
        )
    }
}
