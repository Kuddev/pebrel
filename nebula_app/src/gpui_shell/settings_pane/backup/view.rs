//! 原型的阅读顺序：状态 → 存储 → 时间线；低频操作放在面板下面。
use super::*;

pub(super) fn panel(cx: &App) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .rounded(px(10.0))
        .border_1()
        .border_color(crate::gpui_shell::theme::settings_hairline(cx))
        .overflow_hidden()
}

pub(super) fn caption(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .text_size(px(12.5))
        .line_height(gpui::relative(1.6))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub(super) fn provider(config: &BackupRemoteConfig) -> Message {
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

pub(super) fn size_text(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn backup_time(name: &str) -> String {
    name.strip_prefix("pebrel-backup-")
        .and_then(|s| s.strip_suffix(".nbk"))
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d-%H%M%S").ok())
        .map(|time| time.and_utc().with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| name.to_owned())
}

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn section_backup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.initialize_backup(window, cx);
        let l = crate::gpui_shell::config::ui_language(cx);
        let off = self.backup_remote.protocol == BackupProtocol::Off;
        v_flex()
            .w_full()
            .flex_shrink_0()
            .max_w(px(700.0))
            .gap_6()
            .text_size(px(14.0))
            .child(
                v_flex()
                    .gap_2()
                    .mb_1()
                    .child(
                        div()
                            .text_size(px(22.0))
                            .font_semibold()
                            .child(l.text(Message::BackupFlowTitle)),
                    )
                    .child(caption(l.text(Message::BackupFlowIntro), cx)),
            )
            .when(self.backup_ui.undo.is_some(), |d| {
                d.child(
                    h_flex()
                        .gap_3()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().muted)
                        .flex_wrap()
                        .child(div().flex_1().child(l.text(Message::BackupFlowRestored)))
                        .child(
                            Button::new("backup-undo")
                                .label(l.text(Message::BackupFlowUndo))
                                .ghost()
                                .small()
                                .disabled(self.backup_busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.undo_backup_restore(window, cx)
                                })),
                        )
                        .child(
                            Button::new("backup-dismiss-undo")
                                .icon(IconName::Close)
                                .ghost()
                                .small()
                                .disabled(self.backup_busy)
                                .tooltip(l.text(Message::CommonClose))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.backup_ui.undo = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(if off { self.backup_wizard(window, cx) } else { self.backup_dashboard(cx) })
            .when(!off, |d| d.children(self.backup_status_view(cx)))
            .when(!off, |d| d.child(self.backup_encryption_row(cx)))
            .when(!off, |d| d.child(self.backup_file_rows(cx)))
    }

    pub(super) fn backup_status_view(&self, cx: &App) -> Option<gpui::Div> {
        let l = crate::gpui_shell::config::ui_language(cx);
        self.backup_status.as_ref().map(|status| {
            div()
                .text_size(px(13.0))
                .text_color(if status.is_error() {
                    cx.theme().danger
                } else {
                    cx.theme().muted_foreground
                })
                .child(status.text(l))
        })
    }

    fn snapshot_meta(&self, entry: &Snapshot, cx: &App) -> String {
        let l = crate::gpui_shell::config::ui_language(cx);
        let size = entry.bytes.map(size_text).unwrap_or_else(|| "—".into());
        match self.backup_ui.known.get(&entry.name) {
            Some((count, device)) => l.format(
                Message::BackupFlowSnapshotMeta,
                &[("count", &count.to_string()), ("size", &size), ("device", device)],
            ),
            None => format!("{size} · {}", l.text(Message::CloudEncryptedArchive)),
        }
    }

    fn backup_dashboard(&self, cx: &mut Context<Self>) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        let latest = self.backup_ui.snapshots.first();
        let mut head = v_flex().flex_1().min_w_0().gap_2();
        if let Some(entry) = latest {
            head = head
                .child(caption(l.text(Message::BackupFlowLastBackup), cx))
                .child(div().text_size(px(28.0)).font_semibold().child(backup_time(&entry.name)))
                .child(caption(self.snapshot_meta(entry, cx), cx));
        } else {
            head = head
                .child(div().text_size(px(18.0)).font_semibold().child(l.text(
                    if self.backup_ui.listing {
                        Message::CloudChecking
                    } else {
                        Message::BackupFlowEmpty
                    },
                )))
                .child(caption(l.text(Message::BackupFlowEmptyHint), cx));
        }
        let actions = h_flex()
            .gap_2()
            .flex_wrap()
            .when_some(latest, |row, entry| {
                let name = entry.name.clone();
                row.child(
                    Button::new("backup-restore-latest")
                        .label(l.text(Message::CloudRestore))
                        .small()
                        .disabled(self.backup_busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_backup_restore(
                                RestoreSource::Remote(name.clone()),
                                window,
                                cx,
                            )
                        })),
                )
            })
            .child(
                Button::new("backup-now")
                    .debug_selector(|| "backup-now".into())
                    .label(l.text(Message::CloudBackupNow))
                    .primary()
                    .small()
                    .h(px(34.0))
                    .disabled(
                        self.backup_busy
                            || self.backup_ui.listing
                            || self.backup_ui.list_error.is_some(),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_backup_sheet(BackupSheet::Backup, window, cx)
                    })),
            );
        let path = match self.backup_remote.protocol {
            BackupProtocol::Folder => self.backup_remote.folder_path.clone(),
            BackupProtocol::WebDav => self.backup_remote.webdav_url.clone(),
            BackupProtocol::S3 => {
                format!("{} / {}", self.backup_remote.s3_endpoint, self.backup_remote.s3_bucket)
            },
            BackupProtocol::Sftp => {
                format!("{}:{}", self.backup_remote.sftp_destination, self.backup_remote.sftp_path)
            },
            BackupProtocol::Off => String::new(),
        };
        let store = v_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .px_6()
            .py_3()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        Icon::default()
                            .path(super::setup::provider_icon(&self.backup_remote))
                            .size(px(16.0)),
                    )
                    .child(div().font_medium().child(l.text(provider(&self.backup_remote))))
                    .child(caption(path, cx).flex_1().min_w_0().truncate())
                    .child(
                        Button::new("backup-edit-storage")
                            .label(l.text(Message::BackupFlowChange))
                            .ghost()
                            .small()
                            .disabled(self.backup_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_backup_sheet(BackupSheet::Storage, window, cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(caption(
                        l.text(if self.backup_ui.listing {
                            Message::CloudChecking
                        } else if self.backup_ui.list_error.is_some() {
                            Message::BackupFlowConnectionFailed
                        } else {
                            Message::BackupFlowConnected
                        }),
                        cx,
                    ))
                    .child(
                        Button::new("backup-refresh")
                            .icon(IconName::Redo2)
                            .ghost()
                            .small()
                            .tooltip(l.text(Message::CloudTest))
                            .disabled(self.backup_busy || self.backup_ui.listing)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.refresh_backup_snapshots(cx)),
                            ),
                    ),
            )
            .children(self.backup_ui.list_error.as_ref().map(|error| {
                div()
                    .debug_selector(|| "backup-storage-error".into())
                    .w_full()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone())
            }));
        let mut history =
            v_flex().w_full().px_6().py_3().border_t_1().border_color(cx.theme().border);
        for (index, entry) in self.backup_ui.snapshots.iter().enumerate() {
            let name = entry.name.clone();
            let device = self
                .backup_ui
                .known
                .get(&name)
                .map(|(_, d)| d.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| l.text(Message::CloudEncryptedArchive).into());
            history = history.child(
                h_flex()
                    .w_full()
                    .min_h(px(42.0))
                    .gap_3()
                    .child(
                        div()
                            .relative()
                            .w(px(14.0))
                            .h(px(42.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(6.0))
                                    .w(px(1.0))
                                    .bg(cx.theme().border),
                            )
                            .child(
                                div()
                                    .relative()
                                    .size(px(8.0))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if index == 0 {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .bg(if index == 0 {
                                        cx.theme().primary
                                    } else {
                                        crate::gpui_shell::theme::settings_panel_bg(cx)
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_size(px(13.0)).child(backup_time(&name)))
                            .when(index == 0, |r| {
                                r.child(caption(l.text(Message::BackupFlowLatest), cx))
                            }),
                    )
                    .child(caption(device, cx).flex_1().min_w_0().truncate())
                    .child(caption(entry.bytes.map(size_text).unwrap_or_else(|| "—".into()), cx))
                    .child(
                        Button::new(("backup-restore", index))
                            .label(l.text(Message::CloudRestore))
                            .small()
                            .ghost()
                            .disabled(self.backup_busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_backup_restore(
                                    RestoreSource::Remote(name.clone()),
                                    window,
                                    cx,
                                )
                            })),
                    ),
            );
        }
        history =
            history.child(caption(l.text(Message::BackupFlowRetention), cx).pl(px(26.0)).py_3());
        panel(cx)
            .debug_selector(|| "backup-dashboard".into())
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .p_6()
                    .gap_4()
                    .flex_wrap()
                    .child(head)
                    .child(actions),
            )
            .child(store)
            .when(latest.is_some(), |d| d.child(history))
    }

    fn backup_encryption_row(&self, cx: &mut Context<Self>) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        let ready = cx.try_global::<BackupPassword>().is_some_and(|p| p.0.is_some());
        v_flex()
            .debug_selector(|| "backup-encryption".into())
            .w_full()
            .flex_shrink_0()
            .gap_3()
            .child(div().font_semibold().child(l.text(Message::BackupFlowEncryption)))
            .child(
                v_flex()
                    .w_full()
                    .flex_shrink_0()
                    .gap_2()
                    .py_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .justify_between()
                            .flex_wrap()
                            .child(div().child(l.text(Message::CloudPassphrase)))
                            .child(
                                h_flex()
                                    .flex_shrink_0()
                                    .gap_2()
                                    .child(caption(
                                        l.text(if ready {
                                            Message::BackupFlowPasswordReady
                                        } else {
                                            Message::BackupFlowPasswordMissing
                                        }),
                                        cx,
                                    ))
                                    .child(
                                        Button::new("backup-password")
                                            .debug_selector(|| "backup-password".into())
                                            .label(l.text(if ready {
                                                Message::BackupFlowClear
                                            } else {
                                                Message::BackupFlowEnter
                                            }))
                                            .small()
                                            .ghost()
                                            .disabled(self.backup_busy)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                if ready {
                                                    cx.set_global(BackupPassword::default());
                                                    this.clear_backup_inputs(window, cx);
                                                    cx.notify();
                                                } else {
                                                    this.open_backup_sheet(
                                                        BackupSheet::Password,
                                                        window,
                                                        cx,
                                                    );
                                                }
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        caption(l.text(Message::BackupFlowPasswordHint), cx)
                            .debug_selector(|| "backup-password-description".into())
                            .w_full()
                            .max_w(px(560.0))
                            .flex_shrink_0(),
                    ),
            )
    }

    fn backup_file_rows(&self, cx: &mut Context<Self>) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        v_flex()
            .gap_3()
            .child(div().font_semibold().child(l.text(Message::BackupFlowFiles)))
            .children(
                [
                    (true, Message::CloudExport, Message::BackupFlowExportHint),
                    (false, Message::CloudImport, Message::BackupFlowImportHint),
                ]
                .into_iter()
                .map(|(export, title, hint)| {
                    h_flex()
                        .gap_4()
                        .items_center()
                        .flex_wrap()
                        .py_3()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(180.0))
                                .gap_1()
                                .child(l.text(title))
                                .child(caption(l.text(hint), cx)),
                        )
                        .child(
                            Button::new(if export { "backup-export" } else { "backup-import" })
                                .label(l.text(if export {
                                    Message::CloudExport
                                } else {
                                    Message::BackupFlowChooseFile
                                }))
                                .small()
                                .disabled(self.backup_busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if export {
                                        this.open_backup_sheet(BackupSheet::Export, window, cx);
                                    } else {
                                        this.restore_backup(window, cx);
                                    }
                                })),
                        )
                }),
            )
    }
}
