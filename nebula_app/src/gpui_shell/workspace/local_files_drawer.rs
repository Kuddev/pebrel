//! Each source entity owns its model, inputs, scroll and validation task. The
//! shared SidePanel remains the authority for filesystem browsing and search.
use super::file_tree::{PathOrigin, ROW_PITCH, ROW_WASH_H, display_path, resolve_directory};
use super::*;
use crate::display::side_panel::SidePanel;
use crate::gpui_shell::file_drop::{FileDragGhost, FileTreeDrag};
use crate::i18n::Message;
use gpui::Focusable as _;
#[cfg(all(test, feature = "gpui-test-support"))]
use std::path::PathBuf;

pub(super) struct LocalFilesDrawer {
    source: gpui::WeakEntity<TerminalView>,
    model: SidePanel,
    scroll: gpui::UniformListScrollHandle,
    search: Entity<InputState>,
    path: Entity<InputState>,
    editing: Option<PathOrigin>,
    error: Option<String>,
    validation: Option<gpui::Task<()>>,
    polling: Option<gpui::Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl LocalFilesDrawer {
    pub(super) fn new(
        source: gpui::WeakEntity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let language = crate::gpui_shell::config::ui_language(cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(Message::DualFilesSearch))
        });
        let path = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(Message::FilesPathPlaceholder))
        });
        let search_sub = cx.subscribe_in(&search, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.model.set_file_search_query(input.read(cx).value().to_string());
                this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                cx.notify();
            }
        });
        let path_sub = cx.subscribe_in(&path, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => this.commit_path(window, cx),
            InputEvent::Change => {
                this.error = None;
                cx.notify();
            },
            _ => {},
        });
        Self {
            source,
            model: SidePanel::new(),
            scroll: gpui::UniformListScrollHandle::new(),
            search,
            path,
            editing: None,
            error: None,
            validation: None,
            polling: None,
            _subscriptions: vec![search_sub, path_sub],
        }
    }

    pub(super) fn set_visible(
        &mut self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.open = visible;
        if !visible {
            if self.path.read(cx).focus_handle(cx).is_focused(window)
                || self.search.read(cx).focus_handle(cx).is_focused(window)
            {
                window.blur();
            }
            self.polling = None;
            self.validation = None;
            return;
        }
        if self.polling.is_some() {
            return;
        }
        let executor = cx.background_executor().clone();
        self.polling = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                executor.timer(Duration::from_millis(100)).await;
                let keep = this
                    .update_in(cx, |this, window, cx| {
                        if !this.model.open {
                            return false;
                        }
                        let Some(source) = this.source.upgrade() else { return false };
                        let source = source.read(cx);
                        let wsl = source
                            .wsl_distro()
                            .zip(crate::shell_detect::wsl_guest_cwd(&source.cwd))
                            .map(|(distro, guest)| crate::shell_detect::WslCwd {
                                distro: distro.to_owned(),
                                guest: guest.to_owned(),
                            });
                        let (cwd, wsl) = if let Some(wsl) = wsl {
                            match crate::shell_detect::wsl_mounted_host_cwd(&wsl) {
                                Some(path) => (Some(path), None),
                                None => (None, Some(wsl)),
                            }
                        } else {
                            (source.local_cwd(), None)
                        };
                        if this.model.sync_at(cwd, wsl) {
                            if this.editing.as_ref().is_some_and(|origin| *origin != this.origin())
                            {
                                this.editing = None;
                                this.validation = None;
                                this.error = None;
                                if this.path.read(cx).focus_handle(cx).is_focused(window) {
                                    this.search.update(cx, |input, cx| input.focus(window, cx));
                                }
                            }
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    fn origin(&self) -> PathOrigin {
        PathOrigin {
            view: self.source.upgrade().map(|source| source.entity_id()),
            root: self.model.root().map(Path::to_path_buf),
            wsl: self.model.file_wsl_root().cloned(),
        }
    }

    #[cfg(all(test, feature = "gpui-test-support"))]
    pub(super) fn files_test_state(
        &self,
        cx: &App,
    ) -> (Option<PathBuf>, String, Option<PathBuf>, Vec<PathBuf>, usize, bool) {
        (
            self.model.root().map(Path::to_path_buf),
            self.search.read(cx).value().to_string(),
            self.model.selected.clone(),
            self.model.file_rows().iter().map(|row| row.path.clone()).collect(),
            (-self.scroll.0.borrow().base_handle.offset().y / px(ROW_PITCH)).floor().max(0.0)
                as usize,
            self.model.snapshot_pending() || self.model.file_search_pending(),
        )
    }

    #[cfg(all(test, feature = "gpui-test-support"))]
    pub(super) fn files_test_focus(&self, window: &Window, cx: &App) -> (bool, bool) {
        (
            self.path.read(cx).focus_handle(cx).is_focused(window),
            self.search.read(cx).focus_handle(cx).is_focused(window),
        )
    }

    fn begin_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let origin = self.origin();
        let value = origin
            .wsl
            .as_ref()
            .map(|wsl| wsl.guest.clone())
            .or_else(|| origin.root.as_deref().map(display_path))
            .unwrap_or_default();
        self.path.update(cx, |input, cx| {
            input.set_value(value.clone(), window, cx);
            input.set_selected_range(0..value.len(), cx);
            input.focus(window, cx);
        });
        self.editing = Some(origin);
        self.error = None;
        cx.notify();
    }

    fn cancel_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.validation = None;
        self.error = None;
        self.search.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn commit_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.validation.is_some() {
            return;
        }
        let Some(origin) = self.editing.clone() else { return };
        let text = self.path.read(cx).value().to_string();
        let checked = origin.clone();
        let check =
            cx.background_executor().spawn(async move { resolve_directory(&checked, &text) });
        self.validation = Some(cx.spawn_in(window, async move |this, cx| {
            let result = check.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.model.open
                    || this.editing.as_ref() != Some(&origin)
                    || this.origin() != origin
                {
                    return;
                }
                this.validation = None;
                match result {
                    Ok((path, guest)) => {
                        let already_open = guest.as_ref().map_or_else(
                            || this.model.root() == Some(path.as_path()),
                            |guest| {
                                this.model.file_wsl_root().is_some_and(|root| root.guest == *guest)
                            },
                        );
                        if !this.model.browse_directory(path, guest) && !already_open {
                            this.error = Some(
                                crate::gpui_shell::config::ui_language(cx)
                                    .text(Message::FilesPathUnavailable)
                                    .into(),
                            );
                            cx.notify();
                            return;
                        }
                        this.model.set_file_search_query(String::new());
                        this.search.update(cx, |input, cx| input.set_value("", window, cx));
                        this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                        this.editing = None;
                        this.error = None;
                        this.search.update(cx, |input, cx| input.focus(window, cx));
                    },
                    Err(error) => {
                        this.error = Some(
                            crate::gpui_shell::config::ui_language(cx)
                                .format(Message::FilesPathFailed, &[("error", &error.to_string())]),
                        )
                    },
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn render_path(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        if self.editing.is_none() {
            let origin = self.origin();
            let label = origin
                .wsl
                .map(|wsl| wsl.guest)
                .or_else(|| origin.root.as_deref().map(display_path))
                .unwrap_or_else(|| language.text(Message::FilesPathChoose).to_owned());
            return Button::new("local-files-path")
                .debug_selector(|| "local-files-path".into())
                .ghost()
                .small()
                .w_full()
                .min_w_0()
                .h(px(28.0))
                .tooltip(language.text(Message::FilesPathEdit))
                .child(div().w_full().min_w_0().truncate().text_left().text_xs().child(label))
                .on_click(cx.listener(|this, _, window, cx| this.begin_path(window, cx)))
                .into_any_element();
        }
        h_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .on_action(|_: &gpui_component::input::Enter, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel_path(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .debug_selector(|| "local-files-path-input".into())
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.path).h(px(28.0)).disabled(self.validation.is_some())),
            )
            .child(
                Button::new("local-files-path-go")
                    .debug_selector(|| "local-files-path-go".into())
                    .icon(IconName::Check)
                    .ghost()
                    .xsmall()
                    .disabled(self.validation.is_some())
                    .tooltip(language.text(Message::FilesPathOpen))
                    .on_click(cx.listener(|this, _, window, cx| this.commit_path(window, cx))),
            )
            .child(
                Button::new("local-files-path-cancel")
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip(language.text(Message::CommonCancel))
                    .on_click(cx.listener(|this, _, window, cx| this.cancel_path(window, cx))),
            )
            .into_any_element()
    }

    fn render_row(&self, index: usize, cx: &Context<Self>) -> gpui::AnyElement {
        let Some(row) = self.model.file_rows().get(index).cloned() else {
            return div().h(px(ROW_PITCH)).into_any_element();
        };
        let drag = FileTreeDrag {
            local_path: row
                .guest_path
                .as_ref()
                .zip(self.model.file_wsl_root())
                .map(|(guest, root)| crate::shell_detect::wsl_unc_path(&root.distro, guest))
                .unwrap_or_else(|| row.path.clone()),
            path_text: row.guest_path.clone().unwrap_or_else(|| row.path.display().to_string()),
            name: row.name.clone(),
        };
        let selected = self.model.selected.as_ref() == Some(&row.path);
        let path = row.path.clone();
        let guest = row.guest_path.clone();
        let icon = if row.is_dir {
            if row.expanded { IconName::FolderOpen } else { IconName::FolderClosed }
        } else {
            IconName::File
        };
        h_flex()
            .w_full()
            .h(px(ROW_PITCH))
            .px_1()
            .child(
                h_flex()
                    .id(format!("local-file-row-{index}"))
                    .debug_selector(move || format!("local-file-row-{index}"))
                    .h(px(ROW_WASH_H))
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .pl(px(8.0 + row.depth as f32 * 16.0))
                    .pr_1()
                    .rounded_md()
                    .text_color(if row.ignored {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().foreground
                    })
                    .when(selected, |item| item.bg(cx.theme().tab_active))
                    .hover(|item| item.bg(cx.theme().list_hover))
                    .active(|item| item.bg(cx.theme().list_active))
                    .child(Icon::new(icon).size(px(14.0)))
                    .child(div().min_w_0().truncate().text_sm().child(row.name.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.model.file_rows().get(index).map(|row| &row.path) != Some(&path) {
                            return;
                        }
                        this.model.click_row(index);
                        this.model.selected = Some(path.clone());
                        cx.notify();
                    }))
                    .when(row.is_dir && !row.is_parent, |item| {
                        item.on_double_click(cx.listener(move |this, _, window, cx| {
                            if this.model.file_rows().get(index).map(|current| &current.path)
                                != Some(&row.path)
                            {
                                return;
                            }
                            if this.model.browse_search_directory(row.path.clone(), guest.clone()) {
                                this.search.update(cx, |input, cx| input.set_value("", window, cx));
                                this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                                cx.notify();
                            }
                        }))
                    })
                    .when(!row.is_parent, |item| {
                        item.on_drag(drag, |drag, _, _, cx| {
                            cx.new(|_| FileDragGhost::new(drag.name.clone()))
                        })
                    }),
            )
            .into_any_element()
    }
}

impl Render for LocalFilesDrawer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let notice =
            self.error.clone().or_else(|| self.model.localized_root_notice(language)).or_else(
                || {
                    if self.model.file_search_error().is_some() {
                        Some(language.text(Message::FilesSearchFailed).into())
                    } else if self.model.file_search_pending() {
                        Some(language.text(Message::FilesSearchRunning).into())
                    } else if self.model.snapshot_pending() {
                        Some(language.text(Message::FilesEmptyReadingTitle).into())
                    } else if self.model.file_rows().iter().all(|row| row.is_parent) {
                        Some(
                            language
                                .text(if self.model.search.trim().is_empty() {
                                    Message::FilesEmptyDirectoryEmptyTitle
                                } else {
                                    Message::FilesSearchEmpty
                                })
                                .into(),
                        )
                    } else {
                        None
                    }
                },
            );
        let path = self.render_path(cx);
        v_flex()
            .id("local-files-browser")
            .debug_selector(|| "local-files-browser".into())
            .size_full()
            .min_w_0()
            .p_2()
            .gap_2()
            .overflow_hidden()
            .child(path)
            .child(
                div()
                    .debug_selector(|| "local-files-search".into())
                    .w_full()
                    .child(Input::new(&self.search).h(px(28.0)).text_size(px(12.0)).w_full()),
            )
            .when_some(notice, |panel, notice| {
                panel.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(notice),
                )
            })
            .child(
                div()
                    .debug_selector(|| "local-files-list".into())
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .child(
                        gpui::uniform_list(
                            "local-files-rows",
                            self.model.file_rows().len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range.map(|index| this.render_row(index, cx)).collect()
                            }),
                        )
                        .size_full()
                        .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
                        .track_scroll(&self.scroll),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(16.0))
                            .child(gpui_component::scroll::Scrollbar::vertical(&self.scroll)),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .whitespace_normal()
                    .child(language.text(Message::DualFilesDrag)),
            )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
