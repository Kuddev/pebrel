//! Editor-owned ZIP transfer snapshots and asynchronous file transactions.
use super::*;
use crate::theme_library::{
    ThemeDocument, ThemeLibraryStore,
    package::{Author, CheckedPackage, Manifest, export_package},
};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Export,
    Import,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    Picking,
    Inspecting,
    Committing,
    Done,
}
impl Phase {
    pub(super) fn busy(self) -> bool {
        matches!(self, Self::Picking | Self::Inspecting | Self::Committing)
    }
}
pub(super) struct Inspected {
    pub(super) path: PathBuf,
    pub(super) manifest: Manifest,
    pub(super) document: ThemeDocument,
    pub(super) definition: nebula_settings::ThemeDefinition,
}
pub(super) struct PackageTransfer {
    pub(super) mode: Mode,
    pub(super) phase: Phase,
    pub(super) error: Option<String>,
    pub(super) document: Option<ThemeDocument>,
    pub(super) author: Entity<InputState>,
    pub(super) version: Entity<InputState>,
    pub(super) license: Entity<InputState>,
    pub(super) github: Entity<InputState>,
    pub(super) preview: Option<PathBuf>,
    pub(super) inspected: Option<Inspected>,
    pub(super) installed: Option<ThemeDocument>,
    pub(super) output: Option<PathBuf>,
    pub(super) focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsPane {
    pub(super) fn open_theme_package(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_package.is_some()
            || self.theme_transfer_is_open()
            || self.theme_editor.as_ref().is_none_or(|editor| editor.save_busy)
        {
            return;
        }
        self.theme_package_seq = self.theme_package_seq.wrapping_add(1);
        let document = self.document_for_draft();
        let metadata = document.as_ref().ok().map(ThemeDocument::to_value).unwrap_or_default();
        let receipt = &metadata["metadata"]["package"];
        let mut input =
            |value: &str| cx.new(|cx| InputState::new(window, cx).default_value(value.to_owned()));
        let author = input(receipt["author"]["name"].as_str().unwrap_or(""));
        let version = input(receipt["version"].as_str().unwrap_or("1.0.0"));
        let license = input(receipt["license"].as_str().unwrap_or(""));
        let github = input(receipt["author"]["github"].as_str().unwrap_or(""));
        let subscriptions = [&author, &version, &license, &github]
            .into_iter()
            .map(|entity| cx.observe(entity, |_, _, cx| cx.notify()))
            .collect();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.theme_package = Some(PackageTransfer {
            mode: Mode::Export,
            phase: Phase::Idle,
            error: document.as_ref().err().cloned(),
            document: document.ok(),
            author,
            version,
            license,
            github,
            preview: None,
            inspected: None,
            installed: None,
            output: None,
            focus,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    pub(super) fn close_theme_package(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_package.as_ref().is_some_and(|state| state.phase == Phase::Committing) {
            return;
        }
        self.theme_package_seq = self.theme_package_seq.wrapping_add(1);
        self.theme_package = None;
        if let Some(editor) = &self.theme_editor {
            editor.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn switch_theme_package(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let Some(state) = &mut self.theme_package else { return };
        if state.phase.busy() {
            return;
        }
        self.theme_package_seq = self.theme_package_seq.wrapping_add(1);
        state.mode = mode;
        state.phase = Phase::Idle;
        state.error = if mode == Mode::Export && state.document.is_none() {
            Some(
                crate::gpui_shell::config::ui_language(cx)
                    .text(crate::i18n::Message::ThemeEditorInvalidValue)
                    .to_owned(),
            )
        } else {
            None
        };
        state.installed = None;
        state.output = None;
        cx.notify();
    }

    fn begin_package_task(&mut self, phase: Phase, cx: &mut Context<Self>) -> Option<u64> {
        let state = self.theme_package.as_mut()?;
        if state.phase.busy() {
            return None;
        }
        self.theme_package_seq = self.theme_package_seq.wrapping_add(1);
        state.phase = phase;
        state.error = None;
        cx.notify();
        Some(self.theme_package_seq)
    }

    fn package_current(&self, sequence: u64) -> bool {
        self.theme_package.is_some() && self.theme_package_seq == sequence
    }

    fn package_error(&mut self, sequence: u64, error: String, cx: &mut Context<Self>) {
        if !self.package_current(sequence) {
            return;
        }
        let state = self.theme_package.as_mut().unwrap();
        state.phase = Phase::Idle;
        state.error = Some(error);
        cx.notify();
    }

    pub(super) fn choose_package_file(
        &mut self,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sequence) = self.begin_package_task(Phase::Picking, cx) else { return };
        if !preview {
            self.theme_package.as_mut().unwrap().inspected = None;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                language
                    .text(if preview {
                        crate::i18n::Message::ThemePackageChoosePreview
                    } else {
                        crate::i18n::Message::ThemePackageChooseFile
                    })
                    .into(),
            ),
        });
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = match picked.await {
                Ok(result) => result
                    .map(|paths| paths.and_then(|paths| paths.into_iter().next()))
                    .map_err(|error| error.to_string()),
                Err(_) => Ok(None),
            };
            let path = match result {
                Ok(Some(path)) => path,
                Ok(None) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.package_current(sequence) {
                            this.theme_package.as_mut().unwrap().phase = Phase::Idle;
                            cx.notify();
                        }
                    });
                    return;
                },
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.package_error(sequence, error, cx));
                    return;
                },
            };
            let proceed = this
                .update(cx, |this, cx| {
                    if !this.package_current(sequence) {
                        return false;
                    }
                    let state = this.theme_package.as_mut().unwrap();
                    if preview {
                        state.preview = Some(path.clone());
                        state.phase = Phase::Idle;
                    } else {
                        state.phase = Phase::Inspecting;
                    }
                    cx.notify();
                    !preview
                })
                .unwrap_or(false);
            if !proceed {
                return;
            }
            let result = executor
                .spawn(async move {
                    let package = CheckedPackage::open(&path).map_err(|error| error.to_string())?;
                    let definition =
                        package.document.definition().map_err(|error| error.to_string())?;
                    Ok(Inspected {
                        path,
                        manifest: package.manifest,
                        document: package.document,
                        definition,
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.package_current(sequence) {
                    return;
                }
                match result {
                    Ok(inspected) => {
                        let state = this.theme_package.as_mut().unwrap();
                        state.inspected = Some(inspected);
                        state.phase = Phase::Idle;
                        cx.notify();
                    },
                    Err(error) => this.package_error(sequence, error, cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn confirm_package_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, expected_manifest, expected_document)) = self
            .theme_package
            .as_ref()
            .filter(|state| {
                state.mode == Mode::Import && !state.phase.busy() && state.phase != Phase::Done
            })
            .and_then(|state| state.inspected.as_ref())
            .map(|checked| {
                (checked.path.clone(), checked.manifest.clone(), checked.document.clone())
            })
        else {
            return;
        };
        let changed_message = crate::gpui_shell::config::ui_language(cx)
            .text(crate::i18n::Message::ThemePackageChanged)
            .to_owned();
        let Some(sequence) = self.begin_package_task(Phase::Committing, cx) else { return };
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            // Reopen and verify the selected file at commit time: the earlier
            // inspection is a display snapshot, not permission to trust new bytes.
            let result = executor
                .spawn(async move {
                    let package = CheckedPackage::open(&path).map_err(|error| error.to_string())?;
                    if package.manifest != expected_manifest
                        || package.document != expected_document
                    {
                        return Err(changed_message);
                    }
                    package
                        .install(&ThemeLibraryStore::default())
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.package_current(sequence) {
                    return;
                }
                match result {
                    Ok(document) => {
                        let state = this.theme_package.as_mut().unwrap();
                        state.installed = Some(document);
                        state.phase = Phase::Done;
                        cx.notify();
                    },
                    Err(error) => this.package_error(sequence, error, cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn confirm_package_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.theme_package.as_ref().filter(|state| {
            state.mode == Mode::Export && !state.phase.busy() && state.phase != Phase::Done
        }) else {
            return;
        };
        let Some(document) = state.document.clone() else { return };
        let author = Author {
            name: state.author.read(cx).value().trim().to_owned(),
            github: Some(state.github.read(cx).value().trim().to_owned()).filter(|s| !s.is_empty()),
        };
        let version = state.version.read(cx).value().trim().to_owned();
        let license = state.license.read(cx).value().trim().to_owned();
        if author.name.is_empty() || version.is_empty() || license.is_empty() {
            return;
        }
        let preview = state.preview.clone();
        let suggested = format!(
            "{}.pebrel-theme.zip",
            document
                .name()
                .chars()
                .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' })
                .collect::<String>()
        );
        let Some(sequence) = self.begin_package_task(Phase::Picking, cx) else { return };
        let directory = std::env::temp_dir();
        let picked = cx.prompt_for_new_path(&directory, Some(&suggested));
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(path))) => path,
                Ok(Err(error)) => {
                    let _ = this
                        .update(cx, |this, cx| this.package_error(sequence, error.to_string(), cx));
                    return;
                },
                _ => {
                    let _ = this.update(cx, |this, cx| {
                        if this.package_current(sequence) {
                            this.theme_package.as_mut().unwrap().phase = Phase::Idle;
                            cx.notify();
                        }
                    });
                    return;
                },
            };
            let proceed = this
                .update(cx, |this, cx| {
                    if !this.package_current(sequence) {
                        return false;
                    }
                    this.theme_package.as_mut().unwrap().phase = Phase::Committing;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !proceed {
                return;
            }
            let result = executor
                .spawn(async move {
                    export_package(&document, author, version, license, &path, preview.as_deref())
                        .map(|_| path)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.package_current(sequence) {
                    return;
                }
                match result {
                    Ok(path) => {
                        let state = this.theme_package.as_mut().unwrap();
                        state.output = Some(path);
                        state.phase = Phase::Done;
                        cx.notify();
                    },
                    Err(error) => this.package_error(sequence, error, cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn edit_installed_package(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.theme_package.as_ref().and_then(|state| state.installed.clone())
        else {
            return;
        };
        self.close_theme_package(window, cx);
        if let Err(error) = self.load_imported_theme_editor(document, window, cx) {
            if let Some(editor) = self.theme_editor.as_mut() {
                editor.error = Some(error);
            }
            cx.notify();
        }
    }
}
