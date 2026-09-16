//! Backup pane state: item selection, remote-config protocol and field
//! editing, the local export/restore pickers, the passphrase confirm flow,
//! and completing or cancelling a running backup operation.

use super::file_dialog;
use super::settings;
use super::{BackupOperation, NebulaConfirm, RemoteBackupRequest};

use super::Display;

impl Display {
    pub fn toggle_backup_selection(&mut self, index: usize) {
        match index {
            0 => self.nebula_backup_selection.appearance = !self.nebula_backup_selection.appearance,
            1 => self.nebula_backup_selection.config = !self.nebula_backup_selection.config,
            2 => self.nebula_backup_selection.ssh = !self.nebula_backup_selection.ssh,
            3 => self.nebula_backup_selection.sync = !self.nebula_backup_selection.sync,
            4 => self.nebula_backup_selection.assistant = !self.nebula_backup_selection.assistant,
            5 => self.nebula_backup_selection.session = !self.nebula_backup_selection.session,
            6 => {
                self.nebula_backup_selection.directory_history =
                    !self.nebula_backup_selection.directory_history
            },
            7 => {
                self.nebula_backup_selection.command_history =
                    !self.nebula_backup_selection.command_history
            },
            8 => self.nebula_backup_selection.fonts = !self.nebula_backup_selection.fonts,
            _ => return,
        }
        self.pending_update.dirty = true;
    }

    // ---- 设置→备份→远程备份 ----

    /// 当前远程备份协议（命中测试要按它裁剪可见输入行）。
    pub fn backup_protocol(&self) -> crate::backup_remote::BackupProtocol {
        self.nebula_backup_protocol
    }

    /// 打开设置时装载远程备份状态：协议与非密文字段来自
    /// `nebula_backup.txt`，密文只查存在性（明文不进 UI 状态）。
    pub fn load_backup_remote_state(&mut self) {
        let cfg = crate::backup_remote::BackupRemoteConfig::load();
        self.nebula_backup_protocol = cfg.protocol;
        self.nebula_backup_remote_inputs = Default::default();
        for (index, input) in self.nebula_backup_remote_inputs.iter_mut().enumerate() {
            if let Some(value) = cfg.slot(index) {
                *input = value.to_owned();
            }
        }
        self.nebula_backup_remote_secret_set =
            crate::backup_remote::protocol_secret_set(cfg.protocol);
        self.nebula_backup_remote_focus = None;
    }

    /// 设置页下拉选择远程备份协议：持久化并按新协议重装输入槽。
    pub fn set_backup_protocol_option(&mut self, index: usize) {
        let Some(protocol) = settings::BACKUP_PROTOCOL_OPTIONS.get(index).copied() else { return };
        self.commit_backup_remote_field();
        let mut cfg = crate::backup_remote::BackupRemoteConfig::load();
        cfg.protocol = protocol;
        if let Err(err) = cfg.save() {
            self.nebula_backup_status = Some((err, true));
            self.nebula_backup_status_remote = true;
        }
        self.load_backup_remote_state();
        self.close_settings_dropdown();
        self.pending_update.dirty = true;
    }

    /// 聚焦某个远程备份输入框；先提交上一个（点击切换即失焦保存）。
    pub fn focus_backup_remote_field(&mut self, index: usize) {
        if self.nebula_backup_remote_focus == Some(index) {
            return;
        }
        self.commit_backup_remote_field();
        let count = crate::backup_remote::field_count(self.nebula_backup_protocol);
        if count == 0 {
            return;
        }
        self.nebula_backup_remote_focus = Some(index.min(count - 1));
        self.pending_update.dirty = true;
    }

    pub fn backup_remote_field_push(&mut self, ch: char) {
        let Some(index) = self.nebula_backup_remote_focus else { return };
        if ch.is_control() {
            return;
        }
        // 密文槽允许内部空格（trim 在保存侧）；其余槽拒绝空白——URL、
        // 路径、区域名里出现空格只会是误粘贴。
        let secret = crate::backup_remote::secret_field(self.nebula_backup_protocol) == Some(index);
        if ch.is_whitespace() && !secret {
            return;
        }
        if self.nebula_backup_remote_inputs[index].chars().count() < 512 {
            self.nebula_backup_remote_inputs[index].push(ch);
            self.pending_update.dirty = true;
        }
    }

    pub fn backup_remote_field_paste(&mut self, text: &str) {
        for ch in text.chars() {
            self.backup_remote_field_push(ch);
        }
    }

    pub fn backup_remote_field_backspace(&mut self) {
        let Some(index) = self.nebula_backup_remote_focus else { return };
        if self.nebula_backup_remote_inputs[index].pop().is_some() {
            self.pending_update.dirty = true;
        }
    }

    /// 失焦提交：普通槽写 `nebula_backup.txt`；密文槽若有输入则存入凭据
    /// 管理器并清空缓冲。
    pub fn commit_backup_remote_field(&mut self) {
        let Some(index) = self.nebula_backup_remote_focus.take() else { return };
        self.pending_update.dirty = true;
        let protocol = self.nebula_backup_protocol;
        if crate::backup_remote::secret_field(protocol) == Some(index) {
            let secret = std::mem::take(&mut self.nebula_backup_remote_inputs[index]);
            if secret.trim().is_empty() {
                return;
            }
            let result = match protocol {
                crate::backup_remote::BackupProtocol::WebDav => {
                    crate::backup_remote::store_webdav_password(
                        self.nebula_backup_remote_inputs[1].trim(),
                        &secret,
                    )
                },
                crate::backup_remote::BackupProtocol::S3 => crate::backup_remote::store_s3_secret(
                    self.nebula_backup_remote_inputs[3].trim(),
                    &secret,
                ),
                _ => return,
            };
            match result {
                Ok(()) => {
                    self.nebula_backup_remote_secret_set = true;
                },
                Err(err) => {
                    self.nebula_backup_status = Some((err, true));
                    self.nebula_backup_status_remote = true;
                },
            }
            return;
        }
        let mut cfg = crate::backup_remote::BackupRemoteConfig::load();
        cfg.protocol = protocol;
        if cfg.set_slot(index, self.nebula_backup_remote_inputs[index].trim().to_owned()) {
            if let Err(err) = cfg.save() {
                self.nebula_backup_status = Some((err, true));
                self.nebula_backup_status_remote = true;
            }
        }
    }

    /// Esc：丢弃当前草稿并失焦（还原为文件值；密文槽清空）。
    pub fn cancel_backup_remote_field(&mut self) {
        let Some(index) = self.nebula_backup_remote_focus.take() else { return };
        let cfg = crate::backup_remote::BackupRemoteConfig::load();
        self.nebula_backup_remote_inputs[index] =
            cfg.slot(index).map(str::to_owned).unwrap_or_default();
        self.pending_update.dirty = true;
    }

    /// 「备份到远程 / 从远程恢复」按钮：先行校验配置，通过则弹口令确认。
    /// 真正的网络动作等口令提交后由事件层在后台线程执行。
    pub fn start_backup_remote(&mut self, upload: bool) {
        if self.nebula_backup_busy {
            return;
        }
        self.commit_backup_remote_field();
        self.nebula_backup_status_remote = true;
        if upload && self.nebula_backup_selection.is_empty() {
            self.nebula_backup_status = Some((
                self.ui_language()
                    .pick("至少选择一项备份内容", "Select at least one backup item")
                    .to_owned(),
                true,
            ));
            self.window.request_redraw();
            return;
        }
        if let Err(err) = crate::backup_remote::validate() {
            self.nebula_backup_status = Some((err, true));
            self.window.request_redraw();
            return;
        }
        self.nebula_backup_operation =
            Some(if upload { BackupOperation::RemotePush } else { BackupOperation::RemotePull });
        self.nebula_backup_passphrase.clear();
        self.nebula_backup_passphrase_select_all.clear();
        self.nebula_backup_status = None;
        self.nebula_confirm = Some(NebulaConfirm::BackupPassphrase { restoring: !upload });
        self.window.request_redraw();
    }

    /// 后台远程备份线程收尾（`NebulaBackupRemoteDone`）。
    pub fn backup_remote_done(&mut self, message: &str, error: bool) {
        self.nebula_backup_busy = false;
        self.nebula_backup_status = Some((message.to_owned(), error));
        self.nebula_backup_status_remote = true;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn start_backup_export(&mut self) {
        if self.nebula_backup_selection.is_empty() {
            self.nebula_backup_status = Some((
                self.ui_language()
                    .pick("至少选择一项备份内容", "Select at least one backup item")
                    .to_owned(),
                true,
            ));
            self.nebula_backup_status_remote = false;
            self.window.request_redraw();
            return;
        }
        let Some(path) = file_dialog::save_backup_file(&self.window) else { return };
        self.nebula_backup_operation = Some(BackupOperation::Export(path));
        self.nebula_backup_passphrase.clear();
        self.nebula_backup_passphrase_select_all.clear();
        self.nebula_backup_status = None;
        self.nebula_confirm = Some(NebulaConfirm::BackupPassphrase { restoring: false });
        self.window.request_redraw();
    }

    pub fn start_backup_restore(&mut self) {
        let Some(path) = file_dialog::pick_backup_file(&self.window) else { return };
        self.nebula_backup_operation = Some(BackupOperation::Restore(path));
        self.nebula_backup_passphrase.clear();
        self.nebula_backup_passphrase_select_all.clear();
        self.nebula_backup_status = None;
        self.nebula_confirm = Some(NebulaConfirm::BackupPassphrase { restoring: true });
        self.window.request_redraw();
    }

    pub fn backup_passphrase_push(&mut self, character: char) {
        let replacing_selection = self.nebula_backup_passphrase_select_all.is_selected();
        if !character.is_control()
            && (replacing_selection || self.nebula_backup_passphrase.chars().count() < 256)
        {
            self.nebula_backup_passphrase_select_all
                .insert(&mut self.nebula_backup_passphrase, &character.to_string());
            self.nebula_backup_status = None;
        }
        self.window.request_redraw();
    }

    pub fn backup_passphrase_paste(&mut self, text: &str) {
        let replacing_selection = self.nebula_backup_passphrase_select_all.is_selected();
        let used =
            if replacing_selection { 0 } else { self.nebula_backup_passphrase.chars().count() };
        let incoming: String = text
            .chars()
            .filter(|character| !character.is_control())
            .take(256usize.saturating_sub(used))
            .collect();
        self.nebula_backup_passphrase_select_all
            .insert(&mut self.nebula_backup_passphrase, &incoming);
        self.nebula_backup_status = None;
        self.window.request_redraw();
    }

    pub fn backup_passphrase_backspace(&mut self) {
        self.nebula_backup_passphrase_select_all.backspace(&mut self.nebula_backup_passphrase);
        self.nebula_backup_status = None;
        self.window.request_redraw();
    }

    pub fn backup_passphrase_select_all(&mut self) {
        self.nebula_backup_passphrase_select_all.select(&self.nebula_backup_passphrase);
        self.window.request_redraw();
    }

    /// 口令确认。本地导出/恢复同步完成（小文件 + Argon2 一次派生）；远程
    /// 动作返回请求，由调用方发事件到后台线程——网络不进 UI 线程。
    pub fn complete_backup_operation(&mut self) -> Option<RemoteBackupRequest> {
        let Some(operation) = self.nebula_backup_operation.clone() else { return None };
        let passphrase = self.nebula_backup_passphrase.clone();
        let result = match operation {
            BackupOperation::Export(path) => {
                crate::encrypted_backup::collect(self.nebula_backup_selection)
                    .and_then(|archive| crate::encrypted_backup::seal(&archive, &passphrase))
                    .and_then(|packet| {
                        crate::atomic_file::write(&path, &packet).map_err(|error| error.to_string())
                    })
            },
            BackupOperation::Restore(path) => std::fs::read(&path)
                .map_err(|error| error.to_string())
                .and_then(|packet| crate::encrypted_backup::restore(&packet, &passphrase)),
            BackupOperation::RemotePush | BackupOperation::RemotePull => {
                self.nebula_backup_status_remote = true;
                if passphrase.chars().count() < 8 {
                    self.nebula_backup_status = Some((
                        self.ui_language()
                            .pick(
                                "备份操作失败: 口令至少 8 个字符",
                                "Backup operation failed: passphrase must be at least 8 characters",
                            )
                            .to_owned(),
                        true,
                    ));
                    self.nebula_backup_passphrase.clear();
                    self.nebula_backup_passphrase_select_all.clear();
                    self.window.request_redraw();
                    return None;
                }
                let upload = matches!(operation, BackupOperation::RemotePush);
                self.nebula_backup_busy = true;
                self.nebula_backup_status = Some((
                    self.ui_language()
                        .pick(
                            if upload {
                                "备份上传中…"
                            } else {
                                "正在取回远端备份…"
                            },
                            if upload {
                                "Uploading backup…"
                            } else {
                                "Fetching remote backup…"
                            },
                        )
                        .to_owned(),
                    false,
                ));
                self.nebula_confirm = None;
                self.nebula_backup_operation = None;
                self.nebula_backup_passphrase.clear();
                self.nebula_backup_passphrase_select_all.clear();
                self.window.request_redraw();
                return Some(RemoteBackupRequest {
                    upload,
                    passphrase,
                    selection: self.nebula_backup_selection,
                });
            },
        };
        self.nebula_backup_status_remote = false;
        match result {
            Ok(()) => {
                let restoring = matches!(
                    self.nebula_confirm,
                    Some(NebulaConfirm::BackupPassphrase { restoring: true })
                );
                self.nebula_backup_status = Some((
                    self.ui_language()
                        .pick(
                            if restoring {
                                "备份已恢复，重启后应用全部设置"
                            } else {
                                "备份已导出"
                            },
                            if restoring {
                                "Backup restored; restart to apply all settings"
                            } else {
                                "Backup exported"
                            },
                        )
                        .to_owned(),
                    false,
                ));
                self.nebula_confirm = None;
                self.nebula_backup_operation = None;
                self.nebula_backup_passphrase.clear();
                self.nebula_backup_passphrase_select_all.clear();
            },
            Err(error) => {
                self.nebula_backup_status = Some((
                    format!(
                        "{}: {error}",
                        self.ui_language().pick("备份操作失败", "Backup operation failed")
                    ),
                    true,
                ));
                self.nebula_backup_passphrase.clear();
                self.nebula_backup_passphrase_select_all.clear();
            },
        }
        self.window.request_redraw();
        None
    }

    pub fn cancel_backup_operation(&mut self) {
        self.nebula_confirm = None;
        self.nebula_backup_operation = None;
        self.nebula_backup_passphrase.clear();
        self.nebula_backup_passphrase_select_all.clear();
        self.window.request_redraw();
    }
}
