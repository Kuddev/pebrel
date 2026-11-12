//! Legacy proxy text-field editing. The shared model owns URL composition;
//! incomplete schemes remain UI drafts and never replace the saved address.

use super::{Display, settings, ui};

impl Display {
    /// 手动代理协议下拉只改持久化前缀，地址正文保持不变。
    pub fn set_ssh_proxy_protocol(&mut self, index: usize) {
        let Some(protocol) = settings::MANUAL_PROXY_PROTOCOL_OPTIONS.get(index).copied() else {
            return;
        };
        self.commit_ssh_proxy_field();
        let address = if self.nebula_ssh_proxy_choice == settings::ProxyChoice::Manual {
            self.ssh_proxy_field_text(0).to_owned()
        } else {
            String::new()
        };
        self.nebula_ssh_proxy_protocol = protocol;
        self.set_ssh_proxy_field_text(0, address);
        self.invalidate_proxy_test();
        self.nebula_settings_dropdown = None;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 聚焦某个代理输入框；先提交上一个（点击切换即失焦保存）。完整地址
    /// 更新持久镜像，未输入主机的协议保留为草稿；快照留给 Esc 还原。
    pub fn focus_ssh_proxy_field(&mut self, index: usize) {
        let index = index.min(2);
        if self.nebula_ssh_proxy_focus == Some(index) {
            return;
        }
        self.commit_ssh_proxy_field();
        self.nebula_ssh_proxy_backup =
            [self.nebula_ssh_proxy_url.clone(), self.nebula_ssh_proxy_no_proxy.clone()];
        self.nebula_ssh_proxy_focus = Some(index);
        let text = self.ssh_proxy_field_text(index).to_owned();
        self.nebula_ssh_proxy_cursor[index].collapse_to_end(&text);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
    }

    pub(super) fn ssh_proxy_field_text(&self, index: usize) -> &str {
        match index {
            0 if self.nebula_ssh_proxy_choice == settings::ProxyChoice::Manual => {
                self.nebula_ssh_proxy_draft.text(&self.nebula_ssh_proxy_url)
            },
            0 => "",
            1 => &self.nebula_ssh_proxy_no_proxy,
            _ => crate::ssh_proxy::command_target(&self.nebula_ssh_proxy_url).unwrap_or(""),
        }
    }

    fn set_ssh_proxy_field_text(&mut self, index: usize, field: String) {
        match index {
            0 => {
                self.nebula_ssh_proxy_choice = settings::ProxyChoice::Manual;
                self.nebula_ssh_proxy_draft.edit(
                    &mut self.nebula_ssh_proxy_protocol,
                    &mut self.nebula_ssh_proxy_url,
                    field,
                );
            },
            1 => self.nebula_ssh_proxy_no_proxy = field,
            _ => self.nebula_ssh_proxy_url = format!("command:{field}"),
        }
    }

    pub fn ssh_proxy_cursor(&self, index: usize) -> &ui::text_field::TextCursor {
        &self.nebula_ssh_proxy_cursor[index.min(2)]
    }

    pub fn ssh_proxy_field_push(&mut self, ch: char) {
        self.ssh_proxy_field_paste(&ch.to_string());
    }

    pub fn ssh_proxy_field_paste(&mut self, text: &str) {
        let Some(index) = self.nebula_ssh_proxy_focus else { return };
        // 手动 URL 无空白；绕过列表与命令允许空格。控制字符统一丢弃。
        let clean: String = text
            .chars()
            .filter(|ch| !ch.is_control() && !(index == 0 && ch.is_whitespace()))
            .collect();
        if clean.is_empty() {
            return;
        }
        let mut field = self.ssh_proxy_field_text(index).to_owned();
        self.nebula_ssh_proxy_cursor[index].insert(&mut field, &clean);
        if field.chars().count() > 256 {
            return;
        }
        self.set_ssh_proxy_field_text(index, field);
        self.invalidate_proxy_test();
        // 完整地址立即落盘供下一次连接使用；仅协议的草稿保留上次保存值。
        self.persist_nebula_settings();
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn ssh_proxy_field_backspace(&mut self) {
        let Some(index) = self.nebula_ssh_proxy_focus else { return };
        let mut field = self.ssh_proxy_field_text(index).to_owned();
        self.nebula_ssh_proxy_cursor[index].backspace(&mut field);
        self.set_ssh_proxy_field_text(index, field);
        self.invalidate_proxy_test();
        self.persist_nebula_settings();
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn ssh_proxy_field_delete_forward(&mut self) {
        let Some(index) = self.nebula_ssh_proxy_focus else { return };
        let mut field = self.ssh_proxy_field_text(index).to_owned();
        self.nebula_ssh_proxy_cursor[index].delete_forward(&mut field);
        self.set_ssh_proxy_field_text(index, field);
        self.invalidate_proxy_test();
        self.persist_nebula_settings();
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn ssh_proxy_field_move(&mut self, forward: bool, extend: bool) {
        let Some(index) = self.nebula_ssh_proxy_focus else { return };
        let text = self.ssh_proxy_field_text(index).to_owned();
        self.nebula_ssh_proxy_cursor[index].step(&text, forward, extend);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn ssh_proxy_field_jump(&mut self, to_end: bool, extend: bool) {
        let Some(index) = self.nebula_ssh_proxy_focus else { return };
        let text = self.ssh_proxy_field_text(index).to_owned();
        self.nebula_ssh_proxy_cursor[index].jump(&text, to_end, extend);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn ssh_proxy_field_select_all(&mut self) {
        let Some(index) = self.nebula_ssh_proxy_focus else { return };
        let text = self.ssh_proxy_field_text(index).to_owned();
        self.nebula_ssh_proxy_cursor[index].select_all(&text);
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn ssh_proxy_field_selected_text(&self) -> Option<String> {
        let index = self.nebula_ssh_proxy_focus?;
        self.nebula_ssh_proxy_cursor[index].selected_text(self.ssh_proxy_field_text(index))
    }

    /// 点击定位：把窗口内的落点换算回全文字符索引。窗口逻辑与
    /// [`settings::ssh_proxy_input_rect`] / 渲染侧的尾窗口一致。
    pub fn ssh_proxy_field_place(&mut self, index: usize, x: f32, extend: bool) {
        let index = index.min(2);
        if self.nebula_ssh_proxy_focus != Some(index) {
            return;
        }
        let scale = self.window.scale_factor as f32;
        let cell_w = self.size_info.cell_width();
        let (ix, _, iw, _) = settings::ssh_proxy_input_rect(
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
        let raw = self.ssh_proxy_field_text(index).to_owned();
        let max_cols = (((iw - 24.0 * scale) / cell_w) as usize).max(1);
        // 与渲染同一套窗口：尾窗口能盖住光标就用尾窗口，否则从光标开窗。
        let total = raw.chars().count();
        let mut cols = 0usize;
        let mut tail_len = 0usize;
        for ch in raw.chars().rev() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
            if cols + w > max_cols {
                break;
            }
            cols += w;
            tail_len += 1;
        }
        let caret = self.nebula_ssh_proxy_cursor[index].caret(&raw);
        let tail_hidden = total - tail_len;
        let hidden = if caret >= tail_hidden { tail_hidden } else { caret };
        let visible: String = raw.chars().skip(hidden).collect();
        let at = ui::text_field::index_at(&visible, x - (ix + 12.0 * scale), cell_w) + hidden;
        if extend {
            self.nebula_ssh_proxy_cursor[index].extend_to(&raw, at);
        } else {
            self.nebula_ssh_proxy_cursor[index].place(&raw, at);
        }
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 失焦提交：trim 后写 `nebula_settings.txt`。
    pub fn commit_ssh_proxy_field(&mut self) {
        if self.nebula_ssh_proxy_focus.take().is_none() {
            return;
        }
        self.nebula_ssh_proxy_url = self.nebula_ssh_proxy_url.trim().to_owned();
        self.nebula_ssh_proxy_no_proxy = self.nebula_ssh_proxy_no_proxy.trim().to_owned();
        if self.nebula_ssh_proxy_choice == settings::ProxyChoice::Command {
            let command =
                crate::ssh_proxy::command_target(&self.nebula_ssh_proxy_url).unwrap_or("").trim();
            self.nebula_ssh_proxy_url =
                if command.is_empty() { String::new() } else { format!("command:{command}") };
        }
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// Esc：还原为聚焦时的快照并失焦，不落盘。
    pub fn cancel_ssh_proxy_field(&mut self) {
        if self.nebula_ssh_proxy_focus.take().is_none() {
            return;
        }
        let [url, no_proxy] = std::mem::take(&mut self.nebula_ssh_proxy_backup);
        self.nebula_ssh_proxy_draft.clear();
        self.nebula_ssh_proxy_url = url;
        self.nebula_ssh_proxy_protocol = settings::manual_proxy_parts(&self.nebula_ssh_proxy_url).0;
        self.nebula_ssh_proxy_no_proxy = no_proxy;
        self.pending_update.dirty = true;
    }
}
