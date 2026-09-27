use super::*;
use crate::i18n::Message;

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
        #[cfg(windows)]
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
        window.open_dialog(cx, move |dialog, window, _cx| {
            let confirm_workspace = confirm_workspace.clone();
            let close_workspace = close_workspace.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::WorkspaceCloseWindowTitle),
                body.clone(),
                language.text(Message::CommonClose),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
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
                    if workspace.save_clean_window_session(cx).is_err() {
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
