//! 插件页的任务所有者：安装、启停、卸载与生成主题都在后台执行，
//! 规则与持久化归 `crate::plugins`；页面只保存列表快照、代次和反馈。

use super::*;
use crate::i18n::Message;
use crate::plugins::install::{Library, Listing, Request};
use crate::plugins::{Grant, themes};
use crate::theme_library::ThemeLibraryStore;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Busy {
    Picking,
    Installing,
    Plugin(String),
}

/// A picked package that requested grants and waits for the user's answer.
#[derive(Clone, Debug)]
pub(super) struct Pending {
    pub path: PathBuf,
    pub request: Request,
}

pub(super) struct PluginsState {
    pub listing: Option<Result<Listing, String>>,
    pub loading: bool,
    pub busy: Option<Busy>,
    pub pending: Option<Pending>,
    /// Result of the last operation; `Some(id)` attaches it to that row.
    pub feedback: Option<(Option<String>, Result<String, String>)>,
    sequence: u64,
    library_root: PathBuf,
    theme_root: PathBuf,
}

impl PluginsState {
    pub(super) fn new() -> Self {
        let settings = nebula_settings::settings_dir();
        Self {
            listing: None,
            loading: false,
            busy: None,
            pending: None,
            feedback: None,
            sequence: 0,
            library_root: settings.join("plugins"),
            theme_root: settings.join("themes"),
        }
    }

    #[cfg(all(test, feature = "gpui-test-support"))]
    pub(super) fn with_roots(library_root: PathBuf, theme_root: PathBuf) -> Self {
        Self { library_root, theme_root, ..Self::new() }
    }

    fn library(&self) -> Library {
        Library::new(self.library_root.clone())
    }

    fn store(&self) -> ThemeLibraryStore {
        ThemeLibraryStore::new(self.theme_root.clone())
    }

    fn idle(&self) -> bool {
        !self.loading && self.busy.is_none()
    }
}

impl SettingsPane {
    pub(super) fn refresh_plugins(&mut self, cx: &mut Context<Self>) {
        if !self.plugins.idle() {
            return;
        }
        self.plugins.loading = true;
        let library = self.plugins.library();
        self.run_plugin_task(None, cx, move || (Ok(None), library));
    }

    pub(super) fn pick_plugin_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.plugins.idle() {
            return;
        }
        self.plugins.busy = Some(Busy::Picking);
        self.plugins.pending = None;
        self.plugins.feedback = None;
        self.plugins.sequence = self.plugins.sequence.wrapping_add(1);
        let sequence = self.plugins.sequence;
        let language = crate::gpui_shell::config::ui_language(cx);
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(language.text(Message::SettingsPluginsChooseFolder).into()),
        });
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            // 对话框本身打不开（例如没有桌面门户）也要给出反馈，用户取消则不提示。
            let request = match picked.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => Some(
                        executor
                            .spawn(async move {
                                Library::inspect(&path)
                                    .map(|request| Pending { path, request })
                                    .map_err(|error| error.to_string())
                            })
                            .await,
                    ),
                    None => None,
                },
                Ok(Err(error)) => Some(Err(error.to_string())),
                Ok(Ok(None)) | Err(_) => None,
            };
            let _ = this.update(cx, |this, cx| {
                if this.plugins.sequence != sequence {
                    return;
                }
                this.plugins.busy = None;
                match request {
                    None => {},
                    Some(Ok(pending)) if pending.request.requested.is_empty() => {
                        this.install_plugin(pending, cx);
                    },
                    Some(Ok(pending)) => this.plugins.pending = Some(pending),
                    Some(Err(error)) => {
                        this.plugins.feedback = Some((
                            None,
                            Err(language
                                .format(Message::SettingsPluginsFailed, &[("error", &error)])),
                        ));
                    },
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Install the pending package with exactly the grants it showed the user.
    pub(super) fn confirm_plugin_install(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.plugins.pending.take() {
            self.install_plugin(pending, cx);
        }
    }

    pub(super) fn cancel_plugin_install(&mut self, cx: &mut Context<Self>) {
        self.plugins.pending = None;
        cx.notify();
    }

    fn install_plugin(&mut self, pending: Pending, cx: &mut Context<Self>) {
        if !self.plugins.idle() {
            return;
        }
        self.plugins.busy = Some(Busy::Installing);
        let language = crate::gpui_shell::config::ui_language(cx);
        let (library, store) = (self.plugins.library(), self.plugins.store());
        self.run_plugin_task(None, cx, move || {
            let result = (|| {
                let plugin = library.install(&pending.path, &pending.request.requested)?;
                let themes = if plugin.grants.contains(&Grant::ThemeLibrary) {
                    themes::install_bundled(&library, &plugin.id, &store)?
                } else {
                    Vec::new()
                };
                let added = themes
                    .iter()
                    .filter(|theme| !matches!(theme, themes::BundledTheme::Failed { .. }))
                    .count();
                let mut message =
                    language.format(Message::SettingsPluginsInstalled, &[("name", &plugin.name)]);
                if !themes.is_empty() {
                    message.push(' ');
                    message.push_str(&language.format(
                        Message::SettingsPluginsThemesAdded,
                        &[("count", &added.to_string()), ("total", &themes.len().to_string())],
                    ));
                }
                Ok(Some(message))
            })();
            (result, library)
        });
    }

    pub(super) fn set_plugin_enabled(&mut self, id: String, enabled: bool, cx: &mut Context<Self>) {
        if !self.plugins.idle() {
            return;
        }
        self.plugins.busy = Some(Busy::Plugin(id.clone()));
        let library = self.plugins.library();
        self.run_plugin_task(Some(id.clone()), cx, move || {
            let result = library.set_enabled(&id, enabled).map(|_| None);
            (result, library)
        });
    }

    pub(super) fn uninstall_plugin(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.plugins.idle() {
            return;
        }
        self.plugins.busy = Some(Busy::Plugin(id.clone()));
        let language = crate::gpui_shell::config::ui_language(cx);
        let library = self.plugins.library();
        self.run_plugin_task(None, cx, move || {
            let result = library.uninstall(&id).map(|()| {
                Some(language.format(Message::SettingsPluginsUninstalled, &[("id", &id)]))
            });
            (result, library)
        });
    }

    pub(super) fn generate_plugin_theme(
        &mut self,
        id: String,
        command: String,
        cx: &mut Context<Self>,
    ) {
        if !self.plugins.idle() {
            return;
        }
        self.plugins.busy = Some(Busy::Plugin(id.clone()));
        let language = crate::gpui_shell::config::ui_language(cx);
        let (library, store) = (self.plugins.library(), self.plugins.store());
        self.run_plugin_task(Some(id.clone()), cx, move || {
            let result = themes::generate(&library, &id, &command, &store).map(|document| {
                Some(
                    language
                        .format(Message::SettingsPluginsGenerated, &[("name", document.name())]),
                )
            });
            (result, library)
        });
    }

    /// Run one library operation off the UI thread, then reload the listing in
    /// the same task. A newer operation's sequence discards this result.
    fn run_plugin_task(
        &mut self,
        row: Option<String>,
        cx: &mut Context<Self>,
        operation: impl FnOnce() -> (crate::plugins::Result<Option<String>>, Library) + Send + 'static,
    ) {
        self.plugins.feedback = None;
        self.plugins.sequence = self.plugins.sequence.wrapping_add(1);
        let sequence = self.plugins.sequence;
        let language = crate::gpui_shell::config::ui_language(cx);
        let task = cx.background_executor().spawn(async move {
            let (result, library) = operation();
            (result, library.list().map_err(|error| error.to_string()))
        });
        // 已提交的文件操作即使页面关闭也会完成；弱引用与代次保证结果不会写回已销毁或更新的视图。
        cx.spawn(async move |this, cx| {
            let (result, listing) = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.plugins.sequence != sequence {
                    return;
                }
                this.plugins.loading = false;
                this.plugins.busy = None;
                this.plugins.listing = Some(listing);
                this.plugins.feedback = match result {
                    Ok(None) => None,
                    Ok(Some(message)) => Some((row, Ok(message))),
                    Err(error) => Some((
                        row,
                        Err(language.format(
                            Message::SettingsPluginsFailed,
                            &[("error", &error.to_string())],
                        )),
                    )),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
