//! Backup operations and view-owned state; storage and encryption have one authority.
use super::*;
use crate::backup_remote::{self as remote, BackupProtocol, preferences::ConfigWriter};
use crate::i18n::Message;

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
mod view;

#[derive(Default)]
pub(super) struct BackupUiState {
    initialized: bool,
    configuration: bool,
    scope_open: bool,
    writer: ConfigWriter,
    save_revision: u64,
    save_result: Option<Result<(), String>>,
    save_watch: Option<Task<()>>,
    list_revision: u64,
    listing: bool,
    snapshots: Vec<String>,
    checked: bool,
    list_error: Option<String>,
    secret_ready: Option<bool>,
    secret_busy: bool,
    secret_revision: u64,
}

enum RestoreSource {
    File(std::path::PathBuf),
    Remote(Option<String>),
}

impl SettingsPane {
    fn initialize_backup(&mut self, cx: &mut Context<Self>) {
        if self.backup_ui.initialized {
            return;
        }
        self.backup_ui.initialized = true;
        for input in self.backup_remote_inputs.clone() {
            self._subscriptions.push(cx.subscribe(&input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let old = this.backup_remote.clone();
                    this.read_backup_fields(cx);
                    if this.backup_remote != old {
                        this.queue_backup_save(cx);
                    }
                }
            }));
        }
        self.read_backup_secret_status(cx);
        self.backup_ui.configuration = self.backup_remote.protocol == BackupProtocol::Off;
    }

    fn read_backup_fields(&mut self, cx: &App) {
        for (index, input) in self.backup_remote_inputs.iter().enumerate() {
            self.backup_remote.set_slot(index, input.read(cx).value().trim().to_owned());
        }
        self.backup_remote.selection = self.backup_selection;
    }

    fn queue_backup_save(&mut self, cx: &mut Context<Self>) {
        self.backup_remote.selection = self.backup_selection;
        self.backup_ui.checked = false;
        self.backup_ui.snapshots.clear();
        self.backup_ui.list_error = None;
        self.backup_ui.list_revision = self.backup_ui.list_revision.wrapping_add(1);
        self.backup_ui.listing = false;
        self.backup_ui.save_result = None;
        let writer = self.backup_ui.writer.clone();
        let revision = writer.submit(self.backup_remote.clone());
        self.backup_ui.save_revision = revision;
        // The observer can stop with the view; the writer still persists the last edit.
        self.backup_ui.save_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                if let Some(result) = writer.result(revision) {
                    let _ = this.update(cx, |this, cx| {
                        if this.backup_ui.save_revision == revision {
                            this.backup_ui.save_result = Some(result);
                            cx.notify();
                        }
                    });
                    break;
                }
                if this.upgrade().is_none() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn read_backup_secret_status(&mut self, cx: &mut Context<Self>) {
        let protocol = self.backup_remote.protocol;
        self.backup_ui.secret_revision = self.backup_ui.secret_revision.wrapping_add(1);
        let revision = self.backup_ui.secret_revision;
        self.backup_ui.secret_ready = None;
        let task =
            cx.background_executor().spawn(async move { remote::protocol_secret_set(protocol) });
        cx.spawn(async move |this, cx| {
            let ready = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.backup_ui.secret_revision == revision
                    && this.backup_remote.protocol == protocol
                {
                    this.backup_ui.secret_ready = Some(ready);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_backup_protocol(
        &mut self,
        protocol: BackupProtocol,
        nutstore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.backup_busy || self.backup_ui.secret_busy {
            return;
        }
        self.read_backup_fields(cx);
        self.backup_remote.protocol = protocol;
        if nutstore {
            self.backup_remote.webdav_url = "https://dav.jianguoyun.com/dav/pebrel_backup".into();
        }
        for (index, input) in self.backup_remote_inputs.iter().enumerate() {
            let value = self.backup_remote.slot(index).unwrap_or_default().to_owned();
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.backup_secret_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.queue_backup_save(cx);
        self.read_backup_secret_status(cx);
    }

    fn store_remote_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_ui.secret_busy || self.backup_busy {
            return;
        }
        let secret = zeroize::Zeroizing::new(self.backup_secret_input.read(cx).value().to_string());
        if secret.is_empty() {
            self.backup_status = Some(BackupStatus::CredentialEmpty);
            cx.notify();
            return;
        }
        let config = self.backup_remote.clone();
        self.backup_ui.secret_busy = true;
        self.backup_ui.secret_revision = self.backup_ui.secret_revision.wrapping_add(1);
        let task = cx.background_executor().spawn(async move {
            match config.protocol {
                BackupProtocol::WebDav => {
                    remote::store_webdav_password(&config.webdav_username, &secret)
                },
                BackupProtocol::S3 => remote::store_s3_secret(&config.s3_access_key, &secret),
                _ => Err("No separate credential is required".into()),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.backup_ui.secret_busy = false;
                match result {
                    Ok(()) => {
                        this.backup_secret_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.backup_ui.secret_ready = Some(true);
                        this.backup_status = Some(BackupStatus::CredentialSaved);
                    },
                    Err(error) => this.backup_status = Some(BackupStatus::Error(error)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_backup_snapshots(&mut self, cx: &mut Context<Self>) {
        if self.backup_ui.listing || self.backup_remote.protocol == BackupProtocol::Off {
            return;
        }
        self.backup_ui.list_revision = self.backup_ui.list_revision.wrapping_add(1);
        let revision = self.backup_ui.list_revision;
        let config = self.backup_remote.clone();
        self.backup_ui.listing = true;
        self.backup_ui.list_error = None;
        let task = cx.background_executor().spawn(async move { remote::snapshots(&config) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.backup_ui.list_revision != revision {
                    return;
                }
                this.backup_ui.listing = false;
                this.backup_ui.checked = result.is_ok();
                match result {
                    Ok(names) => this.backup_ui.snapshots = names,
                    Err(error) => {
                        this.backup_ui.snapshots.clear();
                        this.backup_ui.list_error = Some(error);
                    },
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn backup_passphrase(&mut self, cx: &mut Context<Self>) -> Option<zeroize::Zeroizing<String>> {
        let pass = self.backup_pass_input.read(cx).value().to_string();
        if pass.chars().count() < 8 {
            self.backup_status = Some(BackupStatus::PassphraseTooShort);
            self.backup_ui.configuration = true;
            cx.notify();
            return None;
        }
        Some(zeroize::Zeroizing::new(pass))
    }

    fn backup_run_async(
        &mut self,
        task: impl std::future::Future<Output = Result<BackupCompletion, String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.backup_seq = self.backup_seq.wrapping_add(1);
        let seq = self.backup_seq;
        self.backup_busy = true;
        self.backup_status = Some(BackupStatus::Processing);
        let task = cx.background_executor().spawn(task);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |pane, cx| {
                if seq != pane.backup_seq {
                    return;
                }
                pane.backup_busy = false;
                if matches!(result, Ok(BackupCompletion::Restored | BackupCompletion::Pulled(_))) {
                    pane.reload_after_restore(cx);
                }
                let refresh =
                    matches!(result, Ok(BackupCompletion::Pushed(_) | BackupCompletion::Pulled(_)));
                pane.backup_status = Some(match result {
                    Ok(completion) => BackupStatus::Completed(completion),
                    Err(error) => BackupStatus::Error(error),
                });
                if refresh {
                    pane.refresh_backup_snapshots(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn push_remote(&mut self, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        let Some(pass) = self.backup_passphrase(cx) else { return };
        if self.backup_selection.is_empty() {
            self.backup_status = Some(BackupStatus::SelectionRequired);
            cx.notify();
            return;
        }
        let selection = self.backup_selection;
        let config = self.backup_remote.clone();
        self.backup_run_async(
            async move {
                let archive = crate::encrypted_backup::collect(selection)?;
                let packet = crate::encrypted_backup::seal(&archive, &pass)?;
                remote::push_to(&config, &packet).map(BackupCompletion::Pushed)
            },
            cx,
        );
    }

    fn confirm_backup_restore(
        &mut self,
        source: RestoreSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.backup_busy {
            return;
        }
        let Some(pass) = self.backup_passphrase(cx) else { return };
        let language = crate::gpui_shell::config::ui_language(cx);
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            language.text(Message::CloudRestore),
            Some(language.text(Message::CloudRestoreWarning)),
            &[language.text(Message::CommonCancel), language.text(Message::CloudRestore)],
            cx,
        );
        let config = self.backup_remote.clone();
        self.backup_busy = true;
        cx.spawn(async move |this, cx| {
            let accepted = matches!(prompt.await, Ok(1));
            let _ = this.update(cx, |pane, cx| {
                pane.backup_busy = false;
                if accepted {
                    pane.backup_run_async(
                        async move {
                            let (name, packet) = match source {
                                RestoreSource::File(path) => {
                                    (None, std::fs::read(path).map_err(|error| error.to_string())?)
                                },
                                RestoreSource::Remote(name) => {
                                    let (name, packet) =
                                        remote::pull_from(&config, name.as_deref())?;
                                    (Some(name), packet)
                                },
                            };
                            crate::encrypted_backup::restore(&packet, &pass)?;
                            Ok(name
                                .map(BackupCompletion::Pulled)
                                .unwrap_or(BackupCompletion::Restored))
                        },
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn export_backup(&mut self, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        let Some(pass) = self.backup_passphrase(cx) else { return };
        let selection = self.backup_selection;
        if selection.is_empty() {
            self.backup_status = Some(BackupStatus::SelectionRequired);
            cx.notify();
            return;
        }
        let picked = cx
            .prompt_for_new_path(&crate::display::nebula_data_dir(), Some("pebrel.pebrel-backup"));
        self.backup_busy = true;
        cx.spawn(async move |this, cx| {
            let path = picked.await;
            let _ = this.update(cx, |pane, cx| {
                pane.backup_busy = false;
                if let Ok(Ok(Some(path))) = path {
                    pane.backup_run_async(
                        async move {
                            let archive = crate::encrypted_backup::collect(selection)?;
                            let packet = crate::encrypted_backup::seal(&archive, &pass)?;
                            std::fs::write(&path, packet).map_err(|error| error.to_string())?;
                            Ok(BackupCompletion::Exported(path))
                        },
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn restore_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        self.backup_busy = true;
        cx.spawn_in(window, async move |this, cx| {
            let paths = picked.await;
            let _ = this.update_in(cx, |pane, window, cx| {
                pane.backup_busy = false;
                if let Ok(Ok(Some(paths))) = paths {
                    if let Some(path) = paths.into_iter().next() {
                        pane.confirm_backup_restore(RestoreSource::File(path), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
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
