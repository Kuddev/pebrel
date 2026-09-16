//! Keymap pane state: the 按键映射 search field and clash detection, the
//! visible-row projections, and the 键位自定义 (spec 002) capture flow --
//! begin/cancel/preview/assign/clear plus the commit that persists a rebind.
//! Also carries the adjacent global quick-terminal hotkey registration glue
//! (take_quick_hotkey_request / quick_hotkey_registration_done) and the
//! ssh-host auto-save helper that sit alongside the customization block.

use super::keymap;
use super::settings;
use super::ui;

use super::Display;

impl Display {
    // ---- 按键映射页：搜索与冲突 ----

    /// 搜索框是否接管键盘（捕获态优先于搜索）。
    pub fn keymap_search_active(&self) -> bool {
        self.nebula_settings_open
            && self.nebula_keymap_search_focus
            && self.nebula_keymap_capture.is_none()
    }

    pub fn focus_keymap_search(&mut self) {
        self.nebula_keymap_search_focus = true;
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn blur_keymap_search(&mut self) {
        if self.nebula_keymap_search_focus {
            self.nebula_keymap_search_focus = false;
            self.update_settings_ime_cursor();
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    pub fn keymap_search_push(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.nebula_keymap_query_cursor.insert(&mut self.nebula_keymap_query, &ch.to_string());
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn keymap_search_edit(&mut self, text: &str) {
        let clean: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        self.nebula_keymap_query_cursor.insert(&mut self.nebula_keymap_query, &clean);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn keymap_search_backspace(&mut self) {
        self.nebula_keymap_query_cursor.backspace(&mut self.nebula_keymap_query);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn keymap_search_delete_forward(&mut self) {
        self.nebula_keymap_query_cursor.delete_forward(&mut self.nebula_keymap_query);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn keymap_search_move(&mut self, forward: bool, extend: bool) {
        let text = self.nebula_keymap_query.clone();
        self.nebula_keymap_query_cursor.step(&text, forward, extend);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn keymap_search_jump(&mut self, to_end: bool, extend: bool) {
        let text = self.nebula_keymap_query.clone();
        self.nebula_keymap_query_cursor.jump(&text, to_end, extend);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn keymap_search_select_all(&mut self) {
        let text = self.nebula_keymap_query.clone();
        self.nebula_keymap_query_cursor.select_all(&text);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 搜索框文本起点 x 与单元格宽；与渲染同源，点击定位不会漂。
    pub fn keymap_search_text_origin(&self) -> (f32, f32) {
        let scale = self.window.scale_factor as f32;
        let rect = settings::keymap_search_rect(
            &self.size_info,
            scale,
            self.terminal_card_rect(),
            self.nebula_settings_scroll,
            self.nebula_hidden_hosts.len(),
            self.ssh_host_count(),
            self.nebula_density,
            self.keymap_pane_state(),
        );
        (rect.0 + 12.0 * scale, self.size_info.cell_width())
    }

    pub fn keymap_search_place(&mut self, offset_x: f32, cell_w: f32, extend: bool) {
        let text = self.nebula_keymap_query.clone();
        let index = ui::text_field::index_at(&text, offset_x, cell_w);
        if extend {
            self.nebula_keymap_query_cursor.extend_to(&text, index);
        } else {
            self.nebula_keymap_query_cursor.place(&text, index);
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn keymap_search_selected_text(&self) -> Option<String> {
        self.nebula_keymap_query_cursor.selected_text(&self.nebula_keymap_query)
    }

    /// Esc 两段式：先清词，词已空则退出聚焦——与字体弹层搜索一致。
    pub fn keymap_search_escape(&mut self) {
        if self.nebula_keymap_query.is_empty() {
            self.nebula_keymap_search_focus = false;
        } else {
            self.nebula_keymap_query.clear();
            self.nebula_keymap_query_cursor = Default::default();
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// flat 行的可搜索文本：动作名（中英）+ 当前键位展示串。
    fn keymap_row_haystack(&self, flat: usize) -> String {
        let combo = if flat == keymap::QUICK_TERMINAL_ROW {
            keymap::display_stored_combo(&self.nebula_quick_terminal_hotkey)
        } else {
            keymap::EDITABLE_ACTIONS
                .get(flat - 1)
                .and_then(|(action, ..)| keymap::effective_combo(action, &self.nebula_keymap))
                .map(|(combo, _)| combo)
                .unwrap_or_default()
        };
        let (zh, en) = if flat == keymap::QUICK_TERMINAL_ROW {
            ("快速终端", "Quick terminal")
        } else {
            keymap::EDITABLE_ACTIONS.get(flat - 1).map(|(_, zh, en)| (*zh, *en)).unwrap_or(("", ""))
        };
        format!("{zh} {en} {combo}").to_lowercase()
    }

    /// 过滤后的可见行（flat 下标，升序）。空查询 = 全部。
    pub fn keymap_visible_editable(&self) -> Vec<usize> {
        let query = self.nebula_keymap_query.trim().to_lowercase();
        (0..keymap::editable_row_count())
            .filter(|flat| query.is_empty() || self.keymap_row_haystack(*flat).contains(&query))
            .collect()
    }

    pub(super) fn keymap_visible_readonly(&self) -> Vec<usize> {
        let query = self.nebula_keymap_query.trim().to_lowercase();
        keymap::READONLY_ROWS
            .iter()
            .enumerate()
            .filter(|(_, (zh, en, combo))| {
                query.is_empty() || format!("{zh} {en} {combo}").to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// 冲突检测：同一 combo 绑了多个动作 → 每行标记 + 一句提示。只报第一组
    /// ——修完一组再报下一组，提示条不该自己变成列表。
    pub(super) fn keymap_clash_info(&self) -> (Vec<bool>, Option<String>) {
        let total = keymap::editable_row_count();
        let mut combos: Vec<Option<String>> = Vec::with_capacity(total);
        for flat in 0..total {
            let combo = if flat == keymap::QUICK_TERMINAL_ROW {
                Some(keymap::display_stored_combo(&self.nebula_quick_terminal_hotkey))
            } else {
                keymap::EDITABLE_ACTIONS
                    .get(flat - 1)
                    .and_then(|(action, ..)| keymap::effective_combo(action, &self.nebula_keymap))
                    .map(|(combo, _)| combo)
            };
            combos.push(combo.filter(|combo| !combo.is_empty()));
        }
        let mut rows = vec![false; total];
        let mut note = None;
        let name = |flat: usize| -> String {
            if flat == keymap::QUICK_TERMINAL_ROW {
                self.nebula_language.pick("快速终端", "Quick terminal").to_owned()
            } else {
                keymap::EDITABLE_ACTIONS
                    .get(flat - 1)
                    .map(|(_, zh, en)| self.nebula_language.pick(zh, en).to_owned())
                    .unwrap_or_default()
            }
        };
        for a in 0..total {
            let Some(combo_a) = combos[a].clone() else { continue };
            for b in (a + 1)..total {
                let Some(combo_b) = &combos[b] else { continue };
                if !combo_a.eq_ignore_ascii_case(combo_b) {
                    continue;
                }
                rows[a] = true;
                rows[b] = true;
                if note.is_none() {
                    let (a_name, b_name) = (name(a), name(b));
                    let zh = format!(
                        "{combo_a} 同时绑定了「{a_name}」与「{b_name}」——只有排前面的「{a_name}」会触发"
                    );
                    let en = format!(
                        "{combo_a} is bound to both {a_name} and {b_name} — only {a_name}, listed first, fires"
                    );
                    note = Some(self.nebula_language.pick(&zh, &en).to_owned());
                }
            }
        }
        (rows, note)
    }

    /// 按键映射页几何输入（滚动上限与命中测试的调用方取用）。
    pub fn keymap_pane_state(&self) -> settings::KeymapPaneState {
        let visible = self.keymap_visible_editable();
        let mut pane = settings::KeymapPaneState {
            readonly_visible: self.keymap_visible_readonly().len() as u8,
            clash: self.keymap_clash_info().1.is_some(),
            ..Default::default()
        };
        let mut start = 0usize;
        for (group, (.., count)) in keymap::GROUPS.iter().enumerate() {
            let end = start + count;
            pane.visible[group] =
                visible.iter().filter(|flat| (start..end).contains(*flat)).count() as u8;
            start = end;
        }
        pane
    }

    /// 可见槽位 → flat 行（点击命中带的是过滤后的槽位）。
    pub fn keymap_slot_to_flat(&self, slot: usize) -> Option<usize> {
        self.keymap_visible_editable().get(slot).copied()
    }

    pub fn keymap_begin_capture_slot(&mut self, slot: usize) {
        if let Some(flat) = self.keymap_slot_to_flat(slot) {
            self.nebula_keymap_search_focus = false;
            self.keymap_begin_capture(flat);
        }
    }

    /// Auto-save an SSH destination the user typed and successfully connected
    /// to — armed at OSC 133;C, confirmed by a remote `NEBULA|` title or a
    /// session that outlived [`crate::ssh::SAVE_MIN_SESSION`]. Recents: most recent first, deduped, capped. An already-listed host
    /// only refreshes its recency (for the next launch) — the visible list
    /// never jumps while the user is looking at it.
    pub fn nebula_save_ssh_host(&mut self, host: &str) {
        const SAVED_HOSTS_CAP: usize = 20;
        if host.is_empty() {
            return;
        }
        self.nebula_saved_hosts.retain(|h| h != host);
        self.nebula_hidden_hosts.retain(|h| h != host);
        self.nebula_saved_hosts.insert(0, host.to_owned());
        self.nebula_saved_hosts.truncate(SAVED_HOSTS_CAP);
        if !self.nebula_ssh_hosts.iter().any(|h| h == host) {
            // New host: insert below the pinned block, above everything else.
            let at = self
                .nebula_ssh_hosts
                .iter()
                .take_while(|h| self.nebula_pinned_hosts.contains(h))
                .count();
            self.nebula_ssh_hosts.insert(at, host.to_owned());
        }
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    // ---- 键位自定义（spec 002）----

    /// 设置页点击某行的 keycap：进入捕获态（下一次按键成为新绑定）。
    pub fn keymap_begin_capture(&mut self, row: usize) {
        if row < keymap::editable_row_count() {
            self.nebula_keymap_capture = Some(row);
            self.nebula_keymap_capture_preview.clear();
            self.nebula_quick_hotkey_error = None;
            self.pending_update.dirty = true;
        }
    }

    pub fn keymap_cancel_capture(&mut self) {
        if self.nebula_keymap_capture.take().is_some() {
            self.nebula_keymap_capture_preview.clear();
            self.pending_update.dirty = true;
        }
    }

    /// 捕获态的实时修饰键回显（ModifiersChanged 与纯修饰键按下都会走到
    /// 这里）：按住 Ctrl 立即显示 "Ctrl+…"，全部松开回到占位提示。
    pub fn keymap_capture_preview(&mut self, mods: winit::keyboard::ModifiersState) {
        if self.nebula_keymap_capture.is_none() {
            return;
        }
        let prefix = keymap::mods_prefix(mods);
        if self.nebula_keymap_capture_preview != prefix {
            self.nebula_keymap_capture_preview = prefix;
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    /// 捕获完成：`combo` 归属该行动作。同 combo 的旧自定义行被移除（键随
    /// 最后写入者），该动作旧的自定义行也移除（一动作一自定义键）。
    pub fn keymap_assign(&mut self, row: usize, combo: String) {
        if row == keymap::QUICK_TERMINAL_ROW {
            self.nebula_keymap_capture = None;
            self.nebula_keymap_capture_preview.clear();
            self.nebula_quick_terminal_hotkey = combo;
            self.nebula_quick_hotkey_error = None;
            self.nebula_quick_hotkey_request = Some(self.nebula_quick_terminal_hotkey.clone());
            self.persist_nebula_settings();
            self.pending_update.dirty = true;
            return;
        }
        let action_row = row.saturating_sub(1);
        let Some((action, ..)) = keymap::EDITABLE_ACTIONS.get(action_row) else { return };
        let name = keymap::action_storage_name(action);
        self.nebula_keybinds.retain(|(c, a)| c != &combo && !a.eq_ignore_ascii_case(&name));
        self.nebula_keybinds.push((combo, name));
        self.keymap_commit();
    }

    /// Bare Backspace disables the action, preserving pass-through on reload.
    pub fn keymap_clear_custom(&mut self, row: usize) {
        if row == keymap::QUICK_TERMINAL_ROW {
            self.nebula_keymap_capture = None;
            self.nebula_keymap_capture_preview.clear();
            self.nebula_quick_terminal_hotkey.clear();
            self.nebula_quick_hotkey_error = None;
            self.nebula_quick_hotkey_request = Some(self.nebula_quick_terminal_hotkey.clone());
            self.persist_nebula_settings();
            self.pending_update.dirty = true;
            return;
        }
        let action_row = row.saturating_sub(1);
        let Some((action, ..)) = keymap::EDITABLE_ACTIONS.get(action_row) else { return };
        keymap::clear_action(&mut self.nebula_keybinds, action);
        self.keymap_commit();
    }

    fn keymap_commit(&mut self) {
        self.nebula_keymap_capture = None;
        self.nebula_keymap_capture_preview.clear();
        self.nebula_keymap = keymap::build_bindings(&self.nebula_keybinds);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// 取走一次性全局快捷键更新请求，由输入层送到 Processor 的全局管理器。
    pub(crate) fn take_quick_hotkey_request(&mut self) -> Option<String> {
        self.nebula_quick_hotkey_request.take()
    }

    /// Processor 完成注册后的确认/回滚。失败时恢复磁盘与界面中的旧值，
    /// 防止设置页把未注册的组合误显示成当前快捷键。
    pub(crate) fn quick_hotkey_registration_done(
        &mut self,
        requested: &str,
        accepted: bool,
        error: Option<&str>,
        fallback: &str,
    ) {
        if accepted {
            self.nebula_quick_hotkey_error = None;
            return;
        }
        if self.nebula_quick_terminal_hotkey == requested {
            self.nebula_quick_terminal_hotkey = fallback.to_owned();
            self.persist_nebula_settings();
        }
        self.nebula_quick_hotkey_error = error.map(str::to_owned);
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }
}
