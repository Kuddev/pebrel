//! Network-proxy pane state: the global proxy mode selection, the pane-state
//! snapshot, the system-proxy probe and local-proxy scan, the connection
//! self-test request/status plumbing, per-host jump/protocol/override picks,
//! and the ssh-proxy field editing (focus, cursor, insert, drag-free place,
//! commit and cancel).

use super::settings;
use super::ui;

use super::Display;

impl Display {
    /// 设置→网络代理：选择全局模式（下拉行序 =
    /// [`settings::SSH_PROXY_MODE_OPTIONS`]），落盘即生效——连接侧每次
    /// 建连都重读设置文件。
    pub fn set_ssh_proxy_mode(&mut self, index: usize) {
        if let Some(mode) = settings::SSH_PROXY_MODE_OPTIONS.get(index) {
            if self.nebula_ssh_proxy_mode != *mode {
                self.nebula_ssh_proxy_mode = *mode;
                self.invalidate_proxy_test();
                self.persist_nebula_settings();
                if *mode == crate::ssh_proxy::ProxyMode::System {
                    // 切到跟随系统时刷新「当前读到」——只在点击时读注册表。
                    self.refresh_system_proxy_probe();
                }
            }
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 网络页几何的动态输入（滚动上限与命中测试的调用方取用）。
    pub fn ssh_proxy_pane_state(&self) -> settings::ProxyPaneState {
        settings::ProxyPaneState {
            mode: self.nebula_ssh_proxy_mode,
            choice: self.nebula_ssh_proxy_choice,
            found_count: self.nebula_local_proxies.len(),
            scanning: self.nebula_proxy_scanning,
            override_count: 0,
        }
    }

    /// 刷新「跟随系统」探测缓存。注册表读取是跨进程调用，只允许由
    /// 进网络页 / 切模式 / 启动这几个离散事件触发，绝不逐帧。
    pub fn refresh_system_proxy_probe(&mut self) {
        self.nebula_system_proxy_probe = crate::ssh_proxy::probe_system_proxy()
            .map(|(url, source)| (url, source == crate::ssh_proxy::SystemProxySource::Registry));
    }

    pub(super) fn invalidate_proxy_test(&mut self) {
        self.nebula_proxy_test_seq = self.nebula_proxy_test_seq.wrapping_add(1);
        self.nebula_proxy_test_request = None;
        self.nebula_proxy_test_status = settings::ProxyTestStatus::Idle;
    }

    /// 先提交当前输入，再把测试请求交给事件层的共享 SSH runtime。测试线程
    /// 会重新读取落盘配置，因此验证的就是下一条真实连接会使用的值。
    pub fn request_proxy_test(&mut self) {
        self.commit_ssh_proxy_field();
        if matches!(self.nebula_proxy_test_status, settings::ProxyTestStatus::Running) {
            return;
        }
        self.nebula_proxy_test_seq = self.nebula_proxy_test_seq.wrapping_add(1);
        let request_id = self.nebula_proxy_test_seq;
        self.nebula_proxy_test_request = Some(request_id);
        self.nebula_proxy_test_status = settings::ProxyTestStatus::Running;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub(crate) fn take_proxy_test_request(&mut self) -> Option<u64> {
        self.nebula_proxy_test_request.take()
    }

    pub(crate) fn proxy_test_done(
        &mut self,
        request_id: u64,
        outcome: crate::proxy_test::ProxyTestOutcome,
        elapsed_ms: u64,
    ) {
        if request_id != self.nebula_proxy_test_seq
            || !matches!(self.nebula_proxy_test_status, settings::ProxyTestStatus::Running)
        {
            return;
        }
        self.nebula_proxy_test_status = settings::ProxyTestStatus::Complete { outcome, elapsed_ms };
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 指定代理列表选择：发现项在前，随后依次为手动、SSH 跳板、自定义
    /// 命令。切换方式时清空共享 URL，避免解析层拾取已不可见的残值。
    pub fn set_ssh_proxy_link_pick(&mut self, index: usize) {
        let found_count = self.nebula_local_proxies.len();
        let choice = if index < found_count {
            settings::ProxyChoice::Detected(index)
        } else {
            match index - found_count {
                0 => settings::ProxyChoice::Manual,
                1 => settings::ProxyChoice::Jump,
                2 => settings::ProxyChoice::Command,
                _ => return,
            }
        };
        if self.nebula_ssh_proxy_choice != choice {
            self.nebula_ssh_proxy_choice = choice;
            self.nebula_ssh_proxy_focus = None;
            self.nebula_ssh_proxy_url = match choice {
                settings::ProxyChoice::Detected(found) => self
                    .nebula_local_proxies
                    .get(found)
                    .map(|proxy| proxy.url())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            if choice == settings::ProxyChoice::Manual {
                self.nebula_ssh_proxy_protocol = settings::ManualProxyProtocol::Socks5;
            }
            self.persist_nebula_settings();
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn request_local_proxy_scan(&mut self) {
        if self.nebula_proxy_scanning {
            return;
        }
        self.nebula_proxy_scanning = true;
        self.nebula_proxy_scan_request = true;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub(crate) fn take_local_proxy_scan_request(&mut self) -> bool {
        std::mem::take(&mut self.nebula_proxy_scan_request)
    }

    pub fn local_proxy_scan_done(&mut self, proxies: Vec<crate::ssh_proxy::LocalProxyEndpoint>) {
        self.nebula_proxy_scanning = false;
        self.nebula_local_proxies = proxies;
        if self.nebula_ssh_proxy_mode == crate::ssh_proxy::ProxyMode::Custom
            && self.nebula_ssh_proxy_url.trim().is_empty()
            && !self.nebula_local_proxies.is_empty()
        {
            self.nebula_ssh_proxy_choice = settings::ProxyChoice::Detected(0);
            self.nebula_ssh_proxy_url = self.nebula_local_proxies[0].url();
            self.persist_nebula_settings();
        }
        self.nebula_ssh_proxy_choice = self
            .nebula_local_proxies
            .iter()
            .position(|proxy| proxy.url() == self.nebula_ssh_proxy_url)
            .map(settings::ProxyChoice::Detected)
            .unwrap_or_else(|| {
                if crate::ssh_proxy::jump_target(&self.nebula_ssh_proxy_url).is_some() {
                    settings::ProxyChoice::Jump
                } else if crate::ssh_proxy::command_target(&self.nebula_ssh_proxy_url).is_some() {
                    settings::ProxyChoice::Command
                } else {
                    settings::ProxyChoice::Manual
                }
            });
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 跳板下拉选择：写 `jump:<destination>` 并立即落盘（连接侧每次建连
    /// 重读设置文件，无需再通知）。
    pub fn set_ssh_proxy_jump_host(&mut self, index: usize) {
        if let Some(destination) = self.nebula_ssh_hosts.get(index) {
            let value = format!("jump:{destination}");
            if self.nebula_ssh_proxy_url != value {
                self.nebula_ssh_proxy_url = value;
                self.persist_nebula_settings();
            }
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 手动代理协议下拉只改持久化前缀，地址正文保持不变。
    pub fn set_ssh_proxy_protocol(&mut self, index: usize) {
        let Some(protocol) = settings::MANUAL_PROXY_PROTOCOL_OPTIONS.get(index).copied() else {
            return;
        };
        self.commit_ssh_proxy_field();
        let address = if self.nebula_ssh_proxy_choice == settings::ProxyChoice::Manual {
            settings::manual_proxy_parts(&self.nebula_ssh_proxy_url).1.to_owned()
        } else {
            String::new()
        };
        self.nebula_ssh_proxy_protocol = protocol;
        self.nebula_ssh_proxy_url = settings::manual_proxy_value(protocol, &address);
        self.nebula_ssh_proxy_choice = settings::ProxyChoice::Manual;
        self.invalidate_proxy_test();
        self.nebula_settings_dropdown = None;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// 每主机覆盖行 → 打开该主机的编辑器。`index` 是覆盖列表下标，过滤
    /// 顺序与视图构建完全一致（同一迭代 + 同一谓词）。
    pub fn edit_ssh_proxy_override(&mut self, index: usize) {
        let _ = index;
    }

    /// 聚焦某个代理输入框；先提交上一个（点击切换即失焦保存）。编辑直接
    /// 发生在持久镜像字段上，快照留给 Esc 还原。
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
                settings::manual_proxy_parts(&self.nebula_ssh_proxy_url).1
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
                self.nebula_ssh_proxy_url =
                    settings::manual_proxy_value(self.nebula_ssh_proxy_protocol, &field);
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
        // 代理是连接前读取的运行时设置；每次编辑立即落盘，确保用户不必
        // 关闭设置页或重启应用，随后发起的新连接就能读到最新值。
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
        self.nebula_ssh_proxy_url = url;
        self.nebula_ssh_proxy_protocol = settings::manual_proxy_parts(&self.nebula_ssh_proxy_url).0;
        self.nebula_ssh_proxy_no_proxy = no_proxy;
        self.pending_update.dirty = true;
    }
}
