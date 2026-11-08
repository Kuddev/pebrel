//! A load owns its network cancellation, progress subscription and generation.

use super::*;
use crate::ssh_sftp::preview::ReadProgress;
use std::sync::Arc;
use tokio::sync::watch;

impl TextFileView {
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.cancel_object_edit(cx);
        self.loading = true;
        self.document = None;
        self.dirty = false;
        self.notice = None;
        self.content_revision = self.content_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;
        self.preview_task = None;
        self.load_task = None;
        self.progress_task = None;
        self.load_received = 0;
        self.load_total = None;
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        let path = self.path.clone();
        let source = self.source.clone();
        let operation = DocumentOperation::default();
        if let Some(previous) = self.operation.replace(operation.clone()) {
            previous.cancel();
        }
        let progress = source.is_remote().then(|| {
            let (send, receive) = watch::channel(ReadProgress::default());
            self.observe_progress(generation, receive, window, cx);
            send
        });
        let markdown = self.markdown;
        let resources = super::super::scientific_render::assets(cx);
        let task = cx.background_executor().spawn(async move {
            let _permit = resources.document_permit().await;
            source.load(operation, progress).await.map(|doc| {
                let outline = markdown.then(|| Outline::prepare(&doc.text, path.parent()));
                (doc, outline)
            })
        });
        let handle = window.window_handle();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    if view.load_generation != generation {
                        return;
                    }
                    let loaded =
                        if view.operation.as_ref().is_some_and(DocumentOperation::is_cancelled) {
                            Err(std::io::Error::new(
                                std::io::ErrorKind::Interrupted,
                                "File read cancelled",
                            ))
                        } else {
                            loaded
                        };
                    view.loading = false;
                    view.operation = None;
                    view.progress_task = None;
                    match loaded {
                        Ok((document, outline)) => {
                            view.live_edit = None;
                            view.last_edit_cursor = None;
                            view.resume_edit = false;
                            view.history.reset(&document.text);
                            view.input.update(cx, |input, cx| {
                                let offset = input.scroll_offset();
                                input.set_value(document.text.clone(), window, cx);
                                input.set_scroll_offset(offset, cx);
                            });
                            view.document = Some(document);
                            view.dirty = false;
                            view.notice = None;
                            if let Some(outline) = outline {
                                view.apply_outline(outline, cx);
                            }
                            if view.preview && view.focus.is_focused(window) {
                                view.begin_live_edit(0, window, cx);
                            }
                        },
                        Err(error) => {
                            view.notice =
                                Some(if error.kind() == std::io::ErrorKind::Interrupted {
                                    (Message::TransferCancelled, None)
                                } else {
                                    (Message::EditorReadFailed, Some(error.to_string()))
                                });
                        },
                    }
                    cx.emit(TextFileEvent::Changed);
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    fn observe_progress(
        &mut self,
        generation: u64,
        mut progress: watch::Receiver<ReadProgress>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let handle = window.window_handle();
        let executor = cx.background_executor().clone();
        self.progress_task = Some(cx.spawn(async move |this, cx| {
            let mut prefix_len = 0;
            while progress.changed().await.is_ok() {
                let snapshot = progress.borrow_and_update().clone();
                let text = if let Some(prefix) = snapshot.prefix.filter(|p| p.len() > prefix_len) {
                    prefix_len = prefix.len();
                    executor.spawn(async move { text_prefix(prefix) }).await
                } else {
                    None
                };
                let applied = handle
                    .update(cx, |_, window, cx| {
                        this.update(cx, |view, cx| {
                            if !view.loading || view.load_generation != generation {
                                return false;
                            }
                            view.load_received = snapshot.received;
                            view.load_total = snapshot.total;
                            if let Some(text) = text {
                                view.input.update(cx, |input, cx| {
                                    let offset = input.scroll_offset();
                                    input.set_value(text, window, cx);
                                    input.set_scroll_offset(offset, cx);
                                });
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if !applied {
                    break;
                }
            }
        }));
    }
}

fn text_prefix(bytes: Arc<[u8]>) -> Option<String> {
    // A network chunk may split the last UTF-8 codepoint. Invalid bytes in the
    // middle or binary NULs must not be presented as an editable text preview.
    let end = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        Err(_) => return None,
    };
    if bytes[..end].contains(&0) {
        return None;
    }
    Some(crate::text_document::TextSnapshot::decode_prefix(bytes[..end].to_vec(), true, true).text)
}
