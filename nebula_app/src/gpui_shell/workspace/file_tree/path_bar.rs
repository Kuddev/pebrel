//! The path field owns only its draft. Navigation still goes through SidePanel.

use super::*;
use crate::i18n::Message;
use gpui::{EntityId, KeyDownEvent, Task};

#[derive(Clone, PartialEq)]
struct PathOrigin {
    view: Option<EntityId>,
    root: Option<PathBuf>,
    wsl: Option<crate::shell_detect::WslCwd>,
}

pub(in crate::gpui_shell::workspace) struct PathEditor {
    input: Entity<InputState>,
    origin: PathOrigin,
    error: Option<String>,
    pending: Option<Task<()>>,
    _subscription: Subscription,
}

fn display_path(path: &std::path::Path) -> String {
    let value = path.to_string_lossy();
    if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
    }
}

fn breadcrumbs(origin: &PathOrigin) -> Vec<(String, String)> {
    if let Some(wsl) = &origin.wsl {
        let mut parts = vec![("/".to_owned(), "/".to_owned())];
        let mut path = String::new();
        for name in wsl.guest.split('/').filter(|name| !name.is_empty()) {
            path.push('/');
            path.push_str(name);
            parts.push((name.to_owned(), path.clone()));
        }
        return parts;
    }
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut path = PathBuf::new();
    if let Some(root) = &origin.root {
        let shown = display_path(root);
        for component in std::path::Path::new(&shown).components() {
            path.push(component.as_os_str());
            if component == std::path::Component::RootDir && !parts.is_empty() {
                // Windows 的盘符和根分隔符属于同一个导航目标。
                parts.last_mut().unwrap().1 = display_path(&path);
            } else {
                parts.push((
                    component.as_os_str().to_string_lossy().into_owned(),
                    display_path(&path),
                ));
            }
        }
    }
    parts
}

fn resolve_directory(
    origin: &PathOrigin,
    text: &str,
) -> std::io::Result<(PathBuf, Option<String>)> {
    let text = text.trim();
    let text = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(text);
    if text.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Empty directory path"));
    }
    let (path, guest) = if let Some(wsl) = &origin.wsl {
        let guest = crate::ssh_sftp::normalize_remote_path(&wsl.guest, text);
        (crate::shell_detect::wsl_unc_path(&wsl.distro, &guest), Some(guest))
    } else {
        let path = if text == "~" || text.starts_with("~/") || text.starts_with("~\\") {
            crate::platform::dirs::home_dir()
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory unavailable")
                })?
                .join(text.get(2..).unwrap_or(""))
        } else {
            PathBuf::from(text)
        };
        let path = if path.is_absolute() {
            path
        } else {
            origin
                .root
                .clone()
                .or_else(crate::platform::dirs::home_dir)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Enter an absolute directory path",
                    )
                })?
                .join(path)
        };
        (path, None)
    };
    if !std::fs::metadata(&path)?.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotADirectory, "Not a directory"));
    }
    // 本地路径先消解 .. 和链接，避免后续“返回上级”沿未规范化的拼写走错目录。
    let path =
        if guest.is_none() { PathBuf::from(display_path(&path.canonicalize()?)) } else { path };
    Ok((path, guest))
}

impl NebulaWorkspace {
    fn file_tree_path_origin(&self) -> PathOrigin {
        use super::super::WorkspaceTab;
        let view = self.tabs.get(self.active).and_then(|tab| match tab {
            WorkspaceTab::Terminal { .. } => tab.focused_view().map(Entity::entity_id),
            WorkspaceTab::Document { view, .. } => Some(view.entity_id()),
            WorkspaceTab::Code { view, .. } => Some(view.entity_id()),
            WorkspaceTab::Image { view } => Some(view.entity_id()),
            WorkspaceTab::Settings { view, .. } => Some(view.entity_id()),
            WorkspaceTab::Welcome { view, .. } => Some(view.entity_id()),
        });
        PathOrigin {
            view,
            root: self.side_panel.root().map(std::path::Path::to_path_buf),
            wsl: self.side_panel.file_wsl_root().cloned(),
        }
    }

    fn begin_file_tree_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let origin = self.file_tree_path_origin();
        let value = origin
            .wsl
            .as_ref()
            .map(|wsl| wsl.guest.clone())
            .or_else(|| origin.root.as_deref().map(display_path))
            .unwrap_or_default();
        let language = super::super::workspace_ui_language();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .placeholder(language.text(Message::FilesPathPlaceholder));
            input.set_value(value.clone(), window, cx);
            input.set_selected_range(0..value.len(), cx);
            input
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.commit_file_tree_path(window, cx),
                InputEvent::Change => {
                    if let Some(edit) = this.file_tree_path.as_mut() {
                        edit.error = None;
                    }
                    cx.notify();
                },
                _ => {},
            });
        self.file_tree_path = Some(PathEditor {
            input: input.clone(),
            origin,
            error: None,
            pending: None,
            _subscription: subscription,
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn cancel_file_tree_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree_path = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn navigate_file_tree_breadcrumb(
        &mut self,
        target: String,
        origin: &PathOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_tree_path_origin() != *origin {
            return;
        }
        self.begin_file_tree_path_edit(window, cx);
        if let Some(edit) = &self.file_tree_path {
            edit.input.update(cx, |input, cx| input.set_value(target, window, cx));
        }
        self.commit_file_tree_path(window, cx);
    }

    fn commit_file_tree_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.file_tree_path.as_ref().filter(|edit| edit.pending.is_none()) else {
            return;
        };
        let input = edit.input.clone();
        let origin = edit.origin.clone();
        let text = input.read(cx).value().to_string();
        let checked_origin = origin.clone();
        // 磁盘/UNC 检查只在提交时后台执行，输入和正常绘制都不触碰文件系统。
        let check = cx
            .background_executor()
            .spawn(async move { resolve_directory(&checked_origin, &text) });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = check.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.file_tree_path.as_ref().is_some_and(|edit| edit.input == input) {
                    return;
                }
                // 关闭、取消、切换目录/标签后，旧校验不得改变新的浏览上下文。
                if !this.side_panel.open || this.file_tree_path_origin() != origin {
                    this.file_tree_path = None;
                    cx.notify();
                    return;
                }
                match result {
                    Ok((path, guest)) => {
                        // WSL 的树根是 guest 标识，不是校验目录用的宿主 UNC 路径。
                        let already_open = match guest.as_deref() {
                            Some(guest) => this
                                .side_panel
                                .file_wsl_root()
                                .is_some_and(|root| root.guest == guest),
                            None => this.side_panel.root() == Some(path.as_path()),
                        };
                        let changed = this.side_panel.browse_directory(path, guest);
                        if changed || already_open {
                            this.side_panel.set_file_search_query(String::new());
                            this.file_tree_search_input
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            this.file_tree_scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                            this.file_tree_path = None;
                            this.focus_active(window, cx);
                        } else if let Some(edit) = this.file_tree_path.as_mut() {
                            edit.pending = None;
                            edit.error = Some(
                                super::super::workspace_ui_language()
                                    .text(Message::FilesPathUnavailable)
                                    .into(),
                            );
                        }
                    },
                    Err(error) => {
                        let language = super::super::workspace_ui_language();
                        let edit = this.file_tree_path.as_mut().unwrap();
                        edit.pending = None;
                        edit.error = Some(
                            language
                                .format(Message::FilesPathFailed, &[("error", &error.to_string())]),
                        );
                    },
                }
                cx.notify();
            });
        });
        self.file_tree_path.as_mut().unwrap().pending = Some(task);
        cx.notify();
    }

    pub(super) fn render_file_tree_path(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let origin = self.file_tree_path_origin();
        if self.file_tree_path.as_ref().is_some_and(|edit| edit.origin != origin) {
            self.file_tree_path = None;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let Some(edit) = self.file_tree_path.as_ref() else {
            let parts = breadcrumbs(&origin);
            let count = parts.len();
            let first_visible = count.saturating_sub(3);
            let theme = cx.theme();
            let hover_ink = hover_text_color(cx);
            let mut bar = h_flex()
                .id("file-tree-breadcrumbs")
                .debug_selector(|| "file-tree-breadcrumbs".into())
                .h(px(28.0))
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .gap(px(1.0));
            if first_visible > 0 {
                let ancestors = parts[..first_visible].to_vec();
                let workspace = cx.entity().downgrade();
                let menu_origin = origin.clone();
                bar = bar.child(
                    color_feedback_button(
                        "file-tree-crumb-overflow",
                        "...",
                        theme.muted_foreground,
                        hover_ink,
                    )
                    .debug_selector(|| "file-tree-crumb-overflow".into())
                    .small()
                    .h(px(28.0))
                    .px(px(3.0))
                    .aria_label(language.text(Message::FilesPathAncestors))
                    .tooltip(language.text(Message::FilesPathAncestors))
                    .dropdown_menu(move |mut menu, _, _| {
                        for (_, target) in &ancestors {
                            let destination = target.clone();
                            let expected_origin = menu_origin.clone();
                            let owner = workspace.clone();
                            menu = menu.item(
                                PopupMenuItem::new(target.clone())
                                    .icon(IconName::FolderClosed)
                                    .on_click(move |_, window, cx| {
                                        if let Some(owner) = owner.upgrade() {
                                            owner.update(cx, |this, cx| {
                                                this.navigate_file_tree_breadcrumb(
                                                    destination.clone(),
                                                    &expected_origin,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            );
                        }
                        menu
                    }),
                );
            }
            for (index, (label, target)) in parts.into_iter().enumerate().skip(first_visible) {
                let current = index + 1 == count;
                if index > 0 {
                    bar = bar.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.0))
                            .text_color(theme.muted_foreground.opacity(0.55))
                            .child("/"),
                    );
                }
                let click_origin = origin.clone();
                bar = bar.child(
                    color_feedback_button(SharedString::from(if current {
                        "file-tree-path".to_owned()
                    } else {
                        format!("file-tree-crumb-{index}")
                    }),
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(theme.mono_font_family.clone())
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .child(label.clone()),
                        if current { theme.foreground } else { theme.muted_foreground },
                        hover_ink,
                    )
                    .debug_selector(move || {
                        if current {
                            "file-tree-path".into()
                        } else if index + 2 == count {
                            "file-tree-crumb-parent".into()
                        } else {
                            format!("file-tree-crumb-{index}")
                        }
                    })
                    .small()
                    .h(px(28.0))
                    .min_w_0()
                    // Button 默认禁止收缩；路径变深时必须让各段在栏内截断，保留点击区域。
                    .flex_shrink_1()
                    .px(px(2.0))
                    .overflow_hidden()
                    .aria_label(label)
                    .tooltip(if current {
                        language.text(Message::FilesPathEdit).to_owned()
                    } else {
                        target.clone()
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if current {
                            if this.file_tree_path_origin() == click_origin {
                                this.begin_file_tree_path_edit(window, cx);
                            }
                        } else {
                            // 可见目录和折叠列表共用后台校验及旧结果隔离。
                            this.navigate_file_tree_breadcrumb(target.clone(), &click_origin, window, cx);
                        }
                    })),
                );
            }
            if count == 0 {
                bar =
                    bar.child(
                        color_feedback_button(
                            "file-tree-path",
                            language.text(Message::FilesPathChoose),
                            theme.muted_foreground,
                            hover_ink,
                        )
                        .debug_selector(|| "file-tree-path".into())
                        .small()
                        .h(px(28.0))
                        .px(px(2.0))
                        .aria_label(language.text(Message::FilesPathChoose))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.begin_file_tree_path_edit(window, cx)
                        })),
                    );
            }
            return bar.into_any_element();
        };
        let busy = edit.pending.is_some();
        h_flex()
            .id("file-tree-path-editor")
            .debug_selector(|| "file-tree-path-editor".into())
            .w_full()
            .min_w_0()
            .gap_1()
            // 单行输入框会冒泡 Enter；提交动作必须在这里结束，避免换行回写清空全选路径。
            .on_action(|_: &gpui_component::input::Enter, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    cx.stop_propagation();
                    this.cancel_file_tree_path_edit(window, cx);
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&edit.input).h(px(28.0)).text_size(px(12.0)).disabled(busy)),
            )
            .child(
                Button::new("file-tree-path-go")
                    .icon(IconName::Check)
                    .ghost()
                    .xsmall()
                    .disabled(busy)
                    .tooltip(language.text(Message::FilesPathOpen))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.commit_file_tree_path(window, cx)),
                    ),
            )
            .child(
                Button::new("file-tree-path-cancel")
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip(language.text(Message::CommonCancel))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_file_tree_path_edit(window, cx)
                    })),
            )
            .into_any_element()
    }

    pub(in crate::gpui_shell::workspace) fn file_tree_path_error(&self) -> Option<&String> {
        self.file_tree_path.as_ref().and_then(|edit| edit.error.as_ref())
    }
}
