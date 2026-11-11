//! 首次四步引导与抽屉共用的存储表单、内容选择。
use super::view::{caption, panel, provider, size_text};
use super::*;
use crate::gpui_shell::assets::backup as icons;
use gpui_component::menu::PopupMenuItem;

const PROVIDERS: [(BackupProtocol, bool, Message, Message, &'static str); 5] = [
    (
        BackupProtocol::WebDav,
        true,
        Message::CloudNutstore,
        Message::BackupFlowNutstoreHint,
        icons::CLOUD,
    ),
    (
        BackupProtocol::WebDav,
        false,
        Message::CloudWebdav,
        Message::BackupFlowWebdavHint,
        icons::GLOBE,
    ),
    (BackupProtocol::S3, false, Message::CloudS3, Message::BackupFlowS3Hint, icons::DATABASE),
    (BackupProtocol::Sftp, false, Message::CloudSftp, Message::BackupFlowSftpHint, icons::SERVER),
    (
        BackupProtocol::Folder,
        false,
        Message::CloudFolder,
        Message::BackupFlowFolderHint,
        icons::FOLDER,
    ),
];

const CONTENTS: [(BackupCategory, Message, Message); 9] = [
    (BackupCategory::Appearance, Message::CloudAppearance, Message::BackupFlowAppearanceHint),
    (BackupCategory::Config, Message::CloudTerminal, Message::BackupFlowTerminalHint),
    (BackupCategory::Assistant, Message::CloudAssistant, Message::BackupFlowAssistantHint),
    (BackupCategory::Sync, Message::CloudSync, Message::BackupFlowSyncHint),
    (BackupCategory::Fonts, Message::CloudFonts, Message::BackupFlowFontsHint),
    (BackupCategory::Ssh, Message::CloudHosts, Message::BackupFlowHostsHint),
    (BackupCategory::Session, Message::CloudSessions, Message::BackupFlowSessionHint),
    (BackupCategory::DirectoryHistory, Message::CloudDirectories, Message::BackupFlowDirsHint),
    (BackupCategory::CommandHistory, Message::CloudCommands, Message::BackupFlowCommandsHint),
];

pub(super) fn provider_icon(config: &BackupRemoteConfig) -> &'static str {
    PROVIDERS
        .iter()
        .find(|entry| entry.2 == provider(config))
        .map(|entry| entry.4)
        .unwrap_or(icons::DRIVE)
}

impl SettingsPane {
    pub(super) fn backup_wizard(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        let step = self.backup_ui.step;
        let steps = [
            Message::BackupFlowStorage,
            Message::BackupFlowConnection,
            Message::CloudPassphrase,
            Message::CloudScope,
        ];
        let compact = window.viewport_size().width < px(960.0);
        let progress = div()
            .debug_selector(|| "backup-wizard-progress".into())
            .grid()
            .grid_cols(if compact { 2 } else { 4 })
            .flex_shrink_0()
            .w_full()
            .px_6()
            .py_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(steps.into_iter().enumerate().map(|(i, label)| {
                h_flex()
                    .debug_selector(move || format!("backup-wizard-step-{i}"))
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .text_size(px(12.0))
                    .text_color(if i + 1 == step {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(
                        div()
                            .size(px(22.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_1()
                            .border_color(if i < step {
                                cx.theme().primary
                            } else {
                                cx.theme().border
                            })
                            .when(i + 1 == step, |d| {
                                d.bg(cx.theme().primary).text_color(cx.theme().primary_foreground)
                            })
                            .child(if i + 1 < step {
                                Icon::default()
                                    .path(icons::CHECK)
                                    .size(px(14.0))
                                    .text_color(cx.theme().primary)
                                    .into_any_element()
                            } else {
                                div().child((i + 1).to_string()).into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("backup-wizard-step-label-{i}"))
                            .flex_1()
                            .min_w_0()
                            .child(l.text(label)),
                    )
                    .when(i < 3 && !compact, |row| {
                        row.child(
                            div().w(px(12.0)).flex_shrink_0().h(px(1.0)).bg(cx.theme().border),
                        )
                    })
            }));
        let (title, lead) = match step {
            1 => (Message::BackupFlowChooseStorage, Message::BackupFlowChooseStorageHint),
            2 => (
                Message::BackupFlowConnection,
                PROVIDERS
                    .iter()
                    .find(|(_, _, name, _, _)| *name == provider(&self.backup_ui.draft))
                    .unwrap()
                    .3,
            ),
            3 => (Message::BackupFlowSetPassword, Message::BackupFlowSetPasswordHint),
            _ => (Message::BackupFlowChooseContents, Message::BackupFlowChooseContentsHint),
        };
        let title = if step == 2 {
            l.format(
                Message::BackupFlowConnectProvider,
                &[("provider", l.text(provider(&self.backup_ui.draft)))],
            )
        } else {
            l.text(title).to_owned()
        };
        let mut body = v_flex()
            .flex_shrink_0()
            .w_full()
            .min_h(px(380.0))
            .p_6()
            .gap(px(14.0))
            .when(step == 3, |d| d.items_center().gap_5())
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .debug_selector(|| "backup-wizard-heading".into())
                    .when(step == 3, |d| d.items_center().text_center())
                    .child(
                        div()
                            .text_size(px(18.0))
                            .line_height(px(26.0))
                            .font_semibold()
                            .child(title),
                    )
                    .child(caption(l.text(lead), cx).max_w(px(520.0))),
            );
        body = match step {
            1 => body.child(v_flex().w_full().flex_shrink_0().gap_2().children(
                PROVIDERS.into_iter().enumerate().map(
                    |(i, (protocol, nutstore, title, hint, icon))| {
                        Button::new(("backup-provider", i))
                            .debug_selector(move || format!("backup-provider-{i}"))
                            .map(|button| {
                                crate::gpui_shell::widgets::settings_button(button, false, cx)
                            })
                            // Provider rows contain a title and a wrapping description.
                            // Apply their geometry after the shared single-line button style.
                            .w_full()
                            .h_auto()
                            .py_3()
                            .px_4()
                            .justify_start()
                            .selected(provider(&self.backup_ui.draft) == title)
                            .disabled(self.backup_busy)
                            .child(Icon::default().path(icon).size(px(18.0)).flex_shrink_0())
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .items_start()
                                    .child(
                                        div()
                                            .debug_selector(move || {
                                                format!("backup-provider-title-{i}")
                                            })
                                            .text_size(px(14.0))
                                            .font_medium()
                                            .child(l.text(title)),
                                    )
                                    .child(caption(l.text(hint), cx).debug_selector(move || {
                                        format!("backup-provider-hint-{i}")
                                    })),
                            )
                            .when(provider(&self.backup_ui.draft) == title, |b| {
                                b.child(Icon::default().path(icons::CHECK).size(px(16.0)))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_backup_protocol(protocol, nutstore, window, cx)
                            }))
                    },
                ),
            )),
            2 => {
                body.child(self.backup_storage_fields(window.viewport_size().width < px(960.0), cx))
            },
            3 => body
                .child(
                    v_flex()
                        .debug_selector(|| "backup-password-form".into())
                        .text_size(px(13.0))
                        .max_w(px(360.0))
                        .w_full()
                        .gap_2()
                        .child(l.text(Message::CloudPassphrase))
                        .child(
                            Input::new(&self.backup_pass_input)
                                .aria_label(l.text(Message::CloudPassphrase))
                                .text_size(px(13.0))
                                .h(px(30.0))
                                .disabled(self.backup_busy),
                        )
                        .child(div().mt_3().child(l.text(Message::BackupFlowConfirmPassword)))
                        .child(
                            Input::new(self.backup_ui.confirm.as_ref().unwrap())
                                .aria_label(l.text(Message::BackupFlowConfirmPassword))
                                .text_size(px(13.0))
                                .h(px(30.0))
                                .disabled(self.backup_busy),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .max_w(px(520.0))
                        .items_start()
                        .gap(px(10.0))
                        .debug_selector(|| "backup-password-note".into())
                        .p(px(14.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(crate::gpui_shell::theme::settings_panel_bg(cx))
                        .child(
                            Icon::default()
                                .path(icons::LOCK)
                                .size(px(15.0))
                                .mt(px(2.0))
                                .flex_shrink_0()
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            caption(l.text(Message::BackupFlowPasswordNotice), cx)
                                .flex_1()
                                .min_w_0(),
                        ),
                ),
            _ => body
                .child(self.backup_content_picker(true, false, cx))
                .child(caption(l.text(Message::BackupFlowExcluded), cx)),
        };
        let next = match step {
            2 => Message::BackupFlowCheckNext,
            4 => Message::BackupFlowFinishNow,
            _ => Message::BackupFlowNext,
        };
        panel(cx)
            .flex_shrink_0()
            .debug_selector(|| "backup-wizard".into())
            .child(progress)
            .child(body.children(self.backup_status_view(cx)))
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_6()
                    .py_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .when(step > 1, |foot| {
                        foot.child(
                            Button::new("backup-back")
                                .debug_selector(|| "backup-back".into())
                                .label(l.text(Message::BackupFlowBack))
                                .map(|button| {
                                    crate::gpui_shell::widgets::settings_button(button, false, cx)
                                })
                                .disabled(self.backup_busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.backup_ui.step -= 1;
                                    this.backup_status = None;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(step == 1, |d| {
                                d.child(
                                    h_flex()
                                        .gap_1()
                                        .flex_wrap()
                                        .child(caption(
                                            l.text(Message::BackupFlowExportInstead),
                                            cx,
                                        ))
                                        .child(
                                            Button::new("backup-export-setup")
                                                .map(|button| {
                                                    crate::gpui_shell::widgets::settings_button(
                                                        button, true, cx,
                                                    )
                                                })
                                                .label(l.text(Message::CloudExport))
                                                .map(|button| {
                                                    crate::gpui_shell::widgets::settings_button(
                                                        button, false, cx,
                                                    )
                                                })
                                                .disabled(self.backup_busy)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.open_backup_sheet(
                                                        BackupSheet::Export,
                                                        window,
                                                        cx,
                                                    )
                                                })),
                                        ),
                                )
                            })
                            .when(step == 4, |d| d.child(self.backup_selection_summary(false, cx))),
                    )
                    .when(step == 4, |foot| {
                        foot.child(
                            Button::new("backup-later")
                                .debug_selector(|| "backup-later".into())
                                .label(l.text(Message::BackupFlowFinishLater))
                                .map(|button| {
                                    crate::gpui_shell::widgets::settings_button(button, false, cx)
                                })
                                .disabled(self.backup_busy || self.backup_selection.is_empty())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_backup_storage(false, window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("backup-next")
                            .debug_selector(|| "backup-next".into())
                            .label(l.text(if self.backup_busy {
                                Message::CloudChecking
                            } else {
                                next
                            }))
                            .primary()
                            .small()
                            .h(px(34.0))
                            .disabled(
                                self.backup_busy || (step == 4 && self.backup_selection.is_empty()),
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.next_backup_step(window, cx)
                                }),
                            ),
                    ),
            )
    }

    pub(super) fn backup_provider_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let l = crate::gpui_shell::config::ui_language(cx);
        let owner = cx.entity().downgrade();
        Button::new("backup-provider-menu")
            .debug_selector(|| "backup-provider-menu".into())
            .w_full()
            .dropdown_caret(true)
            .justify_between()
            .label(l.text(provider(&self.backup_ui.draft)))
            .disabled(self.backup_busy)
            .dropdown_menu(move |mut menu, _, _| {
                for (protocol, nutstore, name, _, _) in PROVIDERS {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(l.text(name)).on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.select_backup_protocol(protocol, nutstore, window, cx)
                            });
                        },
                    ));
                }
                menu
            })
    }

    pub(super) fn backup_content_picker(
        &self,
        columns: bool,
        restore: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        let summary = if restore {
            self.backup_ui.opened.as_ref().map(|a| a.summary()).unwrap_or_default()
        } else {
            self.backup_ui.summary.clone()
        };
        let mut groups = div()
            .flex()
            .w_full()
            .gap_5()
            .when(columns, |d| d.flex_row().flex_wrap())
            .when(!columns, |d| d.flex_col());
        for (start, end, title) in
            [(0, 5, Message::BackupFlowSettings), (5, 9, Message::BackupFlowData)]
        {
            let mut group =
                v_flex().gap_1().min_w(px(220.0)).flex_1().child(caption(l.text(title), cx));
            for (index, &(category, name, hint)) in
                CONTENTS.iter().enumerate().take(end).skip(start)
            {
                let stats = summary.iter().find(|s| s.category == category);
                if restore && stats.is_none() {
                    continue;
                }
                let checked = self.backup_selection.categories().any(|c| c == category);
                group = group.child(
                    Checkbox::new(("backup-content", index))
                        .debug_selector(move || format!("backup-content-{index}"))
                        .w_full()
                        .py_2()
                        .px_2()
                        .rounded_md()
                        .hover(|d| d.bg(cx.theme().muted))
                        .label(l.text(name))
                        .checked(checked)
                        .disabled(self.backup_busy)
                        .child(caption(l.text(hint), cx))
                        .when_some(stats, |c, s| {
                            c.child(caption(format!("{} · {}", s.files, size_text(s.bytes)), cx))
                        })
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            this.backup_selection.set(category, *checked);
                            cx.notify();
                        })),
                );
            }
            groups = groups.child(group);
        }
        v_flex()
            .w_full()
            .gap_3()
            .child(groups)
            .when(!restore && self.backup_ui.summary_loading, |d| {
                d.child(caption(l.text(Message::BackupFlowMeasuring), cx))
            })
            .when(!restore, |d| {
                d.children(
                    self.backup_ui
                        .summary_error
                        .as_ref()
                        .map(|e| div().text_sm().text_color(cx.theme().danger).child(e.clone())),
                )
            })
    }

    pub(super) fn backup_selection_summary(&self, restore: bool, cx: &App) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        let summary = if restore {
            self.backup_ui.opened.as_ref().map(|a| a.summary()).unwrap_or_default()
        } else {
            self.backup_ui.summary.clone()
        };
        let categories: Vec<_> = self.backup_selection.categories().collect();
        let bytes =
            summary.iter().filter(|s| categories.contains(&s.category)).map(|s| s.bytes).sum();
        caption(
            if categories.is_empty() {
                l.text(Message::BackupFlowSelectOne).to_owned()
            } else if !restore
                && (self.backup_ui.summary_loading || self.backup_ui.summary_error.is_some())
            {
                l.text(Message::BackupFlowMeasuring).to_owned()
            } else {
                l.format(
                    Message::BackupFlowTotal,
                    &[("count", &categories.len().to_string()), ("size", &size_text(bytes))],
                )
            },
            cx,
        )
    }
}
