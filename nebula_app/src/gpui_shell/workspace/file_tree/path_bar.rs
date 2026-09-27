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
            let value = origin
                .wsl
                .as_ref()
                .map(|wsl| wsl.guest.clone())
                .or_else(|| origin.root.as_deref().map(display_path))
                .unwrap_or_else(|| language.text(Message::FilesPathChoose).to_owned());
            return Button::new("file-tree-path")
                .debug_selector(|| "file-tree-path".into())
                .ghost()
                .small()
                .h(px(28.0))
                .w_full()
                .min_w_0()
                .px_1()
                .overflow_hidden()
                .tooltip(language.text(Message::FilesPathEdit))
                .child(div().w_full().min_w_0().truncate().text_left().text_xs().child(value))
                .on_click(
                    cx.listener(|this, _, window, cx| this.begin_file_tree_path_edit(window, cx)),
                )
                .into_any_element();
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
