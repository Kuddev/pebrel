//! Mobile/Runtime adapters for existing tab lifecycles and immutable reader snapshots.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Arc,
};

use base64::Engine as _;
use gpui::{App, Context, WeakEntity, Window};
use gpui_component::Rope;
use serde_json::{Value, json};

use super::{NebulaWorkspace, WorkspaceTab};
use crate::runtime_api::{
    ApiError, RuntimeCommand, RuntimeDispatch,
    tabs::{self, FileInfo, Request},
};

impl WorkspaceTab {
    pub(super) fn runtime_file_info(&self, cx: &App) -> Option<FileInfo> {
        if let Some(file) = self.file_editor(cx) {
            let view = file.read(cx);
            return Some(FileInfo {
                path: view.path.to_string_lossy().into_owned(),
                remote: view.reader_is_remote(),
                dirty: view.is_dirty(),
                saving: view.is_saving(),
                ready: view.reader_revision().is_some(),
                revision: view.reader_revision(),
            });
        }
        match self {
            Self::Image { view } => Some(FileInfo {
                path: view.read(cx).path.to_string_lossy().into_owned(),
                remote: false,
                dirty: false,
                saving: false,
                ready: true,
                revision: None,
            }),
            _ => None,
        }
    }
}

impl NebulaWorkspace {
    fn runtime_tab_index(&self, id: &str) -> Result<usize, ApiError> {
        self.tab_meta.iter().position(|meta| meta.runtime_id.0 == id).ok_or_else(|| {
            ApiError::new(
                "target_not_found",
                "the requested tab was closed or moved to another window",
            )
        })
    }

    pub(super) fn execute_tab_request(
        &mut self,
        window_id: u64,
        request: &Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, ApiError> {
        self.runtime_window_requested(Some(window_id))?;
        match request {
            Request::Close { tab_id } => {
                let index = self.runtime_tab_index(tab_id)?;
                self.close_runtime_tab(index, window, cx)
            },
            Request::Focus { tab_id } => {
                let index = self.runtime_tab_index(tab_id)?;
                self.activate_tab(index, window, cx);
                self.runtime_result(json!({"window_id": window_id, "tab_id": tab_id}), window, cx)
            },
            Request::Open { path } => {
                // 此入口只打开应用内阅读器，未知扩展名绝不交给系统关联程序执行。
                if !crate::gpui_shell::doc_tabs::openable_in_app(path) {
                    return Err(ApiError::new(
                        "unsupported_file",
                        "this file type has no in-app reader",
                    ));
                }
                self.open_document_path(path.clone(), window, cx);
                let id = self.tab_meta[self.active].runtime_id.0.clone();
                self.runtime_result(json!({"window_id": window_id, "tab_id": id}), window, cx)
            },
            Request::Read { .. } => Err(ApiError::new(
                "runtime_unavailable",
                "file reads require the background dispatcher",
            )),
        }
    }

    pub(super) fn close_runtime_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, ApiError> {
        let tab = self
            .tabs
            .get(index)
            .ok_or_else(|| ApiError::new("target_not_found", "the requested tab does not exist"))?;
        if let Some(file) = tab.file_editor(cx) {
            let file = file.read(cx);
            if file.is_saving() || file.is_dirty() {
                // 普通关闭会异步弹桌面对话框；RPC 在真正关闭之前必须保留失败状态。
                return Err(ApiError::new(
                    if file.is_saving() { "file_saving" } else { "unsaved_changes" },
                    "save or resolve this document on the computer before closing its shared tab",
                ));
            }
        }
        if let Some(process) = self.busy_process_in_tab(index, None, cx) {
            return Err(ApiError::new(
                "confirmation_required",
                format!("{process} is still running; confirm closure on the computer"),
            ));
        }
        let tab_id = self.tab_meta[index].runtime_id.0.clone();
        self.close_tab(index, window, cx);
        if self.tab_meta.iter().any(|meta| meta.runtime_id.0 == tab_id) {
            return Err(ApiError::new("close_pending", "the tab has not closed"));
        }
        let action = json!({"window_id": self.runtime_window_id, "tab_index": index, "tab_id": tab_id, "closed": true});
        if self.tabs.is_empty() {
            Ok(
                json!({"action": action, "snapshot": super::windowing::publish_runtime_snapshot(cx)}),
            )
        } else {
            self.runtime_result(action, window, cx)
        }
    }

    fn reader_source(
        &self,
        window_id: u64,
        tab_id: &str,
        cx: &App,
    ) -> Result<ReaderSource, ApiError> {
        self.runtime_window_requested(Some(window_id))?;
        let tab = &self.tabs[self.runtime_tab_index(tab_id)?];
        if let Some(file) = tab.file_editor(cx) {
            return file
                .read(cx)
                .reader_snapshot(cx)
                .map(|(text, revision)| ReaderSource::Text(text, revision))
                .ok_or_else(|| {
                    ApiError::new("file_not_ready", "the computer has not loaded this document")
                });
        }
        if let WorkspaceTab::Image { view } = tab {
            return Ok(ReaderSource::Image(view.read(cx).path.clone()));
        }
        Err(ApiError::new("unsupported_file", "this tab has no readable file content"))
    }
}

enum ReaderSource {
    Text(Rope, u64),
    Image(PathBuf),
}

pub(super) fn dispatch_read(
    dispatch: &Arc<RuntimeDispatch>,
    workspace: &WeakEntity<NebulaWorkspace>,
    cx: &App,
) -> bool {
    let RuntimeCommand::Tab {
        window_id,
        request: Request::Read { tab_id, offset, limit, revision },
    } = &dispatch.command
    else {
        return false;
    };
    let source = workspace
        .upgrade()
        .ok_or_else(|| ApiError::new("target_not_found", "the workspace was closed"))
        .and_then(|workspace| workspace.read(cx).reader_source(*window_id, tab_id, cx));
    let (dispatch, window_id, tab_id, offset, limit, revision) =
        (dispatch.clone(), *window_id, tab_id.clone(), *offset, *limit, revision.clone());
    cx.background_executor()
        .spawn(async move {
            let result = source
                .and_then(|source| read_chunk(source, offset, limit, revision.as_deref()))
                .map(|mut result| {
                    result["window_id"] = json!(window_id);
                    result["tab_id"] = json!(tab_id);
                    result
                });
            dispatch.respond(result);
        })
        .detach();
    true
}

fn read_chunk(
    source: ReaderSource,
    offset: usize,
    limit: usize,
    expected: Option<&str>,
) -> Result<Value, ApiError> {
    let (bytes, total, revision, kind) = match source {
        ReaderSource::Text(text, revision) => {
            let revision = format!("text:{revision}");
            check_revision(expected, &revision)?;
            let total = text.len();
            check_size(total, offset)?;
            let end = offset.saturating_add(limit).min(total);
            // 线协议按字节计数；中文和 emoji 跨块时，结束点退回完整 UTF-8 字符边界。
            let part = (0..4)
                .filter_map(|back| end.checked_sub(back))
                .filter(|end| *end >= offset)
                .find_map(|end| text.try_slice(offset..end).ok())
                .ok_or_else(|| ApiError::invalid_params("offset is not a UTF-8 boundary"))?;
            (part.to_string().into_bytes(), total, revision, "text")
        },
        ReaderSource::Image(path) => {
            let mut file = File::open(path).map_err(file_error)?;
            let before = file.metadata().map_err(file_error)?;
            if !before.is_file() {
                return Err(ApiError::new("unsupported_file", "expected a regular image file"));
            }
            let total = usize::try_from(before.len())
                .map_err(|_| ApiError::new("file_too_large", "image exceeds the reader budget"))?;
            check_size(total, offset)?;
            let revision = image_revision(&before)?;
            check_revision(expected, &revision)?;
            file.seek(SeekFrom::Start(offset as u64)).map_err(file_error)?;
            let mut bytes = vec![0; limit.min(total - offset)];
            file.read_exact(&mut bytes).map_err(file_error)?;
            check_revision(
                Some(&revision),
                &image_revision(&file.metadata().map_err(file_error)?)?,
            )?;
            (bytes, total, revision, "image")
        },
    };
    let next_offset = offset + bytes.len();
    Ok(json!({"kind": kind, "revision": revision, "offset": offset, "next_offset": next_offset,
        "total_bytes": total, "eof": next_offset == total,
        "data": base64::engine::general_purpose::STANDARD.encode(bytes)}))
}

fn check_revision(expected: Option<&str>, actual: &str) -> Result<(), ApiError> {
    if expected.is_some_and(|revision| revision != actual) {
        Err(ApiError::new("file_changed", "the document changed while reading; refresh it"))
    } else {
        Ok(())
    }
}

fn check_size(total: usize, offset: usize) -> Result<(), ApiError> {
    if total > tabs::MAX_FILE_BYTES {
        Err(ApiError::new("file_too_large", "file exceeds the 16 MiB reader budget"))
    } else if offset > total {
        Err(ApiError::invalid_params("offset exceeds the file length"))
    } else {
        Ok(())
    }
}

fn image_revision(metadata: &std::fs::Metadata) -> Result<String, ApiError> {
    let modified = metadata
        .modified()
        .map_err(file_error)?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| ApiError::new("file_read_failed", error.to_string()))?;
    Ok(format!("image:{}:{}", metadata.len(), modified.as_nanos()))
}

fn file_error(error: std::io::Error) -> ApiError {
    ApiError::new("file_read_failed", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_chunks_preserve_utf8_and_require_the_same_revision() {
        let original = "a中文😀z";
        let mut bytes = Vec::new();
        loop {
            let part = read_chunk(
                ReaderSource::Text(Rope::from(original), 9),
                bytes.len(),
                4,
                Some("text:9"),
            )
            .unwrap();
            bytes.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(part["data"].as_str().unwrap())
                    .unwrap(),
            );
            if part["eof"] == true {
                break;
            }
        }
        assert_eq!(String::from_utf8(bytes).unwrap(), original);
        let error = read_chunk(ReaderSource::Text(Rope::from(original), 10), 4, 4, Some("text:9"))
            .unwrap_err();
        assert_eq!(error.code, "file_changed");
        assert!(read_chunk(ReaderSource::Text(Rope::from(original), 9), 2, 4, None).is_err());
    }

    #[test]
    fn image_reads_detect_replacement_and_enforce_the_budget() {
        use std::io::Write as _;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"abcdefgh").unwrap();
        let part = read_chunk(ReaderSource::Image(file.path().into()), 0, 4, None).unwrap();
        let next =
            read_chunk(ReaderSource::Image(file.path().into()), 4, 4, part["revision"].as_str())
                .unwrap();
        assert_eq!(next["eof"], true);
        file.write_all(b"changed").unwrap();
        let error =
            read_chunk(ReaderSource::Image(file.path().into()), 4, 4, part["revision"].as_str())
                .unwrap_err();
        assert_eq!(error.code, "file_changed");
        assert_eq!(check_size(tabs::MAX_FILE_BYTES + 1, 0).unwrap_err().code, "file_too_large");
    }
}
