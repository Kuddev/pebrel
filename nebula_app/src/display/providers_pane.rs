//! Providers pane state: the WebDAV sync fields and actions, plus the AI
//! provider list -- selection, add/delete, Codex apply, per-field editing and
//! drag, save, and the connection self-test request/outcome plumbing.

use super::settings;
use super::ui;

use super::Display;

impl Display {
    // ---- 设置→高级→同步（WebDAV） ----

    /// 打开设置时装载同步状态：url/username/auto_pull 来自
    /// `nebula_sync.txt`，密码/口令只查存在性（明文不进 UI 状态）。
    pub fn load_sync_state(&mut self) {
        let cfg = crate::sync::SyncConfig::load();
        self.nebula_sync_inputs[0] = cfg.url;
        self.nebula_sync_inputs[1] = cfg.username;
        self.nebula_sync_inputs[2].clear();
        self.nebula_sync_inputs[3].clear();
        self.nebula_sync_auto_pull = cfg.auto_pull;
        self.nebula_sync_secret_set = [crate::sync::has_password(), crate::sync::has_passphrase()];
        self.nebula_sync_focus = None;
    }

    /// 聚焦某个同步输入框；先提交上一个（点击切换即失焦保存）。
    pub fn focus_sync_field(&mut self, index: usize) {
        if self.nebula_sync_focus == Some(index) {
            return;
        }
        self.commit_sync_field();
        self.nebula_sync_focus = Some(index.min(3));
        self.pending_update.dirty = true;
    }

    pub fn sync_field_push(&mut self, ch: char) {
        let Some(index) = self.nebula_sync_focus else { return };
        if ch.is_control() {
            return;
        }
        // url/username 拒绝空白；密码/口令允许内部空格（trim 在保存侧）。
        if index < 2 && ch.is_whitespace() {
            return;
        }
        if self.nebula_sync_inputs[index].chars().count() < 256 {
            self.nebula_sync_inputs[index].push(ch);
            self.pending_update.dirty = true;
        }
    }

    pub fn sync_field_paste(&mut self, text: &str) {
        for ch in text.chars() {
            self.sync_field_push(ch);
        }
    }

    pub fn sync_field_backspace(&mut self) {
        let Some(index) = self.nebula_sync_focus else { return };
        if self.nebula_sync_inputs[index].pop().is_some() {
            self.pending_update.dirty = true;
        }
    }

    /// 失焦提交：url/username 写 `nebula_sync.txt`；密码/口令若有输入则
    /// 存入凭据管理器并清空缓冲。口令被弱口令闸拒绝时留在状态行。
    pub fn commit_sync_field(&mut self) {
        let Some(index) = self.nebula_sync_focus.take() else { return };
        self.pending_update.dirty = true;
        match index {
            0 | 1 => {
                let mut cfg = crate::sync::SyncConfig::load();
                cfg.url = self.nebula_sync_inputs[0].trim().to_owned();
                cfg.username = self.nebula_sync_inputs[1].trim().to_owned();
                cfg.auto_pull = self.nebula_sync_auto_pull;
                if let Err(err) = cfg.save() {
                    self.nebula_sync_status = Some((err, true));
                }
            },
            2 | 3 => {
                let secret = std::mem::take(&mut self.nebula_sync_inputs[index]);
                if secret.trim().is_empty() {
                    return;
                }
                let username = self.nebula_sync_inputs[1].trim().to_owned();
                let result = if index == 2 {
                    crate::sync::store_password(&username, &secret)
                } else {
                    crate::sync::store_passphrase(&username, &secret)
                };
                match result {
                    Ok(()) => {
                        self.nebula_sync_secret_set[index - 2] = true;
                        self.nebula_sync_status = Some((
                            if index == 2 {
                                "WebDAV 密码已保存到凭据管理器".to_owned()
                            } else {
                                "同步口令已保存到凭据管理器".to_owned()
                            },
                            false,
                        ));
                    },
                    Err(err) => self.nebula_sync_status = Some((err, true)),
                }
            },
            _ => {},
        }
    }

    // ---- 设置→供应商 ----

    pub(super) fn provider_edit_index(&self) -> Option<usize> {
        self.nebula_providers
            .providers
            .iter()
            .position(|provider| provider.id == self.nebula_providers.active_id)
    }

    pub(crate) fn provider_sync_inputs(&mut self) {
        let Some(index) = self.provider_edit_index() else {
            self.nebula_provider_inputs = Default::default();
            self.nebula_provider_cursors = Default::default();
            self.nebula_provider_focus = None;
            return;
        };
        let provider = &self.nebula_providers.providers[index];
        self.nebula_provider_inputs[0] = provider.name.clone();
        self.nebula_provider_inputs[1] = provider.note.clone();
        self.nebula_provider_inputs[2] = provider.website_url.clone();
        self.nebula_provider_inputs[3] = provider.base_url.clone();
        self.nebula_provider_inputs[4] = provider.model.clone();
        self.nebula_provider_inputs[5].clear();
        for (text, cursor) in
            self.nebula_provider_inputs.iter().zip(self.nebula_provider_cursors.iter_mut())
        {
            cursor.collapse_to_end(text);
        }
        self.nebula_provider_focus = None;
    }

    pub fn provider_select(&mut self, index: usize) {
        let Some(id) =
            self.nebula_providers.providers.get(index).map(|provider| provider.id.clone())
        else {
            return;
        };
        self.commit_provider_field();
        self.nebula_providers.active_id = id;
        self.nebula_provider_test_seq = self.nebula_provider_test_seq.wrapping_add(1);
        self.nebula_provider_test_request = None;
        self.nebula_provider_codex_confirm = None;
        self.nebula_provider_status = None;
        let _ = crate::ai_providers::save(&self.nebula_providers);
        self.provider_sync_inputs();
        self.pending_update.dirty = true;
    }

    pub fn provider_add(&mut self) {
        self.commit_provider_field();
        let id = crate::ai_providers::next_custom_id(&self.nebula_providers);
        let provider =
            crate::ai_providers::AiProvider::preset(crate::ai_providers::ProviderKind::Custom, &id);
        self.nebula_providers.active_id = id;
        self.nebula_providers.providers.push(provider);
        self.nebula_provider_codex_confirm = None;
        self.nebula_provider_status = None;
        self.provider_sync_inputs();
        let _ = crate::ai_providers::save(&self.nebula_providers);
        self.nebula_provider_codex_confirm = None;
        self.pending_update.dirty = true;
    }

    pub fn provider_toggle_codex_goals(&mut self) {
        let Some(index) = self.provider_edit_index() else { return };
        let provider = &mut self.nebula_providers.providers[index];
        provider.codex_goals = !provider.codex_goals;
        self.nebula_provider_codex_confirm = None;
        let _ = crate::ai_providers::save(&self.nebula_providers);
        self.pending_update.dirty = true;
    }

    pub fn provider_toggle_codex_remote(&mut self) {
        let Some(index) = self.provider_edit_index() else { return };
        let provider = &mut self.nebula_providers.providers[index];
        provider.codex_remote_compaction = !provider.codex_remote_compaction;
        self.nebula_provider_codex_confirm = None;
        let _ = crate::ai_providers::save(&self.nebula_providers);
        self.pending_update.dirty = true;
    }

    pub fn provider_apply_codex(&mut self) {
        self.commit_provider_field();
        let Some(index) = self.provider_edit_index() else { return };
        let provider = self.nebula_providers.providers[index].clone();
        if self.nebula_provider_codex_confirm.as_deref() != Some(provider.id.as_str()) {
            self.nebula_provider_codex_confirm = Some(provider.id);
            self.nebula_provider_status = Some((
                self.ui_language()
                    .pick(
                        "再次点击确认：API Key 将明文写入 Codex auth.json（原文件会备份）",
                        "Click again: the API key will be written to Codex auth.json in plaintext (with backup)",
                    )
                    .to_owned(),
                false,
            ));
            self.pending_update.dirty = true;
            return;
        }
        self.nebula_provider_codex_confirm = None;
        self.nebula_provider_status = Some(match crate::codex_config::apply_provider(&provider) {
            Ok(path) => (
                self.ui_language().pick("已应用到 Codex：", "Applied to Codex: ").to_owned()
                    + &path.display().to_string(),
                false,
            ),
            Err(error) => (error, true),
        });
        self.pending_update.dirty = true;
    }

    pub fn provider_toggle_enabled(&mut self) {
        let Some(index) = self.provider_edit_index() else { return };
        self.nebula_providers.providers[index].enabled =
            !self.nebula_providers.providers[index].enabled;
        let _ = crate::ai_providers::save(&self.nebula_providers);
        self.pending_update.dirty = true;
    }

    pub fn focus_provider_field(&mut self, index: usize) {
        if index >= self.nebula_provider_inputs.len() {
            return;
        }
        if self.nebula_provider_focus != Some(index) {
            self.commit_provider_field();
            self.nebula_provider_focus = Some(index);
            self.nebula_provider_cursors[index]
                .collapse_to_end(&self.nebula_provider_inputs[index]);
            self.pending_update.dirty = true;
        }
    }

    pub fn provider_field_push(&mut self, ch: char) {
        let mut buffer = [0; 4];
        self.provider_field_paste(ch.encode_utf8(&mut buffer));
    }

    pub fn provider_field_backspace(&mut self) {
        let Some(index) = self.nebula_provider_focus else { return };
        self.nebula_provider_cursors[index].backspace(&mut self.nebula_provider_inputs[index]);
        self.pending_update.dirty = true;
    }

    pub fn provider_field_paste(&mut self, text: &str) {
        let Some(index) = self.nebula_provider_focus else { return };
        let available = 512usize.saturating_sub(self.nebula_provider_inputs[index].chars().count());
        let clean: String = text
            .chars()
            .filter(|ch| !ch.is_control() && (index == 1 || !ch.is_whitespace()))
            .take(available)
            .collect();
        self.nebula_provider_cursors[index].insert(&mut self.nebula_provider_inputs[index], &clean);
        self.pending_update.dirty = true;
    }

    pub fn provider_field_delete_forward(&mut self) {
        let Some(index) = self.nebula_provider_focus else { return };
        self.nebula_provider_cursors[index].delete_forward(&mut self.nebula_provider_inputs[index]);
        self.pending_update.dirty = true;
    }

    pub fn provider_field_move(&mut self, forward: bool, extend: bool) {
        let Some(index) = self.nebula_provider_focus else { return };
        let text = self.nebula_provider_inputs[index].clone();
        self.nebula_provider_cursors[index].step(&text, forward, extend);
        self.pending_update.dirty = true;
    }

    pub fn provider_field_jump(&mut self, to_end: bool, extend: bool) {
        let Some(index) = self.nebula_provider_focus else { return };
        let text = self.nebula_provider_inputs[index].clone();
        self.nebula_provider_cursors[index].jump(&text, to_end, extend);
        self.pending_update.dirty = true;
    }

    pub fn provider_field_select_all(&mut self) {
        let Some(index) = self.nebula_provider_focus else { return };
        let text = self.nebula_provider_inputs[index].clone();
        self.nebula_provider_cursors[index].select_all(&text);
        self.pending_update.dirty = true;
    }

    pub fn provider_field_selected_text(&self) -> Option<String> {
        let index = self.nebula_provider_focus?;
        // Secret fields accept paste but never expose cleartext to Clipboard.
        (index != 5).then(|| {
            self.nebula_provider_cursors[index].selected_text(&self.nebula_provider_inputs[index])
        })?
    }

    pub fn provider_field_cut(&mut self) -> Option<String> {
        let selected = self.provider_field_selected_text()?;
        self.provider_field_backspace();
        Some(selected)
    }

    pub fn provider_field_place(&mut self, index: usize, pointer_x: f32, extend: bool) {
        if self.nebula_provider_focus != Some(index) {
            return;
        }
        let scale = self.window.scale_factor as f32;
        let Some(field) = settings::provider_input_rect(
            &self.size_info,
            scale,
            self.terminal_card_rect(),
            self.nebula_settings_scroll,
            self.nebula_hidden_hosts.len(),
            self.ssh_host_count(),
            self.nebula_density,
            self.nebula_providers.providers.len(),
            index,
        ) else {
            return;
        };
        let text = self.nebula_provider_inputs[index].clone();
        let at = ui::text_field::index_at(
            &text,
            pointer_x - field.0 - 12.0 * scale,
            self.size_info.cell_width(),
        );
        if extend {
            self.nebula_provider_cursors[index].extend_to(&text, at);
        } else {
            self.nebula_provider_cursors[index].place(&text, at);
        }
        self.pending_update.dirty = true;
    }

    pub fn begin_provider_field_drag(&mut self, index: usize, pointer_x: f32, extend: bool) {
        self.provider_field_place(index, pointer_x, extend);
        self.nebula_settings_text_drag = Some((3, index));
        self.update_settings_ime_cursor();
    }

    pub fn commit_provider_field(&mut self) {
        let Some(field) = self.nebula_provider_focus.take() else { return };
        let Some(index) = self.provider_edit_index() else { return };
        let provider = &mut self.nebula_providers.providers[index];
        let value = self.nebula_provider_inputs[field].trim().to_owned();
        match field {
            0 => provider.name = value,
            1 => provider.note = value,
            2 => provider.website_url = value,
            3 => provider.base_url = value,
            4 => provider.model = value,
            5 => {
                if !value.is_empty() {
                    match crate::ai_providers::store_provider_api_key(provider, &value) {
                        Ok(()) => {
                            self.nebula_provider_status = Some((
                                self.ui_language()
                                    .pick(
                                        "API Key 已保存到凭据管理器",
                                        "API key saved to the credential manager",
                                    )
                                    .to_owned(),
                                false,
                            ));
                        },
                        Err(err) => self.nebula_provider_status = Some((err.to_string(), true)),
                    }
                }
                self.nebula_provider_inputs[5].clear();
            },
            _ => {},
        }
        let _ = crate::ai_providers::save(&self.nebula_providers);
        self.pending_update.dirty = true;
    }

    pub fn provider_save(&mut self) {
        self.commit_provider_field();
        if let Err(err) = crate::ai_providers::save(&self.nebula_providers) {
            self.nebula_provider_status = Some((err.to_string(), true));
        } else {
            self.nebula_provider_status = Some((
                self.ui_language().pick("供应商配置已保存", "Provider saved").to_owned(),
                false,
            ));
        }
        self.pending_update.dirty = true;
    }

    pub fn provider_test(&mut self) {
        self.commit_provider_field();
        let Some(index) = self.provider_edit_index() else { return };
        let provider = self.nebula_providers.providers[index].clone();
        let valid_url =
            provider.base_url.starts_with("http://") || provider.base_url.starts_with("https://");
        if !valid_url
            || provider.model.trim().is_empty()
            || (provider.kind.requires_api_key() && !provider.api_key_set)
        {
            self.nebula_provider_status = Some((
                self.ui_language()
                    .pick(
                        "请填写有效的请求地址、模型并保存 API Key",
                        "Enter an endpoint and model, then save an API key",
                    )
                    .to_owned(),
                true,
            ));
            self.pending_update.dirty = true;
            return;
        }
        self.nebula_provider_test_seq = self.nebula_provider_test_seq.wrapping_add(1);
        let request_id = self.nebula_provider_test_seq;
        self.nebula_provider_test_request =
            Some(crate::ai_providers::ProviderTestRequest { request_id, provider });
        self.nebula_provider_status = Some((
            self.ui_language().pick("正在测试连接…", "Testing connection...").to_owned(),
            false,
        ));
        self.pending_update.dirty = true;
    }

    pub(crate) fn take_provider_test_request(
        &mut self,
    ) -> Option<crate::ai_providers::ProviderTestRequest> {
        self.nebula_provider_test_request.take()
    }

    pub(crate) fn provider_test_done(
        &mut self,
        request_id: u64,
        provider_id: &str,
        outcome: &crate::provider_test::ProviderTestOutcome,
        elapsed_ms: u64,
    ) {
        if request_id != self.nebula_provider_test_seq
            || provider_id != self.nebula_providers.active_id
        {
            return;
        }
        self.nebula_provider_status = Some((
            format!("{} · {elapsed_ms} ms", self.ui_language().provider_test_message(outcome)),
            !outcome.is_success(),
        ));
        self.pending_update.dirty = true;
    }

    pub fn provider_delete(&mut self) {
        let Some(index) = self.provider_edit_index() else { return };
        let id = self.nebula_providers.providers[index].id.clone();
        self.nebula_provider_test_seq = self.nebula_provider_test_seq.wrapping_add(1);
        self.nebula_provider_test_request = None;
        self.nebula_provider_codex_confirm = None;
        self.nebula_provider_status =
            match crate::ai_providers::remove_provider(&mut self.nebula_providers, &id) {
                Ok(()) => {
                    self.provider_sync_inputs();
                    Some((
                        self.ui_language().pick("供应商已删除", "Provider deleted").to_owned(),
                        false,
                    ))
                },
                Err(error) => Some((error.to_string(), true)),
            };
        self.pending_update.dirty = true;
    }

    pub fn provider_count(&self) -> usize {
        self.nebula_providers.providers.len()
    }

    /// Esc：丢弃当前草稿并失焦（url/username 还原为文件值）。
    pub fn cancel_sync_field(&mut self) {
        let Some(index) = self.nebula_sync_focus.take() else { return };
        let cfg = crate::sync::SyncConfig::load();
        match index {
            0 => self.nebula_sync_inputs[0] = cfg.url,
            1 => self.nebula_sync_inputs[1] = cfg.username,
            _ => self.nebula_sync_inputs[index].clear(),
        }
        self.pending_update.dirty = true;
    }
}
