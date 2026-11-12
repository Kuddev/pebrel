use super::*;
use crate::i18n::Message;

fn busy_window_dialog(
    dialog: Dialog,
    window: &Window,
    cx: &App,
    language: crate::i18n::UiLanguage,
    body: SharedString,
    process: &str,
) -> Dialog {
    use gpui_component::button::ButtonCustomVariant;
    use gpui_component::dialog::{CancelDialog, ConfirmDialog};

    let theme = cx.theme();
    let width = (window.viewport_size().width - px(32.0)).clamp(px(220.0), px(360.0));
    let muted = theme.muted_foreground;
    let title = language.text(Message::WorkspaceCloseWindowTitle);
    let cancel = language.text(Message::CommonCancel);
    let force = language.text(Message::WorkspaceCloseWindowForce);
    let process_range = (!process.is_empty())
        .then(|| body.find(process).map(|start| start..start + process.len()))
        .flatten();
    let pill = gpui::HighlightStyle {
        color: Some(theme.link),
        background_color: Some(theme.foreground.opacity(0.07)),
        ..Default::default()
    };
    let key = |label: &'static str| {
        div()
            .px(px(5.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.foreground.opacity(0.05))
            .text_color(muted)
            .font_family(theme.mono_font_family.clone())
            .text_size(px(10.0))
            .child(label)
    };
    let button = |id: &'static str, label: &'static str, shortcut, danger: bool| {
        let fill = if danger { theme.danger.opacity(0.16) } else { theme.secondary };
        let edge = if danger { theme.danger.opacity(0.42) } else { theme.border };
        let ink = if danger && theme.is_dark() {
            theme.danger.blend(theme.foreground.opacity(0.30))
        } else {
            theme.foreground
        };
        let hover =
            if danger { theme.danger.opacity(0.23) } else { theme.foreground.opacity(0.10) };
        Button::new(id)
            .debug_selector(move || id.into())
            .aria_label(label)
            .custom(ButtonCustomVariant::new(cx).color(fill).foreground(ink).hover(hover).active(
                if danger { theme.danger.opacity(0.32) } else { theme.foreground.opacity(0.14) },
            ))
            .border_1()
            .h(px(30.0))
            .px(px(12.0))
            .rounded(px(6.0))
            .border_color(edge)
            .cursor_pointer()
            .child(
                h_flex()
                    .gap(px(6.0))
                    .text_size(px(12.0))
                    .font_medium()
                    .child(label)
                    .child(key(shortcut)),
            )
    };
    dialog
        .width(width)
        .margin_top(((window.viewport_size().height - px(148.0)) * 0.5).max(px(16.0)))
        .px(px(20.0))
        .py(px(18.0))
        .rounded(px(10.0))
        .bg(theme.popover)
        .close_button(false)
        .overlay_closable(true)
        .title(
            div()
                .debug_selector(|| "busy-window-title".into())
                .text_size(px(14.0))
                .font_semibold()
                .child(title),
        )
        .content(move |content, _, _| {
            content.child(
                div().text_size(px(12.5)).line_height(relative(1.5)).text_color(muted).child(
                    gpui::StyledText::new(body.clone())
                        .with_highlights(process_range.clone().map(|range| (range, pill))),
                ),
            )
        })
        .footer(
            DialogFooter::new()
                .w_full()
                .pt(px(10.0))
                .child(button("busy-window-cancel", cancel, "Esc", false).on_click(
                    |_, window, cx| {
                        window.dispatch_action(Box::new(CancelDialog), cx);
                    },
                ))
                .child(button("busy-window-force", force, "↵", true).on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(ConfirmDialog), cx);
                })),
        )
        .button_props(
            DialogButtonProps::default().ok_text(force).cancel_text(cancel).show_cancel(true),
        )
}

impl NebulaWorkspace {
    pub(super) fn request_close_pane(
        &mut self,
        tab_ix: usize,
        pane_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(process) = self.busy_process_in_tab(tab_ix, Some(pane_id), cx) else {
            self.close_pane(tab_ix, pane_id, window, cx);
            return;
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let body: SharedString =
            language.format(Message::WorkspaceCloseRunningProcess, &[("process", &process)]).into();
        let workspace = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let workspace = workspace.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::WorkspaceClosePaneTitle),
                body.clone(),
                language.text(Message::CommonClose),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = workspace.update(cx, |workspace, cx| {
                    workspace.close_pane(tab_ix, pane_id, window, cx);
                });
                true
            })
        });
    }

    pub(super) fn request_close_tab(
        &mut self,
        tab_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(process) = self.busy_process_in_tab(tab_ix, None, cx) else {
            self.close_tab(tab_ix, window, cx);
            return;
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let body: SharedString =
            language.format(Message::WorkspaceCloseRunningProcess, &[("process", &process)]).into();
        let workspace = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let workspace = workspace.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::WorkspaceCloseTabTitle),
                body.clone(),
                language.text(Message::CommonClose),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let _ = workspace.update(cx, |workspace, cx| {
                    workspace.close_tab(tab_ix, window, cx);
                });
                true
            })
        });
    }

    /// GPUI 的 should-close 回调必须同步返回：无繁忙进程时直接允许系统关闭；
    /// 有繁忙进程时先返回 false，再由对话框确认回调显式移除窗口。
    pub(super) fn should_close_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.theme_transition.clear(window);
        windowing::quick_terminal_bounds_changed(self.runtime_window_id, window, cx);
        if self.window_close_pending {
            return false;
        }
        let persist_session = self.window_role == windowing::WindowRole::Regular;
        if persist_session && self.keep_session_on_close(window, cx) {
            return false;
        }
        if self.guard_file_window_close(window, cx) {
            return false;
        }
        self.close_window_after_documents(window, cx)
    }

    /// Continue after document save/discard confirmation. A true result lets
    /// the caller remove the window; false means cancellation or an async close.
    pub(super) fn close_window_after_documents(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let persist_session = self.window_role == windowing::WindowRole::Regular;
        let Some(process) = self.busy_process_in_window(cx) else {
            if persist_session {
                self.finish_close_window(window, cx);
                return false;
            }
            return true;
        };
        if self.window_close_confirm_open {
            return false;
        }
        self.window_close_confirm_open = true;

        let language = crate::gpui_shell::config::ui_language(cx);
        let body: SharedString = language
            .format(Message::WorkspaceCloseRunningWindowProcess, &[("process", &process)])
            .into();
        let confirm_workspace = cx.entity().downgrade();
        let close_workspace = confirm_workspace.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let confirm_workspace = confirm_workspace.clone();
            let close_workspace = close_workspace.clone();
            busy_window_dialog(dialog, window, cx, language, body.clone(), &process)
                .on_ok(move |_, window, cx| {
                    let _ = confirm_workspace.update(cx, |workspace, cx| {
                        if persist_session {
                            workspace.finish_close_window(window, cx);
                        }
                        workspace.window_close_confirm_open = false;
                        // `remove_window` 是确认后的最终动作，不会重新触发
                        // should-close，从而避免再次弹出同一确认框。
                        if !persist_session {
                            window.remove_window();
                        }
                    });
                    true
                })
                .on_close(move |_, _, cx| {
                    let _ = close_workspace.update(cx, |workspace, cx| {
                        workspace.window_close_confirm_open = false;
                        cx.notify();
                    });
                })
        });
        false
    }

    fn finish_close_window(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.window_close_pending = true;
        let panes = self.prepare_session_save(cx);
        let approved_drafts: Vec<_> = self
            .tabs
            .iter()
            .filter_map(|tab| tab.file_editor(cx))
            .map(|file| {
                let draft = file.read(cx).draft(cx);
                (file, draft)
            })
            .collect();
        let handle = self.window_handle;
        cx.spawn(async move |this, cx| {
            let ready = wait_for_session_ids(&panes, cx).await;
            if !ready && !confirm_incomplete_session(handle, cx).await {
                let _ = this.update(cx, |workspace, cx| {
                    workspace.window_close_pending = false;
                    cx.notify();
                });
                return;
            }
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |workspace, cx| {
                    if workspace.tabs.iter().filter_map(|tab| tab.file_editor(cx)).any(|file| {
                        let view = file.read(cx);
                        view.is_saving()
                            || (view.is_dirty()
                                && !approved_drafts.iter().any(|(approved, draft)| {
                                    approved == &file && *draft == view.draft(cx)
                                }))
                    }) {
                        workspace.window_close_pending = false;
                        let language = crate::gpui_shell::config::ui_language(cx);
                        crate::gpui_shell::toast::banner(
                            window,
                            cx,
                            crate::display::ToastKind::Warning,
                            language.text(crate::i18n::Message::UpdateDraftChanged),
                        );
                        cx.notify();
                        return;
                    }
                    if workspace.save_clean_window_session(window, cx).is_err() {
                        workspace.window_close_pending = false;
                        let language = crate::gpui_shell::config::ui_language(cx);
                        crate::gpui_shell::toast::banner(
                            window,
                            cx,
                            crate::display::ToastKind::Warning,
                            language.text(crate::i18n::Message::SessionSaveFailed),
                        );
                        cx.notify();
                        return;
                    }
                    window.remove_window();
                });
            });
        })
        .detach();
    }

    pub(super) fn prepare_session_save(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<TerminalView>> {
        let panes = self
            .tabs
            .iter()
            .filter_map(|tab| match tab {
                WorkspaceTab::Terminal { panes, .. } => Some(panes),
                _ => None,
            })
            .flatten()
            .map(|pane| pane.view.clone())
            .collect::<Vec<_>>();
        for pane in &panes {
            pane.update(cx, |view, cx| view.prepare_ai_session_save(cx));
        }
        panes
    }
}

/// Missing provider metadata must not make closing impossible. Consent covers
/// missing identities only: draft approval and durable-save failures still block.
pub(super) async fn confirm_incomplete_session(
    handle: gpui::AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) -> bool {
    let prompt = handle.update(cx, |_, window, cx| {
        use crate::i18n::Message;
        let language = crate::gpui_shell::config::ui_language(cx);
        window.activate_window();
        window.prompt(
            gpui::PromptLevel::Warning,
            language.text(Message::SessionExitIncompleteTitle),
            Some(language.text(Message::SessionExitIncompleteBody)),
            &[language.text(Message::EditorCancel), language.text(Message::SessionExitAnyway)],
            cx,
        )
    });
    let Ok(prompt) = prompt else { return false };
    matches!(prompt.await, Ok(1))
}

pub(super) async fn wait_for_session_ids(
    panes: &[Entity<TerminalView>],
    cx: &mut gpui::AsyncApp,
) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let pending =
            cx.update(|cx| panes.iter().any(|pane| pane.read(cx).ai_session_save_pending()));
        if !pending {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        cx.background_executor().timer(Duration::from_millis(25)).await;
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    async fn missing_identity_prompt_supports_cancel_and_explicit_close(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(crate::gpui_shell::config::Settings::load(
                nebula_settings::ThemeName::Nord,
            ));
        });
        let handle = cx.add_empty_window().update(|window, _| window.window_handle());
        for (message, expected) in [
            (crate::i18n::Message::EditorCancel, false),
            (crate::i18n::Message::SessionExitAnyway, true),
        ] {
            let answer = cx.update(|cx| crate::gpui_shell::config::ui_language(cx).text(message));
            let task =
                cx.spawn(
                    move |mut cx| async move { confirm_incomplete_session(handle, &mut cx).await },
                );
            cx.run_until_parked();
            assert!(cx.has_pending_prompt());
            cx.simulate_prompt_answer(answer);
            assert_eq!(task.await, expected);
            assert_eq!(
                cx.windows(),
                vec![handle],
                "the prompt cannot stop a pane before durable saving"
            );
        }
    }
}
