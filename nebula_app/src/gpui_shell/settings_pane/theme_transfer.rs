//! Background theme import/export orchestration for the native settings pane.
//!
//! This module deliberately stops at the library boundary.  It owns picker
//! lifetimes, bounded file reads, preview state, and atomic export writes; the
//! settings UI decides how a candidate is displayed and how an imported
//! document becomes an editor draft.  No operation in this module publishes a
//! runtime preference.

use super::*;
use crate::theme_library::{
    ExportArtifact, ImportCandidate, ImportDiagnostic, Inspection, MAX_IMPORT_CANDIDATES,
    MAX_INPUT_BYTES, ThemeDocument, ThemeFormat, export, inspect,
};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The transfer surface has one modal with two modes. Keeping the mode in
/// state means a late picker result cannot accidentally be rendered as the
/// other operation after the user reopens the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThemeTransferMode {
    Import,
    Export,
}

/// UI-facing lifecycle phase. The UI can localize the labels while this
/// module keeps transitions independent from the current language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThemeTransferStatus {
    Idle,
    Picking,
    Inspecting,
    Ready,
    Importing,
    Exporting,
    Writing,
    Completed,
    Error,
}

/// State retained by SettingsPane while a transfer modal is open.
///
/// The vectors and artifacts are snapshots. They are safe to render without
/// touching the filesystem, and the selected import document is only moved to
/// the editor by the caller after confirm_theme_import completes.
pub(super) struct ThemeTransferState {
    pub(super) mode: Option<ThemeTransferMode>,
    pub(super) sequence: u64,
    pub(super) busy: bool,
    pub(super) status: ThemeTransferStatus,
    pub(super) error: Option<String>,
    pub(super) notice: Option<String>,

    pub(super) import_candidates: Vec<ImportCandidate>,
    pub(super) import_diagnostics: Vec<ImportDiagnostic>,
    pub(super) selected_import: usize,
    /// The confirmed source document handed to the editor draft. Persistence
    /// and custom ID/revision assignment happen only when the editor saves.
    pub(super) imported_document: Option<ThemeDocument>,

    pub(super) export_document: Option<ThemeDocument>,
    pub(super) export_format: ThemeFormat,
    pub(super) export_artifact: Option<ExportArtifact>,
    pub(super) export_path: Option<PathBuf>,
}

impl Default for ThemeTransferState {
    fn default() -> Self {
        Self {
            mode: None,
            sequence: 0,
            busy: false,
            status: ThemeTransferStatus::Idle,
            error: None,
            notice: None,
            import_candidates: Vec::new(),
            import_diagnostics: Vec::new(),
            selected_import: 0,
            imported_document: None,
            export_document: None,
            export_format: ThemeFormat::Pebrel,
            export_artifact: None,
            export_path: None,
        }
    }
}

impl ThemeTransferState {
    fn advance(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    fn open(&mut self, mode: ThemeTransferMode) -> u64 {
        let sequence = self.advance();
        self.mode = Some(mode);
        self.busy = true;
        self.status = ThemeTransferStatus::Picking;
        self.error = None;
        self.notice = None;
        self.import_candidates.clear();
        self.import_diagnostics.clear();
        self.selected_import = 0;
        self.imported_document = None;
        self.export_document = None;
        self.export_artifact = None;
        self.export_path = None;
        sequence
    }

    fn begin_task(&mut self, status: ThemeTransferStatus) -> u64 {
        let sequence = self.advance();
        self.busy = true;
        self.status = status;
        self.error = None;
        self.notice = None;
        self.imported_document = None;
        self.export_path = None;
        sequence
    }

    fn invalidate(&mut self) {
        // Increment before clearing the mode. Any callback that already owns
        // the old token will then fail its equality check even if it runs in
        // the same event turn as the close action.
        let sequence = self.advance();
        let format = self.export_format;
        *self = Self { sequence, export_format: format, ..Self::default() };
    }

    fn is_current(&self, sequence: u64) -> bool {
        self.sequence == sequence
    }
}

impl SettingsPane {
    /// Start a multi-file import. The picker is asynchronous and all file
    /// reads and format inspection run on the background executor.
    pub(super) fn open_theme_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_editor.as_ref().is_some_and(|editor| editor.save_busy) {
            return;
        }
        let sequence = self.theme_transfer.open(ThemeTransferMode::Import);
        let language = crate::gpui_shell::config::ui_language(cx);
        let picked = crate::platform::file_picker::files(
            window,
            cx,
            language.text(crate::i18n::Message::ThemeTransferImportTitle),
        );
        let executor = cx.background_executor().clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let paths = picked.await;
            if paths.is_empty() {
                let _ = this.update(cx, |this, cx| {
                    this.cancel_theme_transfer_if_current(sequence, cx);
                });
                return;
            }
            let transitioned = this
                .update(cx, |this, cx| {
                    if !this.theme_transfer.is_current(sequence) {
                        return false;
                    }
                    this.theme_transfer.status = ThemeTransferStatus::Inspecting;
                    cx.notify();
                    true
                })
                .ok()
                .unwrap_or(false);
            if !transitioned {
                return;
            }

            let inspection = executor.spawn(async move { inspect_paths(paths) }).await;
            let _ = this.update(cx, |this, cx| {
                this.finish_theme_import_inspection(sequence, inspection, cx);
            });
        })
        .detach();
    }

    /// Select a candidate in the import preview without touching the store.
    pub(super) fn select_theme_import(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.theme_transfer.mode != Some(ThemeTransferMode::Import)
            || self.theme_transfer.busy
            || index >= self.theme_transfer.import_candidates.len()
        {
            return;
        }
        self.theme_transfer.selected_import = index;
        self.theme_transfer.error = None;
        cx.notify();
    }

    pub(super) fn selected_theme_import(&self) -> Option<&ImportCandidate> {
        self.theme_transfer.import_candidates.get(self.theme_transfer.selected_import)
    }

    /// Confirm the currently selected preview and hand its document to the
    /// editor. The optional name is applied to the draft by the UI; no library
    /// file is written here. The editor's Save action owns persistence and
    /// optimistic conflict handling.
    pub(super) fn confirm_theme_import(
        &mut self,
        requested_name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.theme_transfer.mode != Some(ThemeTransferMode::Import) || self.theme_transfer.busy {
            return;
        }
        let Some(candidate) =
            self.theme_transfer.import_candidates.get(self.theme_transfer.selected_import).cloned()
        else {
            self.theme_transfer.status = ThemeTransferStatus::Error;
            self.theme_transfer.error = Some("no importable theme is selected".to_owned());
            cx.notify();
            return;
        };

        let mut document = candidate.document;
        if let Some(name) = requested_name.filter(|name| !name.trim().is_empty()) {
            match document.with_name(name.trim().to_owned()) {
                Ok(named) => document = named,
                Err(error) => {
                    self.theme_transfer.status = ThemeTransferStatus::Error;
                    self.theme_transfer.error = Some(error.to_string());
                    self.theme_transfer.notice = None;
                    cx.notify();
                    return;
                },
            }
        }
        self.theme_transfer.imported_document = Some(document.clone());
        self.theme_transfer.busy = false;
        self.theme_transfer.status = ThemeTransferStatus::Completed;
        self.theme_transfer.notice = Some(format!("theme draft ready: {}", document.name()));
        self.theme_transfer.error = None;
        cx.notify();
    }

    /// Move the selected document to the UI layer. It has not been written to
    /// the library yet; the editor owns the later Save operation.
    pub(super) fn take_imported_theme(&mut self) -> Option<ThemeDocument> {
        self.theme_transfer.imported_document.take()
    }

    /// Open an export preview for the editor's current draft. This operation
    /// serializes the draft only; it does not save or activate it.
    pub(super) fn open_theme_export(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_editor.as_ref().is_some_and(|editor| editor.save_busy) {
            return;
        }
        let document = self.document_for_draft().ok();
        let Some(document) = document else {
            self.open_theme_export_error(
                "the current theme draft could not be encoded".to_owned(),
                cx,
            );
            return;
        };
        let format = self.theme_transfer.export_format;
        self.open_theme_export_document(document, format, cx);
    }

    /// Variant for callers that already own a validated document (for example,
    /// an imported theme or a picker preview).
    pub(super) fn open_theme_export_document(
        &mut self,
        document: ThemeDocument,
        format: ThemeFormat,
        cx: &mut Context<Self>,
    ) {
        let sequence = self.theme_transfer.open(ThemeTransferMode::Export);
        self.theme_transfer.export_document = Some(document.clone());
        self.theme_transfer.export_format = format;
        self.theme_transfer.status = ThemeTransferStatus::Exporting;
        cx.notify();
        self.start_theme_export_prepare(sequence, document, format, cx);
    }

    /// Change the output adapter while keeping the current document. The
    /// previous preparation task becomes stale immediately, so a slow format
    /// conversion cannot overwrite the newly selected format.
    pub(super) fn select_theme_export_format(
        &mut self,
        format: ThemeFormat,
        cx: &mut Context<Self>,
    ) {
        if self.theme_transfer.mode != Some(ThemeTransferMode::Export)
            || self.theme_transfer.busy
            || self.theme_transfer.export_format == format
        {
            return;
        }
        let Some(document) = self.theme_transfer.export_document.clone() else {
            self.open_theme_export_error("no theme is ready to export".to_owned(), cx);
            return;
        };
        let sequence = self.theme_transfer.begin_task(ThemeTransferStatus::Exporting);
        self.theme_transfer.export_format = format;
        self.theme_transfer.export_artifact = None;
        cx.notify();
        self.start_theme_export_prepare(sequence, document, format, cx);
    }

    fn start_theme_export_prepare(
        &mut self,
        sequence: u64,
        document: ThemeDocument,
        format: ThemeFormat,
        cx: &mut Context<Self>,
    ) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = executor
                .spawn(async move { export(&document, format).map_err(|error| error.to_string()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.finish_theme_export_prepare(sequence, result, cx);
            });
        })
        .detach();
    }

    /// Confirm the prepared artifact and choose its destination. The write
    /// itself is bounded to the already prepared string and uses the same
    /// atomic writer as the rest of the application state.
    pub(super) fn confirm_theme_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_transfer.mode != Some(ThemeTransferMode::Export) || self.theme_transfer.busy {
            return;
        }
        let Some(artifact) = self.theme_transfer.export_artifact.clone() else {
            self.theme_transfer.status = ThemeTransferStatus::Error;
            self.theme_transfer.error = Some("export preview is not ready".to_owned());
            cx.notify();
            return;
        };
        let name = self
            .theme_transfer
            .export_document
            .as_ref()
            .map(|document| document.name())
            .unwrap_or("theme");
        let suggested = suggested_export_filename(name, artifact.extension);
        let directory = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        let picked = cx.prompt_for_new_path(&directory, Some(&suggested));
        // The destination picker is still cancellable. Lock the modal only
        // after a concrete path has been returned and the atomic write is
        // about to begin.
        let sequence = self.theme_transfer.begin_task(ThemeTransferStatus::Picking);
        let executor = cx.background_executor().clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => {
                    let _ = this.update(cx, |this, cx| {
                        this.cancel_theme_transfer_if_current(sequence, cx);
                    });
                    return;
                },
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.fail_theme_transfer_if_current(sequence, error.to_string(), cx);
                    });
                    return;
                },
            };
            let transitioned = this
                .update(cx, |this, cx| {
                    if !this.theme_transfer.is_current(sequence) {
                        return false;
                    }
                    this.theme_transfer.status = ThemeTransferStatus::Writing;
                    cx.notify();
                    true
                })
                .ok()
                .unwrap_or(false);
            if !transitioned {
                return;
            }
            let write_path = path.clone();
            let write_result = executor
                .spawn(async move {
                    crate::atomic_file::write(&write_path, artifact.text.as_bytes())
                        .map_err(|error| error.to_string())
                })
                .await
                .map(|_| path);
            let _ = this.update(cx, |this, cx| {
                this.finish_theme_export_write(sequence, write_result, cx);
            });
        })
        .detach();
    }

    /// Close or cancel the transfer modal. Incrementing the sequence is
    /// essential: picker and background futures are intentionally detached and
    /// may complete after this call.
    pub(super) fn cancel_theme_transfer(&mut self, cx: &mut Context<Self>) {
        self.theme_transfer.invalidate();
        cx.notify();
    }

    pub(super) fn theme_transfer_is_open(&self) -> bool {
        self.theme_transfer.mode.is_some()
    }

    fn finish_theme_import_inspection(
        &mut self,
        sequence: u64,
        inspection: Inspection,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_transfer.is_current(sequence) {
            return;
        }
        self.theme_transfer.busy = false;
        self.theme_transfer.import_candidates = inspection.candidates;
        self.theme_transfer.import_diagnostics = inspection.diagnostics;
        self.theme_transfer.selected_import = 0;
        if self.theme_transfer.import_candidates.is_empty() {
            self.theme_transfer.status = ThemeTransferStatus::Error;
            self.theme_transfer.error = Some("no importable themes were found".to_owned());
        } else {
            self.theme_transfer.status = ThemeTransferStatus::Ready;
            self.theme_transfer.error = None;
            self.theme_transfer.notice = Some(format!(
                "{} theme{} ready to import",
                self.theme_transfer.import_candidates.len(),
                if self.theme_transfer.import_candidates.len() == 1 { " is" } else { "s are" },
            ));
        }
        cx.notify();
    }

    fn finish_theme_export_prepare(
        &mut self,
        sequence: u64,
        result: Result<ExportArtifact, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_transfer.is_current(sequence) {
            return;
        }
        self.theme_transfer.busy = false;
        match result {
            Ok(artifact) => {
                self.theme_transfer.export_artifact = Some(artifact);
                self.theme_transfer.status = ThemeTransferStatus::Ready;
                self.theme_transfer.error = None;
                self.theme_transfer.notice = None;
            },
            Err(error) => {
                self.theme_transfer.export_artifact = None;
                self.theme_transfer.status = ThemeTransferStatus::Error;
                self.theme_transfer.error = Some(error);
                self.theme_transfer.notice = None;
            },
        }
        cx.notify();
    }

    fn finish_theme_export_write(
        &mut self,
        sequence: u64,
        result: Result<PathBuf, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_transfer.is_current(sequence) {
            return;
        }
        self.theme_transfer.busy = false;
        match result {
            Ok(path) => {
                self.theme_transfer.export_path = Some(path.clone());
                self.theme_transfer.status = ThemeTransferStatus::Completed;
                self.theme_transfer.notice = Some(format!("exported theme to {}", path.display()));
                self.theme_transfer.error = None;
            },
            Err(error) => {
                self.theme_transfer.status = ThemeTransferStatus::Error;
                self.theme_transfer.error = Some(error);
                self.theme_transfer.notice = None;
            },
        }
        cx.notify();
    }

    fn cancel_theme_transfer_if_current(&mut self, sequence: u64, cx: &mut Context<Self>) {
        if self.theme_transfer.is_current(sequence) {
            self.theme_transfer.invalidate();
            cx.notify();
        }
    }

    fn fail_theme_transfer_if_current(
        &mut self,
        sequence: u64,
        error: String,
        cx: &mut Context<Self>,
    ) {
        if !self.theme_transfer.is_current(sequence) {
            return;
        }
        self.theme_transfer.busy = false;
        self.theme_transfer.status = ThemeTransferStatus::Error;
        self.theme_transfer.error = Some(error);
        self.theme_transfer.notice = None;
        cx.notify();
    }

    fn open_theme_export_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.theme_transfer.open(ThemeTransferMode::Export);
        self.theme_transfer.busy = false;
        self.theme_transfer.status = ThemeTransferStatus::Error;
        self.theme_transfer.error = Some(error);
        cx.notify();
    }
}

/// Read one selected file with both a metadata guard and a bounded stream
/// read. The second check protects against a file growing after the picker
/// returned and keeps memory usage independent of an untrusted file size.
fn read_theme_file(path: &Path) -> Result<String, String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{}: symbolic links are not accepted", path.display()));
    }
    if !metadata.is_file() {
        return Err(format!("{}: path is not a regular file", path.display()));
    }
    if metadata.len() > MAX_INPUT_BYTES as u64 {
        return Err(format!(
            "{}: input exceeds the 256 KiB limit ({} bytes)",
            path.display(),
            metadata.len()
        ));
    }

    let mut file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "{}: input exceeds the 256 KiB limit (more than {} bytes)",
            path.display(),
            MAX_INPUT_BYTES
        ));
    }
    String::from_utf8(bytes)
        .map_err(|error| format!("{}: input is not UTF-8 ({error})", path.display()))
}

fn inspect_paths(paths: Vec<PathBuf>) -> Inspection {
    let mut inspection = Inspection::default();
    for path in paths {
        let filename = path.display().to_string();
        // Format detection only needs the basename. Passing the full path
        // could select the wrong theme format based on a parent directory's
        // name instead of the file itself.
        let detected_filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(filename.as_str())
            .to_owned();
        match read_theme_file(&path) {
            Ok(text) => {
                let result = inspect(&text, detected_filename);
                let mut candidates = result.candidates;
                let available = MAX_IMPORT_CANDIDATES.saturating_sub(inspection.candidates.len());
                let take = available.min(candidates.len());
                let omitted = candidates.len().saturating_sub(take);
                inspection.candidates.extend(candidates.drain(..take).map(|mut candidate| {
                    candidate.filename = filename.clone();
                    candidate
                }));
                inspection.diagnostics.extend(result.diagnostics.into_iter().map(
                    |mut diagnostic| {
                        diagnostic.filename = filename.clone();
                        diagnostic
                    },
                ));
                if omitted > 0 {
                    inspection.diagnostics.push(ImportDiagnostic {
                        filename,
                        format: None,
                        message: format!(
                            "import candidate limit reached; {omitted} additional theme{} were skipped (limit {MAX_IMPORT_CANDIDATES})",
                            if omitted == 1 { "" } else { "s" }
                        ),
                    });
                }
            },
            Err(message) => {
                inspection.diagnostics.push(ImportDiagnostic { filename, format: None, message })
            },
        }
    }
    inspection
}

fn suggested_export_filename(name: &str, extension: &str) -> String {
    let mut base = String::new();
    for character in name.chars() {
        if character.is_control()
            || matches!(character, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        {
            base.push('-');
        } else {
            base.push(character);
        }
    }
    let base = base.trim().trim_matches('.');
    let base = if base.is_empty() { "theme" } else { base };
    format!("{base}.{extension}")
}
