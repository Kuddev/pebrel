//! 上传、导出与分项恢复：后台执行，成功后再关闭抽屉和刷新页面。
use super::*;

impl SettingsPane {
    pub(super) fn open_backup_sheet(
        &mut self,
        sheet: BackupSheet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.backup_busy {
            return;
        }
        self.backup_status = None;
        self.backup_ui.previous_focus = window.focused(cx);
        self.backup_ui.sheet = Some(sheet);
        self.backup_selection = self.backup_remote.selection;
        self.backup_ui.opened = None;
        self.backup_ui.restore_pass = None;
        self.clear_backup_inputs(window, cx);
        if sheet == BackupSheet::Storage {
            self.backup_ui.draft = self.backup_remote.clone();
            self.backup_ui.tested = false;
            self.fill_backup_fields(window, cx);
        }
        if matches!(sheet, BackupSheet::Backup | BackupSheet::Export) {
            if self.backup_remote.protocol == BackupProtocol::Off {
                self.backup_selection = recommended();
            }
            self.load_backup_summary(cx);
        }
        self.backup_ui.sheet_focus.as_ref().unwrap().focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_backup_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        self.backup_ui.sheet = None;
        self.backup_ui.opened = None;
        self.backup_ui.restore_pass = None;
        self.backup_ui.source = None;
        if let Some(focus) = self.backup_ui.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_backup_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        self.close_backup_sheet(window, cx);
        self.clear_backup_inputs(window, cx);
        self.backup_selection = self.backup_remote.selection;
        self.backup_status = None;
    }

    pub(super) fn perform_backup(
        &mut self,
        export: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.backup_busy || self.backup_selection.is_empty() {
            return;
        }
        let Some(pass) = self.backup_passphrase(cx) else {
            return;
        };
        let selection = self.backup_selection;
        let existing =
            (!export).then(|| self.backup_ui.snapshots.first().map(|s| s.name.clone())).flatten();
        let mut config = self.backup_remote.clone();
        config.selection = selection;
        // 文件选择取消时不写配置、不加密，也不改变上一次选择。
        let path = export.then(|| {
            cx.prompt_for_new_path(&crate::display::nebula_data_dir(), Some("pebrel.pebrel-backup"))
        });
        self.backup_task(
            window,
            cx,
            async move {
                let output = match path {
                    Some(picker) => {
                        match picker.await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())? {
                            Some(path) => Some(path),
                            None => return Ok(None),
                        }
                    },
                    None => None,
                };
                if let Some(name) = existing {
                    let (_, previous) = remote::pull_from(&config, Some(&name))?;
                    archive::open(&previous, &pass)?;
                }
                let archive = archive::collect(selection)?;
                let packet = archive::seal(&archive, &pass)?;
                let (snapshot, completion) = if let Some(path) = output {
                    crate::atomic_file::write(&path, &packet).map_err(|error| error.to_string())?;
                    (None, BackupCompletion::Exported(path))
                } else {
                    let (snapshot, message) = remote::push_snapshot(&config, &packet)?;
                    (Some(snapshot), BackupCompletion::Pushed(message))
                };
                // 选择在一次真实备份成功后保存；取消抽屉不会改变后续默认范围。
                let saved = config.save();
                Ok(Some((config, pass, archive.manifest, snapshot, completion, saved)))
            },
            |this, result, window, cx| {
                let Some((config, pass, manifest, snapshot, completion, saved)) = result else {
                    return;
                };
                this.backup_remote = config;
                cx.set_global(BackupPassword(Some(pass)));
                if let Some(snapshot) = snapshot {
                    this.backup_ui.known.insert(
                        snapshot.name.clone(),
                        (manifest.categories.len(), manifest.device),
                    );
                    this.backup_ui.snapshots.insert(0, snapshot);
                    this.refresh_backup_snapshots(cx);
                }
                this.close_backup_sheet(window, cx);
                this.clear_backup_inputs(window, cx);
                this.backup_status = Some(match saved {
                    Ok(()) => BackupStatus::Completed(completion),
                    Err(error) => BackupStatus::Error(
                        crate::gpui_shell::config::ui_language(cx)
                            .format(Message::BackupFlowSelectionSaveFailed, &[("error", &error)]),
                    ),
                });
            },
        );
    }

    pub(super) fn pick_backup_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        self.backup_task(
            window,
            cx,
            async move { picker.await.map_err(|e| e.to_string())?.map_err(|e| e.to_string()) },
            |this, paths, window, cx| {
                if let Some(path) = paths.and_then(|paths| paths.into_iter().next()) {
                    this.backup_remote_inputs[0].update(cx, |input, cx| {
                        input.set_value(path.to_string_lossy().into_owned(), window, cx)
                    });
                    this.read_backup_fields(cx);
                    this.backup_ui.tested = false;
                }
            },
        );
    }

    pub(super) fn restore_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        self.backup_task(
            window,
            cx,
            async move { picker.await.map_err(|e| e.to_string())?.map_err(|e| e.to_string()) },
            |this, paths, window, cx| {
                if let Some(path) = paths.and_then(|paths| paths.into_iter().next()) {
                    this.open_backup_restore(RestoreSource::File(path), window, cx);
                }
            },
        );
    }

    pub(super) fn open_backup_restore(
        &mut self,
        source: RestoreSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.backup_busy {
            return;
        }
        self.open_backup_sheet(BackupSheet::Restore, window, cx);
        self.backup_ui.source = Some(source);
        if cx.try_global::<BackupPassword>().is_some_and(|p| p.0.is_some()) {
            self.unlock_backup(window, cx);
        } else {
            self.backup_pass_input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    pub(super) fn unlock_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        let Some(source) = self.backup_ui.source.clone() else {
            return;
        };
        let Some(pass) = self.backup_passphrase(cx) else {
            return;
        };
        let config = self.backup_remote.clone();
        self.backup_task(
            window,
            cx,
            async move {
                let packet = match source {
                    RestoreSource::File(path) => std::fs::read(path).map_err(|e| e.to_string())?,
                    RestoreSource::Remote(name) => remote::pull_from(&config, Some(&name))?.1,
                };
                archive::open(&packet, &pass).map(|archive| (archive, pass))
            },
            |this, (archive, pass), window, cx| {
                this.backup_selection =
                    BackupSelection::from_categories(archive.manifest.categories.iter().copied());
                if let Some(RestoreSource::Remote(name)) = &this.backup_ui.source {
                    this.backup_ui.known.insert(
                        name.clone(),
                        (archive.manifest.categories.len(), archive.manifest.device.clone()),
                    );
                }
                cx.set_global(BackupPassword(Some(pass.clone())));
                this.backup_ui.opened = Some(archive);
                this.backup_ui.restore_pass = Some(pass);
                this.clear_backup_inputs(window, cx);
            },
        );
    }

    pub(super) fn restore_selected_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy || self.backup_selection.is_empty() {
            return;
        }
        let Some(archive) = self.backup_ui.opened.clone() else {
            return;
        };
        let Some(pass) = self.backup_ui.restore_pass.clone() else {
            return;
        };
        let selection = self.backup_selection;
        self.backup_task(
            window,
            cx,
            async move {
                archive::recovery::restore_selected(&archive, selection, &pass)
                    .map(|point| (point, pass))
            },
            |this, point, window, cx| {
                this.backup_ui.undo = Some(point);
                this.reload_after_restore(cx);
                this.close_backup_sheet(window, cx);
                this.backup_status = Some(BackupStatus::Completed(BackupCompletion::Restored));
            },
        );
    }

    pub(super) fn undo_backup_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backup_busy {
            return;
        }
        let Some((point, pass)) = self.backup_ui.undo.clone() else {
            return;
        };
        self.backup_task(
            window,
            cx,
            async move { archive::recovery::undo(&point, &pass) },
            |this, _, _, cx| {
                this.backup_ui.undo = None;
                this.reload_after_restore(cx);
                this.backup_status = Some(BackupStatus::Notice(Message::BackupFlowUndone));
            },
        );
    }
}
