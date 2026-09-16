//! Open a project folder without sending commands to an existing terminal.

use std::path::PathBuf;

use crate::i18n::Message;
use crate::session::LaunchSession;

use super::*;

impl NebulaWorkspace {
    pub(super) fn render_open_project_button(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let pending = self.project_picker.is_some();
        let focus_color = cx.theme().ring;
        let label = language.text(if pending {
            Message::WorkspaceProjectPicking
        } else {
            Message::WorkspaceProjectOpen
        });
        title_bar_panel_controls()
            .id("project-folder-control")
            .role(gpui::Role::Button)
            .aria_label(label)
            .track_focus(&self.project_focus)
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(gpui::transparent_black())
            .focus(move |style| style.border_color(focus_color))
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_project_folder(window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    this.open_project_folder(window, cx);
                }
            }))
            .size(px(32.0))
            .debug_selector(|| "open-project-folder".to_owned())
            .child(
                Button::new("open-project-folder")
                    .role(gpui_component::RoleOverride::Presentational)
                    .tab_stop(false)
                    .icon(IconName::FolderOpen)
                    .ghost()
                    .size_full()
                    .loading(pending)
                    .tooltip(label)
                    .on_click(|_, _, _| {}),
            )
    }

    fn open_project_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_picker.is_some() {
            return;
        }
        // Freeze the default shell for this request, not whichever tab is focused
        // when the native dialog eventually returns. No existing PTY receives input.
        let launch = shell_launch::configured_local_launch(cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(language.text(Message::WorkspaceProjectOpen).into()),
        });
        let selection = async move {
            let paths = picked
                .await
                .map_err(std::io::Error::other)?
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(paths.and_then(|paths| paths.into_iter().next()))
        };
        self.open_project_selection(launch, selection, window, cx);
    }

    /// The native picker owns selection; this task owns validation and launch.
    /// Keeping that boundary explicit also lets native-executor acceptance cover
    /// real PTYs without opening a modal dialog over a user's active session.
    fn open_project_selection(
        &mut self,
        launch: LaunchSession,
        selection: impl std::future::Future<Output = std::io::Result<Option<PathBuf>>> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_picker.is_some() {
            return;
        }
        self.project_picker = Some(cx.spawn_in(window, async move |this, cx| {
            let result: std::io::Result<Option<PathBuf>> = async {
                let Some(path) = selection.await? else {
                    return Ok(None);
                };
                // A network/mounted directory may be slow; never stat it on the UI thread.
                cx.background_executor()
                    .spawn(async move { validate_project_directory(path).map(Some) })
                    .await
            }
            .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.project_picker = None;
                match result {
                    Ok(Some(path)) => {
                        let launch = project_launch_at(launch, &path);
                        this.add_terminal_with(launch, Some(path), None, window, cx);
                    },
                    Ok(None) => {},
                    Err(error) => {
                        log::warn!("could not open project folder: {error:#}");
                        let language = crate::gpui_shell::config::ui_language(cx);
                        crate::gpui_shell::toast::banner(
                            window,
                            cx,
                            crate::display::ToastKind::Warning,
                            language.text(Message::WorkspaceProjectOpenFailed),
                        );
                    },
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn validate_project_directory(path: PathBuf) -> std::io::Result<PathBuf> {
    if !std::fs::metadata(&path)?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "selected path is not a directory",
        ));
    }
    Ok(path)
}

fn project_launch_at(mut launch: LaunchSession, path: &Path) -> LaunchSession {
    // A saved profile cwd must not silently override an explicitly chosen project.
    // Update only the new tab's launch snapshot, never the stored terminal profile.
    if let LaunchSession::Profile { cwd, .. } = &mut launch {
        *cwd = Some(path.to_string_lossy().into_owned());
    }
    launch
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

#[cfg(all(test, feature = "gpui-test-support", target_os = "windows"))]
mod native_tests;
