//! Compact backup dashboard and configuration card based on the approved layout.
use super::*;
use crate::gpui_shell::widgets::{NebulaSwitch, device_icon_anchor};
use gpui_component::menu::PopupMenuItem;

fn card(cx: &App) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w_0()
        .gap_4()
        .p_5()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.opacity(0.3))
}

fn provider(config: &remote::BackupRemoteConfig) -> Message {
    match config.protocol {
        BackupProtocol::Off => Message::CloudOff,
        BackupProtocol::Folder => Message::CloudFolder,
        BackupProtocol::WebDav if config.webdav_url.starts_with("https://dav.jianguoyun.com/") => {
            Message::CloudNutstore
        },
        BackupProtocol::WebDav => Message::CloudWebdav,
        BackupProtocol::S3 => Message::CloudS3,
        BackupProtocol::Sftp => Message::CloudSftp,
    }
}

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn section_backup(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.initialize_backup(cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let configuration = self.backup_ui.configuration;
        let tabs = h_flex()
            .gap_1()
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .children(
                [(false, Message::CloudSnapshots), (true, Message::CloudSettings)].into_iter().map(
                    |(selected, title)| {
                        Button::new(if selected {
                            "cloud-tab-settings"
                        } else {
                            "cloud-tab-snapshots"
                        })
                        .debug_selector(move || {
                            if selected { "cloud-tab-settings" } else { "cloud-tab-snapshots" }
                                .into()
                        })
                        .label(language.text(title))
                        .small()
                        .ghost()
                        .selected(configuration == selected)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.backup_ui.configuration = selected;
                            if !selected && !this.backup_ui.checked {
                                this.refresh_backup_snapshots(cx);
                            }
                            cx.notify();
                        }))
                    },
                ),
            );
        let body =
            if configuration { self.backup_configuration(cx) } else { self.backup_dashboard(cx) };
        v_flex()
            .w_full()
            .gap_5()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(20.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::CloudTitle)),
                    )
                    .child(tabs),
            )
            .child(body)
    }

    fn backup_status_view(&self, cx: &App) -> Option<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        self.backup_status.as_ref().map(|status| {
            div()
                .text_sm()
                .text_color(if status.is_error() {
                    cx.theme().danger
                } else {
                    cx.theme().muted_foreground
                })
                .child(status.text(language))
        })
    }

    fn backup_connection_state(&self) -> Message {
        if self.backup_ui.listing {
            Message::CloudChecking
        } else if self.backup_ui.list_error.is_some() {
            Message::CloudUnreachable
        } else if self.backup_ui.checked {
            Message::CloudReachable
        } else {
            Message::CloudNotChecked
        }
    }

    fn backup_dashboard(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let off = self.backup_remote.protocol == BackupProtocol::Off;
        let busy = self.backup_busy || self.backup_ui.secret_busy;
        let mut history = card(cx).child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(language.text(Message::CloudSnapshots)),
                        )
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                            language.format(
                                Message::CloudRetention,
                                &[("count", &remote::KEEP_ARCHIVES.to_string())],
                            ),
                        )),
                )
                .child(
                    Button::new("cloud-refresh")
                        .ghost()
                        .icon(Icon::default().path(crate::gpui_shell::assets::nav::REFRESH))
                        .tooltip(language.text(Message::CloudRefresh))
                        .disabled(off || self.backup_ui.listing || busy)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_backup_snapshots(cx))),
                ),
        );
        if self.backup_ui.snapshots.is_empty() {
            history = history.child(
                div().py_5().text_sm().text_color(cx.theme().muted_foreground).child(
                    language.text(if self.backup_ui.checked {
                        Message::CloudEmpty
                    } else {
                        Message::CloudLoadHistory
                    }),
                ),
            );
        }
        for (index, name) in self.backup_ui.snapshots.iter().enumerate() {
            let target = name.clone();
            history = history.child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .py_3()
                    .gap_3()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .size(px(8.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(cx.theme().muted_foreground),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().truncate().text_sm().child(name.clone()))
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                                language.text(if index == 0 {
                                    Message::CloudLatest
                                } else {
                                    Message::CloudEncryptedArchive
                                }),
                            )),
                    )
                    .child(
                        Button::new(("cloud-restore-version", index))
                            .label(language.text(Message::CloudRestore))
                            .small()
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_backup_restore(
                                    RestoreSource::Remote(Some(target.clone())),
                                    window,
                                    cx,
                                );
                            })),
                    ),
            );
        }
        let connection = self.backup_connection_state();
        v_flex()
            .w_full()
            .gap_5()
            .child(
                card(cx)
                    .child(
                        h_flex()
                            .gap_4()
                            .items_center()
                            .flex_wrap()
                            .child(device_icon_anchor(IconName::Globe, cx))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(220.0))
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .flex_wrap()
                                            .child(
                                                div()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .child(
                                                        language
                                                            .text(provider(&self.backup_remote)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(language.text(connection)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(language.text(Message::CloudEncryptedArchive)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Button::new("cloud-pull")
                                            .label(language.text(Message::CloudRestoreLatest))
                                            .small()
                                            .disabled(off || busy)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.confirm_backup_restore(
                                                    RestoreSource::Remote(None),
                                                    window,
                                                    cx,
                                                )
                                            })),
                                    )
                                    .child(
                                        Button::new("cloud-push")
                                            .label(language.text(Message::CloudBackupNow))
                                            .small()
                                            .primary()
                                            .disabled(off || busy)
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.push_remote(cx)),
                                            ),
                                    ),
                            ),
                    )
                    .children(self.backup_status_view(cx))
                    .children(self.backup_ui.list_error.as_ref().map(|error| {
                        div().text_sm().text_color(cx.theme().danger).child(error.clone())
                    })),
            )
            .child(history)
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("cloud-export-file")
                            .label(language.text(Message::CloudExport))
                            .small()
                            .ghost()
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.export_backup(cx))),
                    )
                    .child(
                        Button::new("cloud-import-file")
                            .label(language.text(Message::CloudImport))
                            .small()
                            .ghost()
                            .disabled(busy)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.restore_backup(window, cx)),
                            ),
                    ),
            )
    }

    fn backup_configuration(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let busy = self.backup_busy || self.backup_ui.secret_busy;
        let owner = cx.entity().downgrade();
        let provider_menu = Button::new("cloud-provider")
            .label(language.text(provider(&self.backup_remote)))
            .disabled(busy)
            .dropdown_menu(move |mut menu, _, _| {
                for (protocol, nutstore, label) in [
                    (BackupProtocol::WebDav, true, Message::CloudNutstore),
                    (BackupProtocol::WebDav, false, Message::CloudWebdav),
                    (BackupProtocol::S3, false, Message::CloudS3),
                    (BackupProtocol::Sftp, false, Message::CloudSftp),
                    (BackupProtocol::Folder, false, Message::CloudFolder),
                    (BackupProtocol::Off, false, Message::CloudOff),
                ] {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(language.text(label)).on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |pane, cx| {
                                pane.select_backup_protocol(protocol, nutstore, window, cx)
                            });
                        },
                    ));
                }
                menu
            });
        let mut storage = card(cx)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(device_icon_anchor(IconName::Globe, cx))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::CloudConfigure)),
                    )
                    .child(
                        Button::new("cloud-scope-toggle")
                            .debug_selector(|| "cloud-scope-toggle".into())
                            .label(language.text(Message::CloudScope))
                            .small()
                            .selected(self.backup_ui.scope_open)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.backup_ui.scope_open = !this.backup_ui.scope_open;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text(Message::CloudProvider)),
                    )
                    .child(provider_menu),
            );
        let fields: &[Message] = match self.backup_remote.protocol {
            BackupProtocol::Off => &[],
            BackupProtocol::Folder => &[Message::CloudFolderPath],
            BackupProtocol::WebDav => &[Message::CloudAddress, Message::CloudUsername],
            BackupProtocol::S3 => &[
                Message::CloudS3Address,
                Message::CloudRegion,
                Message::CloudBucket,
                Message::CloudAccessKey,
            ],
            BackupProtocol::Sftp => &[Message::CloudSshHost, Message::CloudRemotePath],
        };
        for (index, label) in fields.iter().enumerate() {
            storage = storage.child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text(*label)),
                    )
                    .child(Input::new(&self.backup_remote_inputs[index]).disabled(busy)),
            );
        }
        if matches!(self.backup_remote.protocol, BackupProtocol::WebDav | BackupProtocol::S3) {
            storage = storage.child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_xs().child(language.text(Message::CloudCredential)))
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                                language.text(if self.backup_ui.secret_ready == Some(true) {
                                    Message::CloudCredentialSet
                                } else {
                                    Message::CloudCredentialNeeded
                                }),
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(180.0))
                                    .child(Input::new(&self.backup_secret_input).disabled(busy)),
                            )
                            .child(
                                Button::new("cloud-store-secret")
                                    .label(language.text(Message::CloudStoreCredential))
                                    .small()
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.store_remote_secret(window, cx)
                                    })),
                            ),
                    ),
            );
        }
        if self.backup_ui.scope_open {
            storage = storage.child(self.backup_scope(cx));
        }
        let save_label = match &self.backup_ui.save_result {
            Some(Ok(())) => Message::CloudSaved,
            Some(Err(_)) => Message::CloudSaveFailed,
            None if self.backup_ui.save_revision > 0 => Message::CloudSaving,
            None => Message::CloudAutoSave,
        };
        storage =
            storage
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .flex_wrap()
                        .pt_3()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            Button::new("cloud-test")
                                .label(language.text(Message::CloudTest))
                                .small()
                                .disabled(
                                    busy || self.backup_ui.listing
                                        || self.backup_remote.protocol == BackupProtocol::Off,
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.refresh_backup_snapshots(cx)),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(language.text(self.backup_connection_state())),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("cloud-save-status")
                                .debug_selector(|| "cloud-save-status".into())
                                .text_xs()
                                .text_color(if matches!(self.backup_ui.save_result, Some(Err(_))) {
                                    cx.theme().danger
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .child(language.text(save_label)),
                        ),
                )
                .when(matches!(self.backup_ui.save_result, Some(Err(_))), |card| {
                    card.child(
                        Button::new("cloud-retry-save")
                            .label(language.text(Message::CloudRetrySave))
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| this.queue_backup_save(cx))),
                    )
                })
                .children(self.backup_status_view(cx))
                .children(self.backup_ui.list_error.as_ref().map(|error| {
                    div().text_sm().text_color(cx.theme().danger).child(error.clone())
                }));
        v_flex()
            .w_full()
            .gap_5()
            .child(
                div()
                    .id("cloud-storage-card")
                    .debug_selector(|| "cloud-storage-card".into())
                    .child(storage),
            )
            .child(
                card(cx)
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::CloudPassphrase)),
                    )
                    .child(Input::new(&self.backup_pass_input).disabled(busy))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text(Message::CloudPassphraseHint)),
                    ),
            )
    }

    fn backup_scope(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let selection = self.backup_selection;
        let categories: [(Message, bool, fn(&mut crate::encrypted_backup::BackupSelection, bool));
            9] = [
            (Message::CloudAppearance, selection.appearance, |s, v| s.appearance = v),
            (Message::CloudTerminal, selection.config, |s, v| s.config = v),
            (Message::CloudHosts, selection.ssh, |s, v| s.ssh = v),
            (Message::CloudSync, selection.sync, |s, v| s.sync = v),
            (Message::CloudAssistant, selection.assistant, |s, v| s.assistant = v),
            (Message::CloudSessions, selection.session, |s, v| s.session = v),
            (Message::CloudDirectories, selection.directory_history, |s, v| {
                s.directory_history = v
            }),
            (Message::CloudCommands, selection.command_history, |s, v| s.command_history = v),
            (Message::CloudFonts, selection.fonts, |s, v| s.fonts = v),
        ];
        v_flex()
            .w_full()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .debug_selector(|| "cloud-scope-options".into())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::CloudScopeHint)),
            )
            .children(categories.into_iter().enumerate().map(|(index, (label, checked, apply))| {
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .py_1()
                    .child(div().flex_1().text_sm().child(language.text(label)))
                    .child(
                        NebulaSwitch::new(format!("cloud-scope-{index}"))
                            .checked(checked)
                            .disabled(self.backup_busy)
                            .on_click(cx.listener(move |this, enabled: &bool, _, cx| {
                                apply(&mut this.backup_selection, *enabled);
                                this.queue_backup_save(cx);
                            })),
                    )
            }))
    }
}
