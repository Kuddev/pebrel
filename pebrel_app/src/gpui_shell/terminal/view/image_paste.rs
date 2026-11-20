//! Clipboard intake, image staging and paste confirmation share one user action.
//! Image files belong to this pane; asynchronous results never broadcast paths.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use gpui::{ClipboardEntry, ClipboardItem, Context, ParentElement as _, SharedString, Window};
use gpui_component::{Sizable as _, WindowExt as _, checkbox::Checkbox};

use super::{TerminalView, paste_line_count, paste_needs_confirmation, ui_language};
use crate::gpui_shell::prelude::{ButtonVariant, confirm_dialog};
use crate::gpui_shell::toast::{self, ToastKind};
use crate::i18n::Message;

mod lifetime;
use lifetime::{Attachment, Attachments, ImageOwner, Retire};

const MAX_STAGED_BYTES: usize = 128 * 1024 * 1024;

#[derive(Default)]
pub(super) struct ImagePasteState {
    pending: bool,
    generation: u64,
    attachments: Attachments,
    cleanup: Option<gpui::Task<()>>,
}

impl ImagePasteState {
    pub(super) fn observe_input(&mut self, bytes: &[u8]) {
        if self.pending
            && !bytes.starts_with(b"\x1b[200~")
            && bytes.iter().any(|byte| matches!(byte, b'\r' | b'\n' | 3 | 26))
        {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub(super) fn observe_key(&mut self, key: &gpui::Keystroke) {
        if self.pending
            && (key.key == "enter"
                || key.modifiers.control && matches!(key.key.as_str(), "c" | "z"))
        {
            self.generation = self.generation.wrapping_add(1);
        }
    }
}

enum ClipboardPayload {
    Text(String),
    Image(Vec<u8>),
    Files(Vec<std::path::PathBuf>),
}

fn clipboard_payload(item: ClipboardItem) -> Option<ClipboardPayload> {
    // Native file copies may also offer a text representation. Only the file
    // payload authorizes reading/uploading; path-looking text stays ordinary text.
    let files: Vec<_> = item
        .entries
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::ExternalPaths(paths) => Some(paths.paths()),
            _ => None,
        })
        .flatten()
        .cloned()
        .collect();
    if !files.is_empty() {
        return Some(ClipboardPayload::Files(files));
    }
    if let Some(text) = item.text().filter(|text| !text.is_empty()) {
        return Some(ClipboardPayload::Text(text));
    }
    item.into_entries().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(ClipboardPayload::Image(image.bytes)),
        _ => None,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ImageTarget {
    Host,
    Wsl(Option<String>),
    Ssh(String),
}

struct StagedImage {
    path: String,
    attachment: Attachment,
}

enum StageError {
    Budget,
    Failed(String),
}

impl TerminalView {
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(payload) = cx.read_from_clipboard().and_then(clipboard_payload) else { return };
        match payload {
            ClipboardPayload::Text(text) => {
                if self.try_native_clipboard_paste(&text, true, cx) {
                    return;
                }
                let lines = paste_line_count(&text);
                if pebrel_settings::RuntimeSettings::load().multiline_paste_confirm
                    && paste_needs_confirmation(&text, self.term_mode())
                {
                    self.confirm_paste(text, lines, window, cx);
                } else {
                    self.paste_now(&text, cx);
                }
            },
            ClipboardPayload::Image(bytes) => self.paste_image(bytes, window, cx),
            ClipboardPayload::Files(paths) => self.drop_external_paths(&paths, window, cx),
        }
    }

    fn image_target(&self) -> ImageTarget {
        if let Some(destination) = &self.ssh_destination {
            return ImageTarget::Ssh(destination.clone());
        }
        self.exec_context
            .as_ref()
            .and_then(|context| context.wsl_distribution())
            .map_or(ImageTarget::Host, |distro| ImageTarget::Wsl(distro.map(str::to_owned)))
    }

    fn paste_image(&mut self, bytes: Vec<u8>, window: &mut Window, cx: &mut Context<Self>) {
        self.image_paste.attachments.forget_disconnected_files();
        let issue = if self.session.is_none() || self.exited.is_some() {
            Some(Message::CommonImagePasteExpired)
        } else if self.image_paste.pending {
            Some(Message::CommonImagePasteBusy)
        } else if self.image_paste.attachments.remaining() == 0 {
            log::warn!("Pasted images reached the 128 MiB limit for pane {}", self.pane_id);
            Some(Message::CommonImagePasteLimit)
        } else if self.ssh_destination.is_some() && self.ready_ssh_destination().is_none() {
            Some(Message::CommonImagePasteDisconnected)
        } else {
            None
        };
        if let Some(issue) = issue {
            toast::toast(window, cx, ToastKind::Warning, ui_language().text(issue));
            return;
        }

        let target = self.image_target();
        let connection = self
            .session
            .as_ref()
            .and_then(|session| session.remote_connection.lock().ok()?.clone());
        if matches!(target, ImageTarget::Ssh(_)) && connection.is_none() {
            toast::toast(
                window,
                cx,
                ToastKind::Warning,
                ui_language().text(Message::CommonImagePasteDisconnected),
            );
            return;
        }
        let staged_target = target.clone();
        let term = Arc::downgrade(&self.session.as_ref().unwrap().term);
        let owner = (
            self.running_program.clone(),
            self.ai_session.clone(),
            self.agent_activity.primary_pid(),
            self.command_started,
        );
        let generation = self.image_paste.generation;
        let remaining = self.image_paste.attachments.remaining();
        self.image_paste.pending = true;
        let work = cx.background_executor().spawn(async move {
            let png =
                super::super::inline_image::clipboard_png(&bytes).map_err(StageError::Failed)?;
            if png.len() > remaining {
                return Err(StageError::Budget);
            }
            stage_image(staged_target, connection, png).await.map_err(StageError::Failed)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.image_paste.pending = false;
                let same_term = view.session.as_ref().is_some_and(|session| {
                    term.upgrade().is_some_and(|term| Arc::ptr_eq(&term, &session.term))
                });
                let current_owner = (
                    view.running_program.clone(),
                    view.ai_session.clone(),
                    view.agent_activity.primary_pid(),
                    view.command_started,
                );
                if !same_term
                    || view.exited.is_some()
                    || generation != view.image_paste.generation
                    || owner != current_owner
                    || target != view.image_target()
                {
                    toast::toast(
                        window,
                        cx,
                        ToastKind::Warning,
                        ui_language().text(Message::CommonImagePasteExpired),
                    );
                    return;
                }
                match result {
                    Ok(staged) => {
                        let current_connection = view
                            .session
                            .as_ref()
                            .and_then(|session| session.remote_connection.lock().ok()?.clone());
                        if !staged.attachment.belongs_to(current_connection.as_ref()) {
                            toast::toast(
                                window,
                                cx,
                                ToastKind::Warning,
                                ui_language().text(Message::CommonImagePasteExpired),
                            );
                            return;
                        }
                        let owner = if view
                            .running_program
                            .as_deref()
                            .and_then(crate::ai_agents::AgentKind::parse)
                            .is_some()
                        {
                            ImageOwner::AgentDraft
                        } else if view.command_running && !view.command_running_disproved {
                            ImageOwner::Shell
                        } else {
                            ImageOwner::Draft
                        };
                        view.image_paste.attachments.push(staged.attachment, owner);
                        // A staged path is valid for this endpoint only.
                        view.paste_now_impl(&staged.path, false, cx);
                    },
                    Err(StageError::Budget) => {
                        log::warn!(
                            "Pasted image would exceed the 128 MiB limit for pane {}",
                            view.pane_id
                        );
                        toast::toast(
                            window,
                            cx,
                            ToastKind::Warning,
                            ui_language().text(Message::CommonImagePasteLimit),
                        );
                    },
                    Err(StageError::Failed(error)) => {
                        log::warn!("image paste failed: {error}");
                        toast::toast(
                            window,
                            cx,
                            ToastKind::Warning,
                            ui_language()
                                .format(Message::CommonImagePasteFailed, &[("error", &error)]),
                        );
                    },
                }
            });
        })
        .detach();
    }

    pub(super) fn assign_pasted_images_to_command(&mut self) {
        self.image_paste.attachments.assign(ImageOwner::Draft, ImageOwner::Shell);
    }

    pub(super) fn retire_command_images(&mut self, cx: &mut Context<Self>) {
        self.retire_pasted_images(Retire::Command, cx);
    }

    pub(super) fn observe_agent_image_lifetime(
        &mut self,
        event: &crate::ai_hook::AiHookEvent,
        cx: &mut Context<Self>,
    ) {
        use crate::ai_hook::AiHookKind;

        match event.kind {
            AiHookKind::PromptSubmit => {
                self.image_paste.attachments.assign(ImageOwner::AgentDraft, ImageOwner::Agent);
            },
            AiHookKind::SessionEnd => self.retire_pasted_images(Retire::AgentSession, cx),
            AiHookKind::TurnDone if event.active_background_tasks() == 0 => {
                self.retire_pasted_images(Retire::AgentTurn, cx);
            },
            _ => {},
        }
    }

    fn retire_pasted_images(&mut self, source: Retire, cx: &mut Context<Self>) {
        self.image_paste.attachments.retire(source, Instant::now());
        self.schedule_image_paste_cleanup(cx);
    }

    fn schedule_image_paste_cleanup(&mut self, cx: &mut Context<Self>) {
        if self.image_paste.cleanup.is_some() {
            return;
        }
        let Some(expires) = self.image_paste.attachments.next_expiry() else { return };
        let timer =
            cx.background_executor().timer(expires.saturating_duration_since(Instant::now()));
        self.image_paste.cleanup = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let Ok(cleanup) = this.update(cx, |view, cx| {
                let expired = view.image_paste.attachments.take_expired(Instant::now());
                cx.background_executor().spawn(lifetime::remove(expired))
            }) else {
                return;
            };
            let result = cleanup.await;
            let _ = this.update(cx, |view, cx| {
                view.image_paste.cleanup = None;
                view.image_paste.attachments.finish_cleanup(result, Instant::now());
                view.schedule_image_paste_cleanup(cx);
            });
        }));
    }

    // Freeze text with the dialog so a later clipboard change cannot alter it.
    fn confirm_paste(
        &mut self,
        text: String,
        lines: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use pebrel_terminal::term::TermMode;

        let language = ui_language();
        let runs_line_by_line = !self.term_mode().contains(TermMode::BRACKETED_PASTE);
        let title: SharedString =
            language.format(Message::CommonPasteLines, &[("lines", &lines.to_string())]).into();
        let body: SharedString = if runs_line_by_line {
            language.pick(
                "shell 会把这些内容逐行执行。请确认来源可信。",
                "The shell will run these lines one by one. Make sure you trust the source.",
            )
        } else {
            language.pick(
                "内容会作为一整块交给当前程序，不会逐行执行；但行数不少，请确认来源可信。",
                "The app receives this as a single paste and will not run it line by line, but \
                 it is a lot of text — make sure you trust the source.",
            )
        }
        .into();
        let ok_text: SharedString = language.pick("粘贴", "Paste").into();
        let cancel_text: SharedString = language.pick("取消", "Cancel").into();
        let text = Arc::new(text);
        let view = cx.entity().downgrade();
        let never_ask_again = Arc::new(std::sync::atomic::AtomicBool::new(false));
        window.open_dialog(cx, move |dialog, window, _cx| {
            let text = text.clone();
            let view = view.clone();
            let checked = never_ask_again.load(std::sync::atomic::Ordering::Relaxed);
            let checkbox_state = never_ask_again.clone();
            let persist_state = never_ask_again.clone();
            confirm_dialog(
                dialog,
                window,
                title.clone(),
                body.clone(),
                ok_text.clone(),
                cancel_text.clone(),
                ButtonVariant::Primary,
            )
            .child(
                Checkbox::new("nebula-paste-never-ask")
                    .label(language.pick("不再询问", "Don't ask again"))
                    .checked(checked)
                    .small()
                    .on_click(move |checked, window, _| {
                        checkbox_state.store(*checked, std::sync::atomic::Ordering::Relaxed);
                        window.refresh();
                    }),
            )
            .on_ok(move |_, _window, cx| {
                if persist_state.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = pebrel_settings::persist_keys(&[(
                        "multiline_paste_confirm",
                        "0".to_owned(),
                    )]);
                }
                let _ = view.update(cx, |this, cx| this.paste_now(&text, cx));
                true
            })
        });
    }
}

async fn stage_image(
    target: ImageTarget,
    connection: Option<crate::ssh_session::PaneConnection>,
    png: Vec<u8>,
) -> Result<StagedImage, String> {
    let bytes = png.len();
    if let ImageTarget::Ssh(_) = target {
        let (sender, receiver) = futures::channel::oneshot::channel();
        let connection = connection.ok_or("SSH pane connection closed")?;
        let owner = connection.clone();
        crate::ssh_sftp::upload_clipboard_image_on_pane(connection, png, move |result| {
            let staged = result.map(|path| StagedImage {
                attachment: Attachment::remote(path.clone(), owner, bytes),
                path,
            });
            let _ = sender.send(staged);
        });
        return receiver.await.map_err(|_| "image upload stopped".to_owned())?;
    }
    let file = crate::clipboard::stage_image_png(&png).map_err(|error| error.to_string())?;
    let path = match target {
        ImageTarget::Host => file.to_str().ok_or("image path is not UTF-8")?.to_owned(),
        ImageTarget::Wsl(distro) => wsl_image_path(distro.as_deref(), &file)?,
        ImageTarget::Ssh(_) => unreachable!(),
    };
    if path.chars().any(char::is_control) {
        return Err("image path contains a control character".to_owned());
    }
    Ok(StagedImage { path, attachment: Attachment::local(file, bytes) })
}

#[cfg(windows)]
fn wsl_image_path(distro: Option<&str>, path: &Path) -> Result<String, String> {
    use std::time::Duration;

    let system = std::env::var_os("SystemRoot").ok_or("Windows system directory is unavailable")?;
    let executable = Path::new(&system).join("System32").join("wsl.exe");
    let command = || {
        let mut command = std::process::Command::new(&executable);
        crate::platform::process::hidden_command(&mut command);
        if let Some(distro) = distro {
            command.args(["--distribution", distro]);
        }
        command.arg("--exec");
        command
    };
    let mut convert = command();
    convert.args(["wslpath", "-u"]).arg(path);
    let output = crate::display::side_panel::command_output_with_timeout(
        convert,
        Some(Duration::from_secs(30)),
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("WSL could not resolve the clipboard image path".to_owned());
    }
    let guest = parse_wsl_image_path(&output.stdout)?;
    let mut readable = command();
    readable.args(["test", "-r", &guest]);
    let output = crate::display::side_panel::command_output_with_timeout(
        readable,
        Some(Duration::from_secs(30)),
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("the WSL distribution cannot read the clipboard image".to_owned());
    }
    Ok(guest)
}

#[cfg(not(windows))]
fn wsl_image_path(_distro: Option<&str>, _path: &Path) -> Result<String, String> {
    Err("WSL image staging requires Windows".to_owned())
}

fn parse_wsl_image_path(bytes: &[u8]) -> Result<String, String> {
    let path = std::str::from_utf8(bytes)
        .map_err(|_| "WSL image path is not UTF-8")?
        .trim_end_matches(['\r', '\n']);
    if !path.starts_with('/') || path.chars().any(char::is_control) {
        return Err("WSL returned an invalid image path".to_owned());
    }
    Ok(path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_only_clipboard_is_preserved_and_text_keeps_priority() {
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]);
        let item = ClipboardItem::new_image(&image);
        assert!(
            matches!(clipboard_payload(item), Some(ClipboardPayload::Image(bytes)) if bytes == [1, 2, 3])
        );
        let mut mixed = ClipboardItem::new_image(&image);
        mixed.entries.push(ClipboardEntry::String(gpui::ClipboardString::new("text".into())));
        assert!(
            matches!(clipboard_payload(mixed), Some(ClipboardPayload::Text(text)) if text == "text")
        );
    }

    #[test]
    fn native_files_take_priority_but_path_looking_text_never_uploads() {
        let path = std::path::PathBuf::from("report.pdf");
        let item = ClipboardItem {
            entries: vec![
                ClipboardEntry::String(gpui::ClipboardString::new("report.pdf".into())),
                ClipboardEntry::ExternalPaths(gpui::ExternalPaths(vec![path.clone()].into())),
            ],
        };
        assert!(
            matches!(clipboard_payload(item), Some(ClipboardPayload::Files(files)) if files == [path])
        );
        for text in ["C:\\Users\\me\\report.pdf", "/home/me/report.pdf"] {
            assert!(
                matches!(clipboard_payload(ClipboardItem::new_string(text.into())), Some(ClipboardPayload::Text(value)) if value == text)
            );
        }
    }

    #[test]
    fn submission_cancels_pending_image_but_typing_and_bracketed_paste_do_not() {
        let mut state = ImagePasteState { pending: true, ..ImagePasteState::default() };
        state.observe_input(b"prompt");
        state.observe_input(b"\x1b[200~line one\rline two\x1b[201~");
        assert_eq!(state.generation, 0);
        state.observe_input(b"\r");
        assert_eq!(state.generation, 1);
        state.observe_key(&gpui::Keystroke {
            modifiers: gpui::Modifiers { control: true, ..gpui::Modifiers::default() },
            key: "c".to_owned(),
            key_char: None,
        });
        state.observe_key(&gpui::Keystroke {
            modifiers: gpui::Modifiers::default(),
            key: "enter".to_owned(),
            key_char: None,
        });
        assert_eq!(state.generation, 3);
    }

    #[test]
    fn wsl_paths_preserve_spaces_and_reject_host_or_multiline_paths() {
        assert_eq!(
            parse_wsl_image_path(b"/custom/c/My Images/paste.png\r\n").unwrap(),
            "/custom/c/My Images/paste.png"
        );
        assert!(parse_wsl_image_path(b"C:\\paste.png\r\n").is_err());
        assert!(parse_wsl_image_path(b"/tmp/paste.png\ncommand").is_err());
        assert!(parse_wsl_image_path(&[0xff]).is_err());
    }

    #[test]
    fn staged_files_are_unique_and_removed_when_their_owner_is_dropped() {
        let first = crate::clipboard::stage_image_png(b"first").unwrap();
        let second = crate::clipboard::stage_image_png(b"second").unwrap();
        assert_ne!(first.to_path_buf(), second.to_path_buf());
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        let path = first.to_path_buf();
        drop(first);
        assert!(!path.exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
    }
}
