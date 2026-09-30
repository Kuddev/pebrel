//! Native dialogs and background I/O for the host library.

use super::*;
use crate::ssh_profiles::SshProfiles;
use std::io::Read;

enum Completion {
    Cancelled,
    Reloaded(crate::gpui_shell::ssh_hosts::SshHostLists),
    Imported {
        profiles: SshProfiles,
        added: usize,
        skipped: usize,
        restored: Vec<String>,
        visibility_error: Option<String>,
    },
    Exported,
    Duplicated {
        profiles: SshProfiles,
        identity: String,
        label: String,
    },
}

impl SettingsPane {
    fn begin_host_exchange(&mut self, cx: &mut Context<Self>) -> Option<u64> {
        if self.ssh_library.busy || self.ssh_editor.is_some() {
            return None;
        }
        self.commit_pending_ssh_delete();
        self.ssh_library.sequence = self.ssh_library.sequence.wrapping_add(1);
        self.ssh_library.busy = true;
        self.ssh_status = Some(SshStatus::LibraryProcessing);
        cx.notify();
        Some(self.ssh_library.sequence)
    }

    fn finish_host_exchange(
        &mut self,
        sequence: u64,
        result: Result<Completion, String>,
        cx: &mut Context<Self>,
    ) {
        if self.ssh_library.sequence != sequence {
            return;
        }
        self.ssh_library.busy = false;
        match result {
            Ok(Completion::Cancelled) => self.ssh_status = None,
            Ok(Completion::Reloaded(hosts)) => {
                self.ssh_hosts = hosts;
                self.ssh_status = Some(SshStatus::LibraryReloaded);
                cx.emit(SettingsPaneEvent::Changed);
            },
            Ok(Completion::Imported { profiles, added, skipped, restored, visibility_error }) => {
                self.ssh_hosts.profiles = profiles;
                self.ssh_hosts.load_error = None;
                self.ssh_hosts.hidden.retain(|host| !restored.contains(host));
                self.ssh_status = Some(match visibility_error {
                    Some(error) => SshStatus::LibraryImportedPartial { added, error },
                    None => SshStatus::LibraryImported { added, skipped },
                });
                cx.emit(SettingsPaneEvent::Changed);
            },
            Ok(Completion::Exported) => self.ssh_status = Some(SshStatus::LibraryExported),
            Ok(Completion::Duplicated { profiles, identity, label }) => {
                self.ssh_hosts.profiles = profiles;
                self.ssh_hosts.load_error = None;
                self.ssh_library.scope = HostScope::All;
                if let Some(index) =
                    self.filtered_library_hosts(cx).iter().position(|host| host == &identity)
                {
                    self.ssh_library.scroll.scroll_to_item(index, gpui::ScrollStrategy::Top);
                }
                self.ssh_status = Some(SshStatus::Copied(label));
                cx.emit(SettingsPaneEvent::Changed);
            },
            Err(error) => self.ssh_status = Some(SshStatus::Error(error)),
        }
        cx.notify();
    }

    pub(super) fn duplicate_ssh_host(&mut self, host: String, cx: &mut Context<Self>) {
        let Some(sequence) = self.begin_host_exchange(cx) else { return };
        let profiles = self.ssh_hosts.profiles.clone();
        let occupied = self.ssh_hosts.merged();
        let load_error = self.ssh_hosts.load_error.clone();
        let task = cx.background_executor().spawn(async move {
            if let Some(error) = load_error {
                return Err(error);
            }
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
            let identity = format!("pebrel-host-{:032x}", u128::from_be_bytes(bytes));
            if occupied.contains(&identity) {
                return Err("Host identity already exists".into());
            }
            let path = crate::display::nebula_data_dir().join("ssh_profiles.json");
            let (profiles, label) = crate::ssh_profiles::duplication::save_duplicate(
                profiles,
                &path,
                &host,
                &identity,
                crate::ssh_credentials::load_generic_secret,
                crate::ssh_credentials::store_generic_secret,
                crate::ssh_credentials::delete_generic_secret,
            )
            .map_err(|error| error.to_string())?;
            Ok(Completion::Duplicated { profiles, identity, label })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.finish_host_exchange(sequence, result, cx));
        })
        .detach();
    }

    pub(super) fn reload_host_library(&mut self, cx: &mut Context<Self>) {
        let Some(sequence) = self.begin_host_exchange(cx) else {
            return;
        };
        let task = cx.background_executor().spawn(async {
            let hosts = crate::gpui_shell::ssh_hosts::SshHostLists::load();
            if let Some(error) = hosts.load_error.clone() {
                return Err(error);
            }
            Ok(Completion::Reloaded(hosts))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.finish_host_exchange(sequence, result, cx));
        })
        .detach();
    }

    pub(super) fn import_host_csv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sequence) = self.begin_host_exchange(cx) else {
            return;
        };
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.finish_host_exchange(sequence, Err(error.to_string()), cx)
                    });
                    return;
                },
                _ => None,
            };
            let Some(path) = path else {
                let _ = this.update(cx, |this, cx| {
                    this.finish_host_exchange(sequence, Ok(Completion::Cancelled), cx)
                });
                return;
            };
            let records = executor
                .spawn(async move {
                    let mut bytes = Vec::new();
                    std::fs::File::open(&path)
                        .map_err(|e| e.to_string())?
                        .take(8 * 1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                    crate::ssh_profiles::exchange::parse_csv(&text)
                })
                .await;
            let records = match records {
                Ok(records) => records,
                Err(error) => {
                    let _ = this
                        .update(cx, |this, cx| this.finish_host_exchange(sequence, Err(error), cx));
                    return;
                },
            };
            let review = this
                .update_in(cx, |this, window, cx| {
                    if this.ssh_library.sequence != sequence {
                        return None;
                    }
                    let language = crate::gpui_shell::config::ui_language(cx);
                    let added = records
                        .iter()
                        .filter(|record| !this.ssh_hosts.profiles.contains(&record.destination))
                        .count();
                    let detail = language.format(
                        Message::HostsImportPreview,
                        &[
                            ("added", &added.to_string()),
                            ("skipped", &(records.len() - added).to_string()),
                        ],
                    );
                    Some(window.prompt(
                        gpui::PromptLevel::Info,
                        language.text(Message::HostsImport),
                        Some(&detail),
                        &[
                            language.text(Message::TransferCancel),
                            language.text(Message::HostsImport),
                        ],
                        cx,
                    ))
                })
                .ok()
                .flatten();
            let Some(review) = review else {
                return;
            };
            if review.await != Ok(1) {
                let _ = this.update(cx, |this, cx| {
                    this.finish_host_exchange(sequence, Ok(Completion::Cancelled), cx)
                });
                return;
            }
            let result = executor
                .spawn(async move {
                    let path = crate::display::nebula_data_dir().join("ssh_profiles.json");
                    let mut profiles = SshProfiles::load(&path).map_err(|e| e.to_string())?;
                    let mut restored: Vec<_> =
                        records.iter().map(|record| record.destination.clone()).collect();
                    let added = profiles.import_missing(&records)?;
                    profiles.save(&path).map_err(|e| e.to_string())?;
                    let raw = nebula_settings::RawSettings::load();
                    let old_hidden = raw.value("hidden_hosts").unwrap_or_default();
                    let hidden = old_hidden
                        .split(',')
                        .map(str::trim)
                        .filter(|host| !restored.iter().any(|restored| restored == host))
                        .collect::<Vec<_>>()
                        .join(",");
                    let visibility_error = if hidden != old_hidden {
                        nebula_settings::persist_keys(&[("hidden_hosts", hidden)])
                            .err()
                            .map(|error| error.to_string())
                    } else {
                        None
                    };
                    if visibility_error.is_some() {
                        restored.clear();
                    }
                    Ok(Completion::Imported {
                        profiles,
                        added,
                        skipped: records.len() - added,
                        restored,
                        visibility_error,
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| this.finish_host_exchange(sequence, result, cx));
        })
        .detach();
    }

    pub(super) fn export_host_csv(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(error) = self.ssh_hosts.load_error.clone() {
            self.ssh_status = Some(SshStatus::Error(error));
            cx.notify();
            return;
        }
        let Some(sequence) = self.begin_host_exchange(cx) else {
            return;
        };
        let directory = std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let picked = cx.prompt_for_new_path(&directory, Some("pebrel-hosts.csv"));
        let profiles = self.ssh_hosts.profiles.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = match picked.await {
                Ok(Ok(Some(path))) => {
                    executor
                        .spawn(async move {
                            crate::atomic_file::write(&path, profiles.export_csv().as_bytes())
                                .map_err(|e| e.to_string())?;
                            Ok(Completion::Exported)
                        })
                        .await
                },
                Ok(Err(error)) => Err(error.to_string()),
                _ => Ok(Completion::Cancelled),
            };
            let _ = this.update(cx, |this, cx| this.finish_host_exchange(sequence, result, cx));
        })
        .detach();
    }
}
