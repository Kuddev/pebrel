//! Settings pickers: the live terminal-background preview, the shell picker,
//! the startup-directory pick/clear/import flow, the font picker (catalog
//! build, search-query field and popup scrollbar, family apply), the default
//! shell/profile choosers, the settings text-drag glue shared by those
//! fields, and restoring a hidden SSH host.

use std::path::PathBuf;

use super::file_dialog;
use super::settings;
use super::ssh_ui::merge_ssh_hosts;
use super::ui;
use super::{NebulaShell, ToastKind, contains_rect};

use crate::config::font::Font;
use crate::display::color::Rgb;

use super::Display;

impl Display {
    /// Terminal background color the live settings preview should show: the
    /// custom background wins, else the active theme's terminal background.
    pub(super) fn preview_terminal_bg(&self) -> Rgb {
        self.nebula_background.unwrap_or(self.nebula_theme.palette().term_bg)
    }

    pub fn toggle_shell_picker(&mut self) {
        self.toggle_settings_dropdown(settings::SettingsDropdown::Shell);
    }

    pub fn close_shell_picker(&mut self) {
        if self.nebula_settings_dropdown == Some(settings::SettingsDropdown::Shell) {
            self.close_settings_dropdown();
        }
    }

    pub fn pick_startup_directory(&mut self) {
        let Some(path) = file_dialog::pick_startup_directory(&self.window) else { return };
        if !path.is_dir() {
            return;
        }

        self.nebula_startup_directory = Some(path);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn clear_startup_directory(&mut self) {
        if self.nebula_startup_directory.take().is_none() {
            return;
        }

        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn import_terminal_directory(&mut self) -> bool {
        let Some(directory) = file_dialog::pick_terminal_directory(&self.window) else {
            return false;
        };
        let found = match crate::terminal_profiles::scan_directory(&directory) {
            Ok(found) => found,
            Err(error) => {
                self.push_toast(format!("无法扫描终端目录: {error}"), ToastKind::Warning);
                return false;
            },
        };
        if found.is_empty() {
            self.push_toast("目录中未找到受支持的终端程序", ToastKind::Warning);
            return false;
        }

        let mut profiles = match crate::terminal_profiles::TerminalProfiles::load() {
            Ok(profiles) => profiles,
            Err(error) => {
                self.push_toast(format!("无法读取终端配置: {error}"), ToastKind::Warning);
                return false;
            },
        };
        let count = found.len();
        for profile in found {
            if let Err(error) = profiles.upsert(profile) {
                self.push_toast(format!("无法导入终端: {error}"), ToastKind::Warning);
                return false;
            }
        }
        match profiles.save() {
            Ok(()) => {
                self.push_toast(format!("已导入 {count} 个终端，立即可用"), ToastKind::Success);
                true
            },
            Err(error) => {
                self.push_toast(format!("无法保存终端配置: {error}"), ToastKind::Warning);
                false
            },
        }
    }

    pub(crate) fn startup_directory(&self) -> Option<PathBuf> {
        self.nebula_startup_directory.as_ref().filter(|path| path.is_dir()).cloned()
    }

    pub fn toggle_font_picker(&mut self) {
        // 首次展开才枚举系统字体——这是整个功能里唯一昂贵的一步，放在
        // 用户已经预期有一次加载的时刻。
        if self.nebula_settings_dropdown != Some(settings::SettingsDropdown::Font) {
            self.ensure_font_catalog();
        }
        self.toggle_settings_dropdown(settings::SettingsDropdown::Font);
    }

    /// 惰性装配**字体目录**：系统族与导入族合并去重、按当前过滤条件筛选，
    /// 当前生效字体始终保留。
    fn ensure_font_catalog(&mut self) {
        #[cfg(windows)]
        if self.nebula_system_fonts.is_none() {
            self.nebula_system_fonts = Some(self.glyph_cache.system_font_families());
        }
        self.rebuild_font_catalog();
    }

    pub(super) fn rebuild_font_catalog(&mut self) {
        let system = self.nebula_system_fonts.clone().unwrap_or_default();
        #[cfg(windows)]
        let imported = self.glyph_cache.private_font_families();
        #[cfg(not(windows))]
        let imported: Vec<String> = Vec::new();
        // 多级 fallback 列表（issue #33）在目录里以主族身份参与匹配与高亮；
        // 链本身仍原样保存在设置值里。
        let primary_family =
            crate::renderer::primary_font_family(&self.nebula_font_family).to_owned();
        let catalog = crate::font_install::font_catalog(
            &system,
            &imported,
            self.nebula_font_show_all,
            &self.nebula_font_query,
            &primary_family,
        );
        self.nebula_font_proportional = catalog
            .iter()
            .filter(|entry| !entry.monospaced)
            .map(|entry| entry.name.to_lowercase())
            .collect();
        let mut families: Vec<String> = catalog.into_iter().map(|entry| entry.name).collect();
        // 内置字体永远排在最前，与上游一致。
        families.retain(|family| family != crate::font_install::REQUIRED_FONT_FAMILY);
        families.insert(0, crate::font_install::REQUIRED_FONT_FAMILY.to_owned());
        if !families.iter().any(|family| family == &primary_family) {
            families.push(primary_family);
        }
        self.nebula_font_families = families;
        // 候选集合变了，旧滚动位置无意义；回到顶部避免窗口悬在越界偏移上。
        self.nebula_font_popup_scroll = 0;
    }

    /// 搜索框里文本的起点 x 与单元格宽——鼠标定位光标要用它换算落点。
    /// 与渲染同源（[`settings::font_search_field_rect`]），两边不会漂。
    pub fn font_search_text_origin(&self) -> (f32, f32) {
        let scale = self.window.scale_factor as f32;
        let cell_w = self.size_info.cell_width();
        let field = settings::font_search_field_rect(
            &self.size_info,
            scale,
            self.terminal_card_rect(),
            self.nebula_settings_section,
            self.nebula_settings_scroll,
            self.nebula_settings_dropdown,
            self.font_picker_count(),
            self.nebula_font_popup_scroll,
            self.nebula_hidden_hosts.len(),
            self.ssh_host_count(),
            self.nebula_density,
        );
        (field.map_or(0.0, |rect| rect.0 + 12.0 * scale), cell_w)
    }

    pub fn font_query(&self) -> &str {
        &self.nebula_font_query
    }

    /// 插入文本，或 `None` 表示退格。查询串一变就重建目录——列表跟着打字走，
    /// 才是「所见即所搜」。
    pub fn font_query_edit(&mut self, insert: Option<&str>) {
        match insert {
            Some(text) => {
                let clean: String = text.chars().filter(|ch| !ch.is_control()).collect();
                if clean.is_empty() {
                    return;
                }
                self.nebula_font_query_cursor.insert(&mut self.nebula_font_query, &clean);
            },
            None => self.nebula_font_query_cursor.backspace(&mut self.nebula_font_query),
        }
        self.rebuild_font_catalog();
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn font_query_delete_forward(&mut self) {
        self.nebula_font_query_cursor.delete_forward(&mut self.nebula_font_query);
        self.rebuild_font_catalog();
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn font_query_move(&mut self, forward: bool, extend: bool) {
        let text = self.nebula_font_query.clone();
        self.nebula_font_query_cursor.step(&text, forward, extend);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn font_query_jump(&mut self, to_end: bool, extend: bool) {
        let text = self.nebula_font_query.clone();
        self.nebula_font_query_cursor.jump(&text, to_end, extend);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn font_query_select_all(&mut self) {
        let text = self.nebula_font_query.clone();
        self.nebula_font_query_cursor.select_all(&text);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn font_query_selected_text(&self) -> Option<String> {
        self.nebula_font_query_cursor.selected_text(&self.nebula_font_query)
    }

    /// 按点击落点定位光标。`offset_x` 是相对文本起点的距离。
    pub fn font_query_place(&mut self, offset_x: f32, cell_w: f32, extend: bool) {
        let text = self.nebula_font_query.clone();
        let index = ui::text_field::index_at(&text, offset_x, cell_w);
        if extend {
            self.nebula_font_query_cursor.extend_to(&text, index);
        } else {
            self.nebula_font_query_cursor.place(&text, index);
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn begin_font_query_drag(&mut self, offset_x: f32, cell_w: f32, extend: bool) {
        self.font_query_place(offset_x, cell_w, extend);
        self.nebula_settings_text_drag = Some((0, 0));
        self.update_settings_ime_cursor();
    }

    pub fn begin_keymap_search_drag(&mut self, offset_x: f32, cell_w: f32, extend: bool) {
        self.keymap_search_place(offset_x, cell_w, extend);
        self.nebula_settings_text_drag = Some((1, 0));
        self.update_settings_ime_cursor();
    }

    pub fn begin_ssh_proxy_drag(&mut self, index: usize, x: f32, extend: bool) {
        self.ssh_proxy_field_place(index, x, extend);
        self.nebula_settings_text_drag = Some((2, index.min(1)));
        self.update_settings_ime_cursor();
    }

    pub fn settings_text_drag_to(&mut self, x: f32) -> bool {
        let Some((kind, index)) = self.nebula_settings_text_drag else { return false };
        match kind {
            0 => {
                let (text_x, cell_w) = self.font_search_text_origin();
                self.font_query_place(x - text_x, cell_w, true);
            },
            1 => {
                let (text_x, cell_w) = self.keymap_search_text_origin();
                self.keymap_search_place(x - text_x, cell_w, true);
            },
            2 => self.ssh_proxy_field_place(index, x, true),
            3 => self.provider_field_place(index, x, true),
            _ => return false,
        }
        self.update_settings_ime_cursor();
        true
    }

    pub fn end_settings_text_drag(&mut self) -> bool {
        self.nebula_settings_text_drag.take().is_some()
    }

    /// 将输入法候选窗锚到当前自绘字段的 caret。终端网格的 caret 仍由主渲染
    /// 路径维护；设置页是另一套坐标系，若不在这里重推，中文候选窗会飘到
    /// 终端左上角，看起来就像输入框没有获得焦点。
    pub(crate) fn update_settings_ime_cursor(&self) {
        if !self.nebula_settings_open {
            self.window.reset_ime_cursor_area_cache();
            return;
        }
        let scale = self.window.scale_factor as f32;
        let cell_w = self.size_info.cell_width();
        let cell_h = self.size_info.cell_height();
        let caret = |text: &str, cursor: &ui::text_field::TextCursor| {
            ui::text_field::columns_before(text, cursor.caret(text)) as f32 * cell_w
        };
        if self.nebula_settings_dropdown == Some(settings::SettingsDropdown::Font) {
            if let Some(field) = settings::font_search_field_rect(
                &self.size_info,
                scale,
                self.terminal_card_rect(),
                self.nebula_settings_section,
                self.nebula_settings_scroll,
                self.nebula_settings_dropdown,
                self.font_picker_count(),
                self.nebula_font_popup_scroll,
                self.nebula_hidden_hosts.len(),
                self.ssh_host_count(),
                self.nebula_density,
            ) {
                self.window.set_ime_cursor_area_px(
                    field.0
                        + 12.0 * scale
                        + caret(&self.nebula_font_query, &self.nebula_font_query_cursor),
                    field.1,
                    cell_w,
                    field.3.max(cell_h),
                );
            }
        } else if self.keymap_search_active() {
            let field = settings::keymap_search_rect(
                &self.size_info,
                scale,
                self.terminal_card_rect(),
                self.nebula_settings_scroll,
                self.nebula_hidden_hosts.len(),
                self.ssh_host_count(),
                self.nebula_density,
                self.keymap_pane_state(),
            );
            self.window.set_ime_cursor_area_px(
                field.0
                    + 12.0 * scale
                    + caret(&self.nebula_keymap_query, &self.nebula_keymap_query_cursor),
                field.1,
                cell_w,
                field.3.max(cell_h),
            );
        } else if let Some(index) = self.nebula_ssh_proxy_focus {
            let field = settings::ssh_proxy_input_rect(
                &self.size_info,
                scale,
                self.terminal_card_rect(),
                self.nebula_settings_scroll,
                self.nebula_hidden_hosts.len(),
                self.ssh_host_count(),
                self.nebula_density,
                self.ssh_proxy_pane_state(),
                index,
            );
            let text = self.ssh_proxy_field_text(index);
            self.window.set_ime_cursor_area_px(
                field.0 + 12.0 * scale + caret(text, &self.nebula_ssh_proxy_cursor[index.min(1)]),
                field.1,
                cell_w,
                field.3.max(cell_h),
            );
        } else if let Some(index) = self.nebula_provider_focus {
            if let Some(field) = settings::provider_input_rect(
                &self.size_info,
                scale,
                self.terminal_card_rect(),
                self.nebula_settings_scroll,
                self.nebula_hidden_hosts.len(),
                self.ssh_host_count(),
                self.nebula_density,
                self.nebula_providers.providers.len(),
                index,
            ) {
                let text = &self.nebula_provider_inputs[index];
                self.window.set_ime_cursor_area_px(
                    field.0 + 12.0 * scale + caret(text, &self.nebula_provider_cursors[index]),
                    field.1,
                    cell_w,
                    field.3.max(cell_h),
                );
            }
        }
    }

    pub fn font_popup_scroll(&self) -> usize {
        self.nebula_font_popup_scroll
    }

    pub fn font_popup_scroll_by(&mut self, delta: i32) -> bool {
        let total = settings::font_popup_row_count(self.nebula_font_families.len());
        let max_scroll = total.saturating_sub(8);
        let next =
            (self.nebula_font_popup_scroll as i32 + delta).clamp(0, max_scroll as i32) as usize;
        if next == self.nebula_font_popup_scroll {
            return false;
        }
        self.nebula_font_popup_scroll = next;
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    /// 字体弹层滚动条几何（绘制/命中同源）与最大候选偏移。
    fn font_popup_scrollbar(&self) -> Option<(ui::widgets::OverlayScrollbar, usize)> {
        settings::font_popup_scrollbar(
            &self.size_info,
            self.window.scale_factor as f32,
            self.terminal_card_rect(),
            self.nebula_settings_section,
            self.nebula_settings_scroll,
            self.nebula_settings_dropdown,
            self.font_picker_count(),
            self.nebula_font_popup_scroll,
            self.nebula_hidden_hosts.len(),
            self.ssh_host_count(),
            self.nebula_density,
        )
    }

    /// 按下：命中 track/thumb 即接管拖拽并立即滚到目标。返回是否消费。
    pub fn font_popup_scrollbar_press(&mut self, x: f32, y: f32) -> bool {
        let Some((bar, max)) = self.font_popup_scrollbar() else { return false };
        if !bar.hit_test(x, y) {
            return false;
        }
        let grab = if contains_rect(bar.thumb, x, y) { y - bar.thumb.1 } else { bar.thumb.3 * 0.5 };
        self.nebula_font_popup_drag = Some(grab);
        self.nebula_font_popup_scroll = bar.target_offset(y, grab, max);
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    pub fn font_popup_scrollbar_drag_to(&mut self, y: f32) -> bool {
        let Some(grab) = self.nebula_font_popup_drag else { return false };
        let Some((bar, max)) = self.font_popup_scrollbar() else { return false };
        let target = bar.target_offset(y, grab, max);
        if target != self.nebula_font_popup_scroll {
            self.nebula_font_popup_scroll = target;
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
        true
    }

    pub fn font_popup_scrollbar_dragging(&self) -> bool {
        self.nebula_font_popup_drag.is_some()
    }

    pub fn end_font_popup_scrollbar_drag(&mut self) -> bool {
        self.nebula_font_popup_drag.take().is_some()
    }

    /// 切换「显示全部」并重建目录。这是临时过滤，不写入设置。
    pub fn toggle_font_show_all(&mut self) {
        self.nebula_font_show_all = !self.nebula_font_show_all;
        self.ensure_font_catalog();
        self.nebula_font_popup_scroll = 0;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn close_font_picker(&mut self) {
        if self.nebula_settings_dropdown == Some(settings::SettingsDropdown::Font) {
            self.close_settings_dropdown();
        }
    }

    pub fn effective_font(&self, base: &Font) -> Font {
        base.clone().with_family(self.nebula_font_family.clone())
    }

    /// 事务性地切换字体族：先确认它真能加载，成功才更新生效字体与持久化
    /// 偏好；失败保留原字体与原偏好，并给出可理解的错误。
    ///
    /// 上游此前「先持久化再加载」是安全的——那时目录里只有内置字体与已经
    /// 成功导入过的族，个个都预先验证过。系统字体枚举打破了这个不变量，
    /// 所以这道预检是本功能自带的安全网，不是顺手修的既有缺陷。
    fn apply_font_family(&mut self, family: String, base: &Font) {
        if !self.glyph_cache.family_loads(&family, self.font_size) {
            self.nebula_font_notice = Some(format!("字体无法加载：{family}"));
            self.pending_update.dirty = true;
            self.window.request_redraw();
            return;
        }
        // 字体选择器只换主族；用户手写的多级 fallback 链（逗号分隔，
        // issue #33）原样保留在新值后面。
        let family = {
            let rest: Vec<&str> = crate::renderer::split_font_families(&self.nebula_font_family)
                .into_iter()
                .skip(1)
                .filter(|fallback| *fallback != family)
                .collect();
            if rest.is_empty() { family } else { format!("{family}, {}", rest.join(", ")) }
        };
        self.nebula_font_family = family;
        self.nebula_font_notice = None;
        let font = self.effective_font(base).with_size(self.font_size);
        self.pending_update.set_font(font);
        self.persist_nebula_settings();
        self.window.request_redraw();
    }

    pub fn set_terminal_font_by_index(&mut self, index: usize, base: &Font) {
        if let Some(family) = self.nebula_font_families.get(index).cloned() {
            self.apply_font_family(family, base);
            self.nebula_settings_dropdown = None;
            return;
        }
        // 倒数第二行：临时过滤切换，不关闭下拉——用户通常要接着挑字体。
        if index == self.nebula_font_families.len() {
            self.toggle_font_show_all();
            return;
        }
        if index != self.nebula_font_families.len() + 1 {
            return;
        }

        #[cfg(windows)]
        {
            let Some(source) = file_dialog::pick_font_file(&self.window) else { return };
            let stored = match crate::font_install::store_imported_font(&source) {
                Ok(stored) => stored,
                Err(error) => {
                    self.nebula_font_notice = Some(error);
                    self.nebula_settings_dropdown = None;
                    self.pending_update.dirty = true;
                    return;
                },
            };
            match self.glyph_cache.add_private_font(&stored.path) {
                Ok(families) => {
                    for family in &families {
                        if !self.nebula_font_families.iter().any(|known| known == family) {
                            self.nebula_font_families.push(family.clone());
                        }
                    }
                    self.nebula_font_families[1..]
                        .sort_by_key(|family| family.to_ascii_lowercase());
                    if let Some(family) = families.into_iter().next() {
                        self.apply_font_family(family, base);
                    }
                },
                Err(error) => {
                    if stored.created {
                        let _ = std::fs::remove_file(&stored.path);
                    }
                    self.nebula_font_notice = Some(format!("字体无法加载：{error}"));
                    self.pending_update.dirty = true;
                },
            }
            self.nebula_settings_dropdown = None;
        }
        #[cfg(not(windows))]
        self.open_user_config_file();
    }

    /// Default-shell picker (command palette mode). Kept for compatibility.
    /// detected-shell dropdown as the "+" chevron, but confirming SETS the
    /// default instead of launching a tab. Replaces the old 2-value cycle.
    pub fn open_default_shell_picker(&mut self) {
        let shells =
            self.nebula_detected_shells.get_or_insert_with(crate::shell_detect::detect_shells);
        let profiles: Vec<_> = self
            .nebula_profiles
            .iter()
            .filter(|profile| profile.settings_id().is_some())
            .cloned()
            .collect();
        let default_shell =
            self.nebula_shell_id.as_deref().unwrap_or_else(|| self.nebula_shell.settings_value());
        self.nebula_palette.set_default_shell_menu(shells, &profiles, default_shell);
        self.nebula_palette.open_default_picker();
        self.pending_update.dirty = true;
    }

    /// Apply a picked default shell: keep the raw id for persistence and the
    /// spawn override, and track the PTY-integrated executor family in the
    /// enum so the prompt bootstrap picks the right base.
    pub fn set_default_shell(&mut self, shell: &crate::shell_detect::DetectedShell) {
        if let Some(family) = NebulaShell::from_settings(&shell.id) {
            self.nebula_shell = family;
        }
        self.nebula_shell_id = Some(shell.id.clone());
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// Persist an imported terminal profile as the default shell. The profile
    /// key resolves back to the live config on the next tab creation, while
    /// the actual command and arguments remain owned by the imported store.
    pub fn set_default_profile(&mut self, profile: &crate::config::ui_config::Profile) {
        let Some(id) = profile.settings_id() else { return };
        if let Some(family) = profile.shell_id.as_deref().and_then(NebulaShell::from_settings) {
            self.nebula_shell = family;
        }
        self.nebula_shell_id = Some(id);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub fn set_default_shell_by_index(&mut self, index: usize) {
        let detected_count = self.nebula_detected_shells.as_ref().map_or(0, Vec::len);
        let shell =
            self.nebula_detected_shells.as_ref().and_then(|shells| shells.get(index)).cloned();
        if let Some(shell) = shell {
            self.set_default_shell(&shell);
        } else if let Some(profile) = self
            .nebula_profiles
            .iter()
            .filter(|profile| profile.settings_id().is_some())
            .nth(index.saturating_sub(detected_count))
            .cloned()
        {
            self.set_default_profile(&profile);
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    /// Restore a destination after the short Undo period. Config aliases only
    /// need to leave `hidden_hosts`; manually saved addresses are re-added to
    /// the saved list. Expired credentials intentionally remain deleted.
    pub fn restore_hidden_ssh_host(&mut self, index: usize) {
        let Some(host) = self.nebula_hidden_hosts.get(index).cloned() else { return };
        let pending_same_host =
            self.nebula_ssh_delete_undo.as_ref().is_some_and(|undo| undo.host == host);
        if pending_same_host && self.undo_delete_ssh_host() {
            return;
        }

        self.nebula_hidden_hosts.retain(|entry| entry != &host);
        let from_config = crate::ssh::ssh_config_hosts().iter().any(|entry| entry == &host);
        if !from_config && !self.nebula_saved_hosts.iter().any(|entry| entry == &host) {
            self.nebula_saved_hosts.insert(0, host);
        }
        self.nebula_ssh_hosts = merge_ssh_hosts(
            &self.nebula_saved_hosts,
            &self.nebula_pinned_hosts,
            &self.nebula_hidden_hosts,
        );
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }
}
