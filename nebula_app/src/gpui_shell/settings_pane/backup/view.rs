//! Backup storage, local archives and snapshot history.
use super::*;

pub(super) fn panel(cx: &App) -> gpui::Div {
    super::super::design::settings_card(cx).flex_shrink_0().overflow_hidden()
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
        let wizard = self.backup_remote.protocol == BackupProtocol::Off && self.backup_ui.step > 0;
        v_flex()
            .w_full()
            .flex_shrink_0()
            .max_w(px(700.0))
            .gap(px(20.0))
            .text_size(px(13.0))
            .child(
                v_flex()
                    .gap_2()
                    .mb_1()
                    .child(
                        div()
                            .text_size(px(18.0))
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
                                .map(|button| {
                                    crate::gpui_shell::widgets::settings_button(button, false, cx)
                                })
                                .disabled(self.backup_busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.undo_backup_restore(window, cx)
                                })),
                        )
                        .child(
                            Button::new("backup-dismiss-undo")
                                .icon(IconName::Close)
                                .map(|button| {
                                    crate::gpui_shell::widgets::settings_icon_button(button, cx)
                                })
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
            .when(wizard, |page| page.child(self.backup_wizard(window, cx)))
            .when(!wizard, |page| {
                page.child(self.backup_dashboard(cx))
                    .children(self.backup_status_view(cx))
                    .child(self.backup_local_archive(cx))
            })
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
        let off = self.backup_remote.protocol == BackupProtocol::Off;
        let latest = self.backup_ui.snapshots.first().filter(|_| !off);
        let path = match self.backup_remote.protocol {
            BackupProtocol::Folder => self.backup_remote.folder_path.clone(),
            BackupProtocol::WebDav => self.backup_remote.webdav_url.clone(),
            BackupProtocol::S3 => {
                format!("{} / {}", self.backup_remote.s3_endpoint, self.backup_remote.s3_bucket)
            },
            BackupProtocol::Sftp => {
                format!("{}:{}", self.backup_remote.sftp_destination, self.backup_remote.sftp_path)
            },
            BackupProtocol::Off => l.text(Message::BackupFlowStorageOffHint).to_owned(),
        };
        let hairline = crate::gpui_shell::theme::settings_hairline(cx);
        let busy = self.backup_busy || self.backup_ui.listing;
        let storage = backup_row(
            l.text(if off { Message::BackupFlowStorageOff } else { provider(&self.backup_remote) }),
            if off {
                caption(path, cx)
            } else {
                caption(path, cx).font_family(cx.theme().mono_font_family.clone()).truncate()
            },
            h_flex()
                .gap(px(8.0))
                .flex_wrap()
                .child(
                    backup_button("backup-configure", l.text(Message::BackupFlowConfigureNode), cx)
                        .debug_selector(|| "backup-configure".into())
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if off {
                                this.backup_ui.step = 1;
                                this.backup_status = None;
                                cx.notify();
                            } else {
                                this.open_backup_sheet(BackupSheet::Storage, window, cx);
                            }
                        })),
                )
                .child(
                    backup_button("backup-now", l.text(Message::CloudBackupNow), cx)
                        .debug_selector(|| "backup-now".into())
                        .disabled(off || busy || self.backup_ui.list_error.is_some())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_backup_sheet(BackupSheet::Backup, window, cx)
                        })),
                ),
            cx,
        );
        let status = (!off).then(|| {
            let failed = self.backup_ui.list_error.is_some();
            let theme = cx.theme();
            let color = if !failed {
                theme.muted_foreground
            } else if theme.is_dark() {
                theme.danger.blend(theme.foreground.opacity(0.30))
            } else {
                theme.danger
            };
            let label = l.text(if self.backup_ui.listing {
                Message::CloudChecking
            } else if failed {
                Message::BackupFlowConnectionFailed
            } else {
                Message::BackupFlowConnected
            });
            v_flex()
                .w_full()
                .min_w_0()
                .px(px(14.0))
                .py(px(10.0))
                .gap_2()
                .border_t_1()
                .border_color(hairline)
                .bg(cx.theme().foreground.opacity(0.025))
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .justify_between()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap(px(6.0))
                                .min_w_0()
                                .rounded_full()
                                .px(px(9.0))
                                .py(px(3.0))
                                .border_1()
                                .border_color(color.opacity(0.30))
                                .bg(color.opacity(0.12))
                                .text_color(color)
                                .text_size(px(11.5))
                                .child(div().size(px(6.0)).rounded_full().flex_shrink_0().bg(color))
                                .child(label),
                        )
                        .child(
                            Button::new("backup-refresh")
                                .ghost()
                                .h(px(30.0))
                                .text_size(px(12.0))
                                .icon(IconName::Redo2)
                                .label(l.text(if failed {
                                    Message::CommonRetry
                                } else {
                                    Message::CloudTest
                                }))
                                .disabled(busy)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.refresh_backup_snapshots(cx)),
                                ),
                        ),
                )
                .children(self.backup_ui.list_error.as_ref().map(|error| {
                    div()
                        .debug_selector(|| "backup-storage-error".into())
                        .w_full()
                        .min_w_0()
                        .text_size(px(12.0))
                        .line_height(gpui::relative(1.5))
                        .text_color(color)
                        .child(error.clone())
                }))
        });
        let mut history =
            v_flex().w_full().px(px(14.0)).py(px(8.0)).border_t_1().border_color(hairline);
        if let Some(entry) = latest {
            history = history
                .child(caption(l.text(Message::BackupFlowLastBackup), cx))
                .child(caption(self.snapshot_meta(entry, cx), cx));
        }
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
                            .map(|button| {
                                crate::gpui_shell::widgets::settings_button(button, false, cx)
                            })
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
        v_flex()
            .w_full()
            .gap(px(8.0))
            .child(backup_group_heading(l.text(Message::BackupFlowStorageNode), cx))
            .child(
                panel(cx)
                    .debug_selector(|| "backup-dashboard".into())
                    .border_1()
                    .border_color(hairline)
                    .rounded(px(8.0))
                    .child(storage)
                    .children(status)
                    .when(latest.is_some(), |card| card.child(history)),
            )
    }

    fn backup_local_archive(&self, cx: &mut Context<Self>) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        let ready = cx.try_global::<BackupPassword>().is_some_and(|p| p.0.is_some());
        let hairline = crate::gpui_shell::theme::settings_hairline(cx);
        let password = backup_row(
            l.text(Message::CloudPassphrase),
            caption(l.text(Message::BackupFlowPasswordHint), cx)
                .debug_selector(|| "backup-password-description".into()),
            backup_button(
                "backup-password",
                l.text(if ready { Message::BackupFlowClear } else { Message::BackupFlowEnter }),
                cx,
            )
            .debug_selector(|| "backup-password".into())
            .disabled(self.backup_busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                if ready {
                    cx.set_global(BackupPassword::default());
                    this.clear_backup_inputs(window, cx);
                    cx.notify();
                } else {
                    this.open_backup_sheet(BackupSheet::Password, window, cx);
                }
            })),
            cx,
        );
        v_flex()
            .w_full()
            .gap(px(8.0))
            .debug_selector(|| "backup-encryption".into())
            .child(backup_group_heading(l.text(Message::BackupFlowLocalArchive), cx))
            .child(
                panel(cx)
                    .border_1()
                    .border_color(hairline)
                    .rounded(px(8.0))
                    .child(password)
                    .children(
                        [
                            (true, Message::CloudExport, Message::BackupFlowExportHint),
                            (false, Message::CloudImport, Message::BackupFlowImportHint),
                        ]
                        .into_iter()
                        .map(|(export, title, hint)| {
                            let id = if export { "backup-export" } else { "backup-import" };
                            backup_row(
                                l.text(title),
                                caption(l.text(hint), cx),
                                backup_button(
                                    id,
                                    l.text(if export {
                                        Message::CloudExport
                                    } else {
                                        Message::BackupFlowChooseFile
                                    }),
                                    cx,
                                )
                                .debug_selector(move || id.into())
                                .disabled(self.backup_busy)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if export {
                                            this.open_backup_sheet(BackupSheet::Export, window, cx);
                                        } else {
                                            this.restore_backup(window, cx);
                                        }
                                    },
                                )),
                                cx,
                            )
                            .border_t_1()
                            .border_color(hairline)
                        }),
                    ),
            )
    }
}

fn backup_group_heading(text: &'static str, cx: &App) -> gpui::Div {
    div()
        .pl(px(2.0))
        .text_size(px(11.5))
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

fn backup_row(
    title: &'static str,
    detail: impl IntoElement,
    action: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w_0()
        .min_h(px(42.0))
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(px(8.0))
        .px(px(14.0))
        .py(px(10.0))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(160.0))
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_medium()
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .child(detail),
        )
        .child(action)
}

fn backup_button(id: &'static str, label: &'static str, cx: &App) -> Button {
    crate::gpui_shell::widgets::settings_button(Button::new(id).label(label), true, cx)
        .h(px(30.0))
        .text_size(px(12.0))
}
