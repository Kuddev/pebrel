//! Storage field presentation shared by the setup wizard and storage drawer.
use super::view::{caption, provider};
use super::*;
use gpui_component::menu::PopupMenuItem;

pub(super) struct StorageField {
    pub label: Message,
    pub placeholder: Option<Message>,
    hint: Option<Message>,
    mono: bool,
    pub secret: bool,
}

impl StorageField {
    const fn new(label: Message, placeholder: Option<Message>) -> Self {
        Self { label, placeholder, hint: None, mono: false, secret: false }
    }
    const fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
    const fn secret(mut self) -> Self {
        self.secret = true;
        self
    }
    const fn hint(mut self, hint: Message) -> Self {
        self.hint = Some(hint);
        self
    }
}

pub(super) fn storage_fields(config: &BackupRemoteConfig) -> &'static [StorageField] {
    use Message::*;
    match config.protocol {
        BackupProtocol::WebDav if provider(config) == CloudNutstore => {
            const {
                &[
                    StorageField::new(BackupFlowAddressLabel, None).mono(),
                    StorageField::new(CloudUsername, Some(BackupFlowNutstoreAccountPlaceholder)),
                    StorageField::new(
                        BackupFlowAppPasswordLabel,
                        Some(BackupFlowAppPasswordPlaceholder),
                    )
                    .secret()
                    .hint(BackupFlowAppPasswordHint),
                ]
            }
        },
        BackupProtocol::WebDav => {
            const {
                &[
                    StorageField::new(BackupFlowAddressLabel, Some(BackupFlowWebdavPlaceholder))
                        .mono()
                        .hint(BackupFlowWebdavAddressHint),
                    StorageField::new(CloudUsername, Some(BackupFlowUsernamePlaceholder)),
                    StorageField::new(BackupFlowPasswordLabel, Some(BackupFlowPasswordPlaceholder))
                        .secret(),
                ]
            }
        },
        BackupProtocol::S3 => {
            const {
                &[
                    StorageField::new(CloudS3Address, Some(BackupFlowS3Placeholder))
                        .mono()
                        .hint(BackupFlowS3AddressHint),
                    StorageField::new(CloudRegion, None).mono(),
                    StorageField::new(CloudBucket, Some(BackupFlowBucketPlaceholder)).mono(),
                    StorageField::new(
                        BackupFlowAccessKeyLabel,
                        Some(BackupFlowAccessKeyPlaceholder),
                    )
                    .mono(),
                    StorageField::new(
                        BackupFlowSecretKeyLabel,
                        Some(BackupFlowSecretKeyPlaceholder),
                    )
                    .secret(),
                ]
            }
        },
        BackupProtocol::Sftp => {
            const {
                &[
                    StorageField::new(BackupFlowSshHostLabel, None).hint(BackupFlowSshHostHint),
                    StorageField::new(CloudRemotePath, Some(BackupFlowRemotePathPlaceholder))
                        .mono(),
                ]
            }
        },
        BackupProtocol::Folder => {
            const {
                &[StorageField::new(BackupFlowFolderLabel, Some(BackupFlowFolderPlaceholder))
                    .mono()]
            }
        },
        BackupProtocol::Off => &[],
    }
}

impl SettingsPane {
    pub(super) fn backup_storage_fields(&self, stacked: bool, cx: &mut Context<Self>) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        let protocol = self.backup_ui.draft.protocol;
        let mut fields = v_flex().w_full().gap_0();
        for (index, field) in storage_fields(&self.backup_ui.draft).iter().enumerate() {
            let mut control = h_flex()
                .gap_2()
                .min_w_0()
                .debug_selector(move || format!("backup-field-control-{index}"))
                .when(stacked, |d| d.w_full())
                .when(!stacked, |d| d.w(px(300.0)).flex_shrink_0());
            if protocol == BackupProtocol::Sftp && index == 0 {
                let hosts = self.ssh_hosts.merged_with_labels();
                let owner = cx.entity().downgrade();
                let selected = self.backup_remote_inputs[0].read(cx).value();
                control = control.child(
                    Button::new("backup-ssh-host")
                        .flex_1()
                        .min_w_0()
                        .dropdown_caret(true)
                        .label(if selected.is_empty() {
                            l.text(Message::CloudSshHost).into()
                        } else {
                            selected
                        })
                        .disabled(self.backup_busy || hosts.is_empty())
                        .dropdown_menu(move |mut menu, _, _| {
                            for (destination, label) in &hosts {
                                let owner = owner.clone();
                                let destination = destination.clone();
                                let label = if label.is_empty() {
                                    destination.clone()
                                } else {
                                    format!("{label} · {destination}")
                                };
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, window, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            this.backup_remote_inputs[0].update(cx, |input, cx| {
                                                input.set_value(destination.clone(), window, cx)
                                            });
                                            this.backup_ui.tested = false;
                                            cx.notify();
                                        });
                                    },
                                ));
                            }
                            menu
                        }),
                );
            } else {
                control = control.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .when(field.mono, |d| d.font_family(cx.theme().mono_font_family.clone()))
                        .child(
                            Input::new(if field.secret {
                                &self.backup_secret_input
                            } else {
                                &self.backup_remote_inputs[index]
                            })
                            .aria_label(l.text(field.label))
                            .text_size(px(if field.mono { 12.5 } else { 13.0 }))
                            .h(px(30.0))
                            .disabled(self.backup_busy),
                        ),
                );
            }
            if protocol == BackupProtocol::Folder {
                control = control.child(
                    Button::new("backup-browse")
                        .label(l.text(Message::BackupFlowBrowse))
                        .small()
                        .disabled(self.backup_busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.pick_backup_folder(window, cx)),
                        ),
                );
            }
            let description = v_flex()
                .gap(px(3.0))
                .min_w_0()
                .debug_selector(move || format!("backup-field-label-{index}"))
                .when(!stacked, |d| d.flex_1())
                .child(div().text_size(px(14.0)).font_medium().child(l.text(field.label)))
                .when(!stacked, |d| d.children(field.hint.map(|hint| caption(l.text(hint), cx))));
            fields = fields.child(
                div()
                    .flex()
                    .w_full()
                    .py(px(12.0))
                    .when(stacked, |d| d.flex_col().gap_2())
                    .when(!stacked, |d| d.flex_row().items_center().gap(px(24.0)))
                    .child(description)
                    .child(control)
                    .when(stacked, |d| {
                        d.children(field.hint.map(|hint| caption(l.text(hint), cx)))
                    }),
            );
        }
        if stacked && matches!(protocol, BackupProtocol::WebDav | BackupProtocol::S3) {
            fields = fields.child(caption(l.text(Message::BackupFlowCredentialHint), cx));
        }
        fields
    }
}
