//! 设置页拥有抽屉和焦点；离开设置时不会在窗口全局留下悬空弹层。
use super::view::caption;
use super::*;
use gpui_component::FocusTrapElement as _;

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn backup_drawer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let sheet = self.backup_ui.sheet?;
        let l = crate::gpui_shell::config::ui_language(cx);
        let focus = self.backup_ui.sheet_focus.as_ref()?;
        let (title, lead, action) = match sheet {
            BackupSheet::Storage => {
                (Message::BackupFlowStorage, Message::BackupFlowStorageHint, Message::CommonSave)
            },
            BackupSheet::Backup => {
                (Message::CloudBackupNow, Message::BackupFlowUploadHint, Message::BackupFlowStart)
            },
            BackupSheet::Export => {
                (Message::CloudExport, Message::BackupFlowExportHint, Message::CloudExport)
            },
            BackupSheet::Restore => {
                (Message::CloudRestore, Message::BackupFlowRestoreHint, Message::CloudRestore)
            },
            BackupSheet::Password => {
                (Message::CloudPassphrase, Message::BackupFlowPasswordHint, Message::CommonSave)
            },
        };
        let cached = cx.try_global::<BackupPassword>().is_some_and(|p| p.0.is_some());
        let mut body = v_flex().w_full().gap_4();
        body = match sheet {
            BackupSheet::Storage => body
                .child(self.backup_provider_menu(cx))
                .child(caption(l.text(Message::BackupFlowSwitchHint), cx))
                .child(self.backup_storage_fields(true, cx))
                .child(
                    Button::new("backup-check")
                        .label(l.text(Message::CloudTest))
                        .disabled(self.backup_busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.check_backup_connection(false, window, cx)
                        })),
                )
                .child(caption(
                    l.text(if self.backup_ui.tested {
                        Message::BackupFlowConnected
                    } else {
                        Message::BackupFlowReadOnlyCheck
                    }),
                    cx,
                )),
            BackupSheet::Backup | BackupSheet::Export => body
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().child(l.text(Message::BackupFlowChooseContents)))
                        .child(
                            Button::new("backup-defaults")
                                .label(l.text(Message::BackupFlowDefaults))
                                .small()
                                .ghost()
                                .disabled(self.backup_busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.backup_selection = recommended();
                                    cx.notify();
                                })),
                        ),
                )
                .child(self.backup_content_picker(false, false, cx))
                .child(caption(l.text(Message::BackupFlowExcluded), cx))
                .when(!cached, |d| d.child(self.backup_password_field(cx))),
            BackupSheet::Restore => {
                let source = match &self.backup_ui.source {
                    Some(RestoreSource::File(path)) => {
                        path.file_name().unwrap_or_default().to_string_lossy().into_owned()
                    },
                    Some(RestoreSource::Remote(name)) => name.clone(),
                    None => String::new(),
                };
                body = body.child(caption(source, cx));
                if self.backup_ui.opened.is_some() {
                    body.child(caption(l.text(Message::BackupFlowUnselectedUntouched), cx))
                        .child(self.backup_content_picker(false, true, cx))
                        .child(caption(l.text(Message::BackupFlowRestoreWarning), cx))
                } else {
                    body.child(self.backup_password_field(cx)).child(
                        Button::new("backup-unlock")
                            .debug_selector(|| "backup-unlock".into())
                            .label(l.text(Message::BackupFlowUnlock))
                            .disabled(self.backup_busy)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.unlock_backup(window, cx)),
                            ),
                    )
                }
            },
            BackupSheet::Password => body.child(self.backup_password_field(cx)),
        };
        body = body.children(self.backup_status_view(cx));
        let disabled = self.backup_busy
            || match sheet {
                BackupSheet::Backup | BackupSheet::Export => self.backup_selection.is_empty(),
                BackupSheet::Restore => {
                    self.backup_ui.opened.is_none() || self.backup_selection.is_empty()
                },
                _ => false,
            };
        let viewport = window.viewport_size();
        let drawer = v_flex()
            .id("backup-drawer")
            .debug_selector(|| "backup-drawer".into())
            .role(gpui::accesskit::Role::Dialog)
            .aria_label(l.text(title))
            .w(px(440.0).min(viewport.width))
            .h_full()
            .min_w_0()
            .bg(crate::gpui_shell::theme::settings_panel_bg(cx))
            .text_color(cx.theme().foreground)
            .border_l_1()
            .border_color(cx.theme().border)
            .shadow_xl()
            .track_focus(focus)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.cancel_backup_sheet(window, cx);
                }
            }))
            .child(
                h_flex()
                    .px_6()
                    .py_5()
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_size(px(18.0)).font_semibold().child(l.text(title)))
                            .child(caption(l.text(lead), cx)),
                    )
                    .child(
                        Button::new("backup-close-drawer")
                            .icon(IconName::Close)
                            .small()
                            .ghost()
                            .tooltip(l.text(Message::CommonClose))
                            .disabled(self.backup_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_backup_sheet(window, cx)
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_y_scrollbar().child(body.p_6()))
            .child(
                v_flex()
                    .px_6()
                    .py_4()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .when(
                        matches!(
                            sheet,
                            BackupSheet::Backup | BackupSheet::Export | BackupSheet::Restore
                        ),
                        |d| {
                            d.child(
                                self.backup_selection_summary(sheet == BackupSheet::Restore, cx),
                            )
                        },
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("backup-cancel")
                                    .debug_selector(|| "backup-cancel".into())
                                    .label(l.text(Message::CommonCancel))
                                    .ghost()
                                    .small()
                                    .disabled(self.backup_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cancel_backup_sheet(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("backup-confirm")
                                    .debug_selector(|| "backup-confirm".into())
                                    .label(l.text(if self.backup_busy {
                                        Message::BackupFlowProcessing
                                    } else {
                                        action
                                    }))
                                    .primary()
                                    .small()
                                    .disabled(disabled)
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| match sheet {
                                            BackupSheet::Storage => {
                                                this.check_backup_connection(true, window, cx)
                                            },
                                            BackupSheet::Backup => {
                                                this.perform_backup(false, window, cx)
                                            },
                                            BackupSheet::Export => {
                                                this.perform_backup(true, window, cx)
                                            },
                                            BackupSheet::Restore => {
                                                this.restore_selected_backup(window, cx)
                                            },
                                            BackupSheet::Password => {
                                                if let Some(pass) = this.backup_passphrase(cx) {
                                                    cx.set_global(BackupPassword(Some(pass)));
                                                    this.close_backup_sheet(window, cx);
                                                    this.clear_backup_inputs(window, cx);
                                                }
                                            },
                                        },
                                    )),
                            ),
                    ),
            )
            .focus_trap("backup-drawer-trap", focus);
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(gpui::point(px(0.0), px(0.0)))
                    .child(
                        div()
                            .id("backup-overlay")
                            .w(viewport.width)
                            .h(viewport.height)
                            .flex()
                            .justify_end()
                            .occlude()
                            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.30))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.cancel_backup_sheet(window, cx);
                                }),
                            )
                            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                            .child(drawer),
                    ),
            )
            .with_priority(8)
            .into_any_element(),
        )
    }

    fn backup_password_field(&self, cx: &App) -> gpui::Div {
        let l = crate::gpui_shell::config::ui_language(cx);
        v_flex()
            .w_full()
            .gap_2()
            .child(l.text(Message::CloudPassphrase))
            .child(Input::new(&self.backup_pass_input).h(px(34.0)).disabled(self.backup_busy))
            .child(caption(l.text(Message::BackupFlowPasswordHint), cx))
    }
}
