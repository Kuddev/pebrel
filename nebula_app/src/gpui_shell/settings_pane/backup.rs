//! 备份设置的草稿与操作生命周期；文件、协议和加密规则归业务层所有。
use super::*;
use crate::backup_remote::{self as remote, BackupProtocol, BackupRemoteConfig, Snapshot};
use crate::encrypted_backup::{
    self as archive, BackupArchive, BackupCategory, BackupSelection, CategorySummary,
};
use crate::i18n::Message;
use zeroize::Zeroizing;

mod drawer;
mod form;
mod operations;
mod setup;
#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
mod view;

#[derive(Default)]
struct BackupPassword(Option<Zeroizing<String>>);
impl gpui::Global for BackupPassword {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BackupSheet {
    Storage,
    Backup,
    Export,
    Restore,
    Password,
}

#[derive(Clone)]
enum RestoreSource {
    File(std::path::PathBuf),
    Remote(String),
}

#[derive(Default)]
pub(super) struct BackupUiState {
    initialized: bool,
    step: usize,
    draft: BackupRemoteConfig,
    tested: bool,
    draft_snapshots: Vec<Snapshot>,
    snapshots: Vec<Snapshot>,
    listing: bool,
    list_error: Option<String>,
    summary: Vec<CategorySummary>,
    summary_loading: bool,
    summary_error: Option<String>,
    confirm: Option<Entity<InputState>>,
    sheet: Option<BackupSheet>,
    sheet_focus: Option<FocusHandle>,
    previous_focus: Option<FocusHandle>,
    source: Option<RestoreSource>,
    opened: Option<BackupArchive>,
    restore_pass: Option<Zeroizing<String>>,
    undo: Option<(archive::recovery::RestorePoint, Zeroizing<String>)>,
    known: std::collections::HashMap<String, (usize, String)>,
    task: Option<Task<()>>,
}

fn recommended() -> BackupSelection {
    BackupSelection::from_categories([
        BackupCategory::Appearance,
        BackupCategory::Config,
        BackupCategory::Assistant,
        BackupCategory::Ssh,
        BackupCategory::Session,
    ])
}

impl SettingsPane {
    fn initialize_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_ui.initialized {
            return;
        }
        self.backup_ui.initialized = true;
        self.backup_ui.step = 1;
        self.backup_ui.confirm = Some(cx.new(|cx| InputState::new(window, cx).masked(true)));
        self.backup_ui.sheet_focus = Some(cx.focus_handle());
        self.backup_ui.draft = self.backup_remote.clone();
        if self.backup_remote.protocol == BackupProtocol::Off {
            self.backup_selection = recommended();
            self.backup_ui.draft.protocol = BackupProtocol::WebDav;
            self.backup_ui.draft.webdav_url = "https://dav.jianguoyun.com/dav/pebrel_backup".into();
        }
        self.fill_backup_fields(window, cx);
        for input in self
            .backup_remote_inputs
            .iter()
            .chain([&self.backup_secret_input])
            .cloned()
            .collect::<Vec<_>>()
        {
            self._subscriptions.push(cx.subscribe(&input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.backup_ui.tested = false;
                    cx.notify();
                }
            }));
        }
        if self.backup_remote.protocol != BackupProtocol::Off {
            self.refresh_backup_snapshots(cx);
        }
    }

    fn read_backup_fields(&mut self, cx: &App) {
        for (index, input) in self.backup_remote_inputs.iter().enumerate() {
            self.backup_ui.draft.set_slot(index, input.read(cx).value().trim().to_owned());
        }
    }

    fn fill_backup_fields(&self, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let fields = form::storage_fields(&self.backup_ui.draft);
        for (index, input) in self.backup_remote_inputs.iter().enumerate() {
            let value = self.backup_ui.draft.slot(index).unwrap_or_default().to_owned();
            let placeholder = fields
                .get(index)
                .filter(|field| !field.secret)
                .and_then(|field| field.placeholder)
                .map(|id| language.text(id))
                .unwrap_or("");
            input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx);
                input.set_value(value, window, cx);
            });
        }
        let secret_placeholder = fields
            .iter()
            .find(|field| field.secret)
            .and_then(|field| field.placeholder)
            .map(|id| language.text(id))
            .unwrap_or("");
        self.backup_secret_input
            .update(cx, |input, cx| input.set_placeholder(secret_placeholder, window, cx));
        self.backup_pass_input.update(cx, |input, cx| {
            input.set_placeholder(
                language.text(Message::BackupFlowNewPasswordPlaceholder),
                window,
                cx,
            )
        });
    }

    fn select_backup_protocol(
        &mut self,
        protocol: BackupProtocol,
        nutstore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.backup_busy {
            return;
        }
        self.read_backup_fields(cx);
        self.backup_ui.draft.protocol = protocol;
        if nutstore {
            self.backup_ui.draft.webdav_url = "https://dav.jianguoyun.com/dav/pebrel_backup".into();
        } else if protocol == BackupProtocol::WebDav
            && self.backup_ui.draft.webdav_url.starts_with("https://dav.jianguoyun.com/")
        {
            self.backup_ui.draft.webdav_url.clear();
        }
        if protocol == BackupProtocol::S3 && self.backup_ui.draft.s3_region.is_empty() {
            self.backup_ui.draft.s3_region = "auto".into();
        }
        self.fill_backup_fields(window, cx);
        self.backup_secret_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.backup_ui.tested = false;
        self.backup_status = None;
        cx.notify();
    }

    fn backup_task<T: Send + 'static>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.backup_seq = self.backup_seq.wrapping_add(1);
        let seq = self.backup_seq;
        self.backup_busy = true;
        self.backup_status = Some(BackupStatus::Processing);
        let work = cx.background_executor().spawn(work);
        self.backup_ui.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if seq != this.backup_seq {
                    return;
                }
                this.backup_busy = false;
                this.backup_status = None;
                match result {
                    Ok(value) => done(this, value, window, cx),
                    Err(error) => this.backup_status = Some(BackupStatus::Error(error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn refresh_backup_snapshots(&mut self, cx: &mut Context<Self>) {
        if self.backup_ui.listing || self.backup_remote.protocol == BackupProtocol::Off {
            return;
        }
        let config = self.backup_remote.clone();
        let original = config.clone();
        self.backup_ui.listing = true;
        self.backup_ui.list_error = None;
        let task =
            cx.background_executor().spawn(async move { remote::snapshot_details(&config, None) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.backup_ui.listing = false;
                if this.backup_remote != original {
                    this.refresh_backup_snapshots(cx);
                    return;
                }
                match result {
                    Ok(entries) => this.backup_ui.snapshots = entries,
                    Err(error) => this.backup_ui.list_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_backup_summary(&mut self, cx: &mut Context<Self>) {
        if self.backup_ui.summary_loading {
            return;
        }
        self.backup_ui.summary_loading = true;
        self.backup_ui.summary_error = None;
        let all = BackupSelection {
            appearance: true,
            config: true,
            ssh: true,
            sync: true,
            assistant: true,
            session: true,
            directory_history: true,
            command_history: true,
            fonts: true,
        };
        let task = cx
            .background_executor()
            .spawn(async move { archive::collect(all).map(|a| a.summary()) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.backup_ui.summary_loading = false;
                match result {
                    Ok(summary) => this.backup_ui.summary = summary,
                    Err(error) => this.backup_ui.summary_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn check_backup_connection(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        self.read_backup_fields(cx);
        let config = self.backup_ui.draft.clone();
        let secret = Zeroizing::new(self.backup_secret_input.read(cx).value().to_string());
        self.backup_ui.tested = false;
        self.backup_task(
            window,
            cx,
            async move {
                remote::snapshot_details(&config, (!secret.is_empty()).then_some(secret.as_str()))
            },
            move |this, entries, window, cx| {
                this.backup_ui.tested = true;
                this.backup_ui.draft_snapshots = entries;
                if save {
                    this.save_backup_storage(false, window, cx);
                } else if this.backup_ui.sheet.is_none() {
                    this.backup_ui.step = 3;
                    this.backup_pass_input.update(cx, |input, cx| input.focus(window, cx));
                }
            },
        );
    }

    fn next_backup_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        self.backup_status = None;
        match self.backup_ui.step {
            1 => {
                self.backup_ui.step = 2;
                self.backup_remote_inputs[0].update(cx, |input, cx| input.focus(window, cx));
            },
            2 => self.check_backup_connection(false, window, cx),
            3 => {
                let pass = self.backup_pass_input.read(cx).value();
                let confirm = self.backup_ui.confirm.as_ref().unwrap().read(cx).value();
                if pass.chars().count() < 8 {
                    self.backup_status = Some(BackupStatus::PassphraseTooShort);
                } else if pass != confirm {
                    self.backup_status = Some(BackupStatus::Error(
                        crate::gpui_shell::config::ui_language(cx)
                            .text(Message::BackupFlowMismatch)
                            .into(),
                    ));
                } else {
                    cx.set_global(BackupPassword(Some(Zeroizing::new(pass.to_string()))));
                    self.backup_ui.step = 4;
                    self.load_backup_summary(cx);
                }
            },
            _ => self.save_backup_storage(true, window, cx),
        }
        cx.notify();
    }

    fn save_backup_storage(
        &mut self,
        backup_now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.backup_busy || self.backup_selection.is_empty() {
            return;
        }
        if !self.backup_ui.tested {
            self.check_backup_connection(true, window, cx);
            return;
        }
        self.read_backup_fields(cx);
        let mut config = self.backup_ui.draft.clone();
        config.selection = self.backup_selection;
        let secret = Zeroizing::new(self.backup_secret_input.read(cx).value().to_string());
        self.backup_task(
            window,
            cx,
            async move {
                if !secret.is_empty() {
                    match config.protocol {
                        BackupProtocol::WebDav => {
                            remote::store_webdav_password(&config.webdav_username, &secret)?
                        },
                        BackupProtocol::S3 => {
                            remote::store_s3_secret(&config.s3_access_key, &secret)?
                        },
                        _ => {},
                    }
                }
                config.save()?;
                Ok(config)
            },
            move |this, config, window, cx| {
                this.backup_remote = config;
                this.backup_ui.snapshots = std::mem::take(&mut this.backup_ui.draft_snapshots);
                this.backup_ui.list_error = None;
                this.close_backup_sheet(window, cx);
                this.clear_backup_inputs(window, cx);
                this.backup_status = Some(BackupStatus::RemoteConfigSaved);
                if backup_now {
                    this.perform_backup(false, window, cx);
                }
            },
        );
    }

    fn backup_passphrase(&mut self, cx: &mut Context<Self>) -> Option<Zeroizing<String>> {
        let input = self.backup_pass_input.read(cx).value();
        let pass = if input.is_empty() {
            cx.try_global::<BackupPassword>().and_then(|p| p.0.clone())
        } else {
            Some(Zeroizing::new(input.to_string()))
        };
        if pass.as_ref().is_none_or(|p| p.chars().count() < 8) {
            self.backup_status = Some(BackupStatus::PassphraseTooShort);
            cx.notify();
            return None;
        }
        pass
    }

    fn clear_backup_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [&self.backup_pass_input, &self.backup_secret_input]
            .into_iter()
            .chain(self.backup_ui.confirm.iter())
        {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    pub(super) fn reload_after_restore(&mut self, cx: &mut Context<Self>) {
        self.ssh_hosts = crate::gpui_shell::ssh_hosts::SshHostLists::load();
        let (runtime, settings) = crate::gpui_shell::config::Settings::load_current_snapshot(cx);
        self.runtime = runtime;
        cx.set_global(settings);
        cx.emit(SettingsPaneEvent::Changed);
        cx.notify();
    }
}
