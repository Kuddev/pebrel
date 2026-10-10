//! Workspace export dialog and its completion feedback.

use super::*;
use crate::i18n::Message;

impl NebulaWorkspace {
    pub(super) fn prompt_save_workspace(
        &self,
        export: crate::session::Session,
        stem: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stem: String = stem
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        let directory =
            crate::platform::dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let prompt =
            cx.prompt_for_new_path(&directory, Some(&format!("{stem}.nebula-workspace.json")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = prompt.await else { return };
            let result = crate::session::save_to(&path, &export);
            let _ = this.update_in(cx, |_, window, cx| {
                let language = crate::gpui_shell::config::ui_language(cx);
                match result {
                    Ok(()) => crate::gpui_shell::toast::toast(
                        window,
                        cx,
                        crate::display::ToastKind::Success,
                        language.format(
                            Message::WorkspaceExported,
                            &[("path", &path.display().to_string())],
                        ),
                    ),
                    Err(error) => crate::gpui_shell::toast::toast(
                        window,
                        cx,
                        crate::display::ToastKind::Warning,
                        language.format(
                            Message::WorkspaceExportFailed,
                            &[("error", &error.to_string())],
                        ),
                    ),
                }
            });
        })
        .detach();
    }
}
