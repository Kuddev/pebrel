//! Upload ownership ends on submission, focus loss, disconnect or pane destruction.
use super::*;
use crate::gpui_shell::prelude::*;
use crate::ssh_sftp::intake::{FileIntake, UploadedPaths};
use gpui::{
    AnyElement, InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _, div, px,
};

impl TerminalView {
    pub(super) fn upload_dropped_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(destination) = self.ready_ssh_destination().map(str::to_owned) else {
            toast::toast(
                window,
                cx,
                ToastKind::Warning,
                ui_language().text(Message::CommonFileUploadDisconnected),
            );
            return;
        };
        let connection = self
            .session
            .as_ref()
            .and_then(|session| session.remote_connection.lock().ok()?.clone());
        let Some(connection) = connection else {
            toast::toast(
                window,
                cx,
                ToastKind::Warning,
                ui_language().text(Message::CommonFileUploadDisconnected),
            );
            return;
        };
        if self.path_drop.pending {
            toast::toast(
                window,
                cx,
                ToastKind::Warning,
                ui_language().text(Message::CommonFileUploadBusy),
            );
            return;
        }
        let (wake_tx, wake_rx) = tokio::sync::mpsc::channel(1);
        let wake = Arc::new(move || {
            let _ = wake_tx.try_send(());
        });
        match FileIntake::start(destination, connection, self.remote_cwd(), paths, wake) {
            Ok((intake, receive)) => {
                self.await_uploaded_paths(intake, receive, wake_rx, window, cx)
            },
            Err(error) => self.file_upload_error(&error.to_string(), window, cx),
        }
    }

    fn await_uploaded_paths(
        &mut self,
        intake: FileIntake,
        mut receive: tokio::sync::oneshot::Receiver<Result<UploadedPaths, String>>,
        mut wake: tokio::sync::mpsc::Receiver<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let term = Arc::downgrade(&self.session.as_ref().unwrap().term);
        let generation = self.path_drop.generation;
        let destination = self.ssh_destination.clone();
        let owner = (
            self.running_program.clone(),
            self.ai_session.clone(),
            self.agent_activity.primary_pid(),
            self.command_started,
        );
        self.path_drop.pending = true;
        self.path_drop.upload = Some(intake);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = loop {
                tokio::select! {
                    result = &mut receive => break result.unwrap_or_else(|_| Err("File upload stopped".to_owned())),
                    Some(()) = wake.recv() => {
                        if this.update(cx, |_, cx| cx.notify()).is_err() { return; }
                    },
                }
            };
            let _ = this.update_in(cx, |view, window, cx| {
                view.path_drop.pending = false;
                view.path_drop.upload = None;
                cx.notify();
                let same_term = view.session.as_ref().is_some_and(|session| {
                    term.upgrade().is_some_and(|term| Arc::ptr_eq(&term, &session.term))
                });
                let current_owner = (view.running_program.clone(), view.ai_session.clone(), view.agent_activity.primary_pid(), view.command_started);
                if !same_term
                    || view.ready_ssh_destination() != destination.as_deref()
                    || generation != view.path_drop.generation
                    || owner != current_owner
                    || !view.focus_handle.is_focused(window)
                {
                    toast::toast(window, cx, ToastKind::Warning, ui_language().text(Message::CommonDropPathsExpired));
                    return;
                }
                match result {
                    Ok(uploaded) => {
                        if drop_text_for_paths(&uploaded.paths, PathQuote::Posix).is_some() {
                            view.paste_dropped_paths(&uploaded.paths, window, cx);
                            uploaded.retain();
                        } else {
                            toast::toast(window, cx, ToastKind::Warning, ui_language().text(Message::CommonDropPathsInvalid));
                        }
                    },
                    Err(error) => view.file_upload_error(&error, window, cx),
                }
            });
        }).detach();
    }

    fn file_upload_error(&self, error: &str, window: &mut Window, cx: &mut Context<Self>) {
        toast::toast(
            window,
            cx,
            ToastKind::Warning,
            ui_language().format(Message::CommonFileUploadFailed, &[("error", error)]),
        );
    }

    pub(in super::super) fn file_upload_indicator(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let upload = self.path_drop.upload.as_ref()?;
        let progress = upload.progress();
        let cancelled = upload.cancelled();
        let language = ui_language();
        let label = if cancelled {
            language.text(Message::CommonFileUploadCancelling).to_owned()
        } else if progress.total == 0 {
            language.text(Message::CommonFileUploadPreparing).to_owned()
        } else {
            language.format(
                Message::CommonFileUploadProgress,
                &[("percent", &format!("{:.0}", progress.fraction() * 100.0))],
            )
        };
        Some(
            h_flex()
                .debug_selector(|| "file-upload-progress".into())
                .absolute()
                .top(px(4.0))
                .right(px(12.0))
                .gap_2()
                .px_2()
                .py_1()
                .rounded(px(4.0))
                .bg(cx.theme().popover)
                .text_color(cx.theme().foreground)
                .text_sm()
                .child(div().child(label))
                .child(
                    Button::new("cancel-file-upload")
                        .debug_selector(|| "cancel-file-upload".into())
                        .ghost()
                        .small()
                        .label(language.text(Message::CommonCancel))
                        .disabled(cancelled)
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.path_drop.invalidate();
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
