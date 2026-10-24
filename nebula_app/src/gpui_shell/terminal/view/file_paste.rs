//! SSH file paste belongs to one input lifetime, from native clipboard to upload.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, Keystroke, ParentElement as _,
    Styled as _, Task, Window,
};

use super::{TerminalView, ui_language};
use crate::display::side_panel::{PathQuote, drop_text_for_paths};
use crate::gpui_shell::prelude::*;
use crate::gpui_shell::toast::{self, ToastKind};
use crate::i18n::Message;
use crate::ssh_sftp::clipboard_files::{self, Operation, UploadedFiles};

#[derive(Default)]
pub(super) struct FilePasteState {
    operation: Option<Operation>,
    task: Option<Task<()>>,
    count: usize,
}

impl FilePasteState {
    pub(super) fn cancel(&self) {
        if let Some(operation) = &self.operation {
            operation.cancel();
        }
    }
}

impl Drop for FilePasteState {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl TerminalView {
    pub(super) fn paste_native_files(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.ssh_destination.is_none() {
            return false;
        }
        match crate::platform::clipboard_files::read() {
            Ok(Some(paths)) => self.paste_files(paths, window, cx),
            Ok(None) => return false,
            Err(error) => Self::file_paste_failed(&error.to_string(), window, cx),
        }
        true
    }

    fn paste_files(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_paste.operation.is_some() {
            toast::toast(
                window,
                cx,
                ToastKind::Info,
                ui_language().text(Message::CommonFilePasteBusy),
            );
            return;
        }
        let source = self
            .session
            .as_ref()
            .and_then(|session| session.remote_sftp.lock().ok()?.clone())
            .filter(|source| source.is_open());
        let Some(source) = source.filter(|_| self.ready_ssh_destination().is_some()) else {
            toast::toast(
                window,
                cx,
                ToastKind::Warning,
                ui_language().text(Message::CommonFilePasteDisconnected),
            );
            return;
        };
        if self.session.is_none() || !self.focus_handle.is_focused(window) {
            return;
        }
        let runtime = match crate::ssh_session::runtime() {
            Ok(runtime) => runtime,
            Err(error) => {
                Self::file_paste_failed(&error.to_string(), window, cx);
                return;
            },
        };
        let count = paths.len();
        let directory = self.remote_cwd();
        let upload_source = source.clone();
        let operation = Operation::default();
        let upload_operation = operation.clone();
        let (sender, receiver) = futures::channel::oneshot::channel();
        runtime.spawn(async move {
            let result =
                clipboard_files::upload(upload_source, directory, paths, upload_operation).await;
            // An abandoned result drops its file owner and removes unaccepted uploads.
            let _ = sender.send(result);
        });
        self.start_file_paste(
            count,
            operation,
            Some(source),
            async move { receiver.await.map_err(|_| "File upload stopped".to_owned())? },
            window,
            cx,
        );
    }

    fn start_file_paste(
        &mut self,
        count: usize,
        operation: Operation,
        source: Option<crate::ssh_session::SftpSource>,
        work: impl Future<Output = Result<UploadedFiles, String>> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let term = self.session.as_ref().map(|session| Arc::downgrade(&session.term));
        let owner = (
            self.running_program.clone(),
            self.ai_session.clone(),
            self.agent_activity.primary_pid(),
            self.command_started,
        );
        let destination = self.ssh_destination.clone();
        self.file_paste.operation = Some(operation.clone());
        self.file_paste.count = count;
        self.file_paste.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.file_paste.operation = None;
                view.file_paste.task = None;
                let same_session = view.session.as_ref().is_some_and(|session| {
                    term.as_ref()
                        .and_then(|term| term.upgrade())
                        .is_some_and(|term| Arc::ptr_eq(&term, &session.term))
                });
                let current_owner = (
                    view.running_program.clone(),
                    view.ai_session.clone(),
                    view.agent_activity.primary_pid(),
                    view.command_started,
                );
                let valid = !operation.is_cancelled()
                    && source.as_ref().is_none_or(|source| source.is_open())
                    && same_session
                    && owner == current_owner
                    && destination.as_deref() == view.ready_ssh_destination()
                    && view.output_visible
                    && view.marked_text.is_none()
                    && window.is_window_active()
                    && view.focus_handle.is_focused(window);
                match result {
                    Ok(uploaded) if valid => {
                        if let Some(text) = drop_text_for_paths(uploaded.paths(), PathQuote::Posix)
                        {
                            // Never broadcast a host-specific path or submit the user's line.
                            view.paste_now_impl(&text, false, cx);
                            uploaded.keep();
                        } else {
                            Self::file_paste_failed(
                                "The uploaded paths cannot be pasted safely",
                                window,
                                cx,
                            );
                        }
                    },
                    Ok(_) => toast::toast(
                        window,
                        cx,
                        ToastKind::Info,
                        ui_language().text(Message::CommonFilePasteCancelled),
                    ),
                    Err(_) if operation.is_cancelled() => toast::toast(
                        window,
                        cx,
                        ToastKind::Info,
                        ui_language().text(Message::CommonFilePasteCancelled),
                    ),
                    Err(error) => Self::file_paste_failed(&error, window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn file_paste_failed(error: &str, window: &mut Window, cx: &mut Context<Self>) {
        toast::toast(
            window,
            cx,
            ToastKind::Warning,
            ui_language().format(Message::CommonFilePasteFailed, &[("error", error)]),
        );
    }

    pub(super) fn cancel_file_paste_key(
        &mut self,
        key: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.file_paste.operation.is_some()
            && key.key == "escape"
            && !key.modifiers.control
            && !key.modifiers.alt
            && !key.modifiers.platform
        {
            self.file_paste.cancel();
            cx.notify();
            return true;
        }
        false
    }

    pub(super) fn file_paste_indicator(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let operation = self.file_paste.operation.as_ref()?;
        let cancelled = operation.is_cancelled();
        let text = if cancelled {
            ui_language().text(Message::CommonFilePasteCancelling).to_owned()
        } else {
            ui_language().format(
                Message::CommonFilePasteUploading,
                &[("count", &self.file_paste.count.to_string())],
            )
        };
        let cancel = crate::gpui_shell::widgets::settings_button(
            Button::new("ssh-file-paste-cancel")
                .debug_selector(|| "ssh-file-paste-cancel".into())
                .label(ui_language().text(Message::CommonCancel))
                .disabled(cancelled)
                .on_click(cx.listener(|view, _, _, cx| {
                    view.file_paste.cancel();
                    cx.notify();
                })),
            false,
            cx,
        );
        Some(
            h_flex()
                .id("ssh-file-paste")
                .debug_selector(|| "ssh-file-paste".into())
                .absolute()
                .bottom_2()
                .left_2()
                .right_2()
                .p_2()
                .gap_2()
                .flex_wrap()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .text_sm()
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .occlude()
                .child(text)
                .child(cancel)
                .into_any_element(),
        )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
