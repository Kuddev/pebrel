//! 远端文件浏览器：抽屉在 SSH pane 上的另一种渲染。
//!
//! # 为什么不是一个独立面板
//!
//! 用户心里只有一个"文件"入口。远端浏览器如果做成第三个抽屉页签，同一件事
//! （看当前这台机器上的文件）就有了两个入口，而且用户得先知道自己在本地还是
//! 远端才能选对——那本来是程序该知道的事。
//!
//! 所以判据是：**抽屉的"文件"视图渲染谁，由聚焦 pane 的身份决定。** 聚焦
//! SSH pane 就画远端列表，切回本地 tab 就自动翻回本地目录树。浏览状态按
//! pane 留着，切回来还在原来那个目录。
//!
//! # 异步怎么回到界面
//!
//! 传输和列目录跑在本项目自己的网络 runtime 上（连接池和认证策略都在那儿），
//! 而界面更新必须回到 GPUI 的执行器。两者用一条 oneshot 接起来：网络侧算完
//! 把结果送进管道，GPUI 侧 `cx.spawn` 等在管道另一端，拿到就写状态 + `notify`。
//!
//! 落地时必须校验**世代号**。用户快速点几层目录时，先发的请求可能后到；不校验
//! 就会出现"点进 c 目录，界面却显示 b 目录的内容"。世代号对不上的响应直接
//! 丢弃——它描述的是一个已经不存在的意图。

mod drag;
#[cfg(all(test, feature = "gpui-test-support"))]
mod follow_tests;
mod interaction;
mod selection;
mod target;
mod transfers;
use selection::Selection;
use target::RemoteTransferTarget;

use gpui::AppContext as _;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, ExternalPaths, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px, uniform_list,
};

use crate::gpui_shell::prelude::*;
use crate::i18n::Message;
use crate::ssh_sftp::{
    SftpBrowseSession, SftpConflictPolicy, SftpController, SftpEntry, SftpEntryKind, SftpPhase,
    SftpSnapshot, SftpTransferOptions,
};

use super::{NebulaWorkspace, workspace_ui_language};

const TEXT_INSET: f32 = 12.0;
pub(super) const KEY_CONTEXT: &str = "RemoteFiles";
gpui::actions!(remote_files, [UploadFiles, DownloadSelection]);

#[derive(Clone)]
struct RemoteDirectorySnapshot {
    destination: String,
    path: String,
    entries: Vec<SftpEntry>,
    error: Option<String>,
    selection: Selection,
    shell_cwd: Option<String>,
    retry_path: Option<String>,
}

/// 远端浏览器的状态。
///
/// 当前可见状态只有一份，但每个 pane 都保留最近一次已确认的目录快照与滚动
/// 句柄。切 tab 只是换回这份快照，不触发网络列目录；连接池是否复用仍由下层
/// 按目的地裁定，与 UI 缓存的所有权互不混淆。
#[derive(Default)]
pub(super) struct RemoteBrowser {
    /// 当前绑定的 pane。`None` 表示抽屉此刻不该画远端内容。
    pane: Option<u64>,
    /// 当前绑定的远端目的地，用于标题和后续请求。
    destination: String,
    /// 每个 pane 最近一次已确认的列表。不能只记路径：切回来再按路径列一次，
    /// 本质上仍是 tab 激活触发刷新，也会丢掉选择和瞬时滚动上下文。
    snapshots: HashMap<u64, RemoteDirectorySnapshot>,
    /// One interactive SFTP subsystem per SSH pane. Bulk transfers deliberately
    /// use `transfer` below so a large copy cannot head-of-line block browsing.
    browse_sessions: HashMap<u64, SftpBrowseSession>,
    /// uniform_list 的 handle 内含滚动位置；每个 pane 各持一个，避免 A 主机的
    /// 长目录把 B 主机顶到同一个偏移。
    scrolls: HashMap<u64, gpui::UniformListScrollHandle>,
    /// 当前列出的目录。
    path: String,
    /// Last cwd reported by this pane. Manual browsing survives repeated
    /// prompts/polls until the shell actually changes directory.
    shell_cwd: Option<String>,
    pending_path: Option<String>,
    following_cwd: bool,
    navigation: Option<gpui::Task<()>>,
    retry_path: Option<String>,
    entries: Vec<SftpEntry>,
    /// 正在等一次列目录的响应。
    loading: bool,
    /// 上一次失败的原因。与 `entries` 并存：读不到新目录时旧列表还留在屏幕上
    /// 更有用（用户至少知道自己刚才在哪），错误单独一行说明为什么没动。
    error: Option<String>,
    /// 导航世代号。晚发早到的响应靠它被丢弃。
    generation: u64,
    selection: Selection,
    focus_handle: Option<gpui::FocusHandle>,
    /// 当前唯一的传输控制器。任务可以跨 pane 切换继续跑，但只有目的地与当前
    /// pane 一致时才把完成结果写回列表，避免 A 主机的结果覆盖 B 主机。
    transfer: Option<SftpController>,
    /// 控制器世代号。旧控制器的 wake 可能晚到，不能因此读取刚装上的新控制器。
    transfer_id: u64,
    skip_unchanged: bool,
    preflighting: bool,
    preflight_id: u64,
    last_outcome: Option<crate::i18n::Message>,
    /// 远端复制载荷必须带稳定的源 destination，不能从当前标题反推源主机。
    clipboard: Option<RemoteClipboard>,
}

#[derive(Clone)]
struct RemoteClipboard {
    source_destination: String,
    entries: Vec<SftpEntry>,
}

#[derive(Clone)]
enum PendingRemoteTransfer {
    Upload(Vec<PathBuf>),
    Download { entries: Vec<SftpEntry>, local_directory: PathBuf },
    Copy(RemoteClipboard),
}

impl Drop for RemoteBrowser {
    fn drop(&mut self) {
        if let Some(controller) = &self.transfer {
            controller.cancel();
        }
    }
}

impl RemoteBrowser {
    /// 抽屉此刻是否应该画远端内容。
    pub(super) fn active(&self) -> bool {
        self.pane.is_some()
    }

    fn park_active(&mut self) {
        let Some(pane) = self.pane else { return };
        if self.path.is_empty() {
            return;
        }
        self.snapshots.insert(
            pane,
            RemoteDirectorySnapshot {
                destination: self.destination.clone(),
                path: self.path.clone(),
                entries: self.entries.clone(),
                error: self.error.clone(),
                selection: self.selection.clone(),
                // A switch cancels unfinished navigation. Reobserve the pane
                // cwd when it returns rather than treating its old listing as
                // already synchronized with the cancelled target.
                shell_cwd: if self.loading && self.following_cwd {
                    None
                } else {
                    self.shell_cwd.clone()
                },
                retry_path: self.retry_path.clone(),
            },
        );
    }

    /// 绑定 pane。返回 `true` 表示已有完整快照，调用方不得再列目录。
    fn bind(&mut self, pane: u64, destination: String) -> bool {
        self.last_outcome = None;
        self.park_active();
        self.navigation = None;
        self.pending_path = None;
        self.following_cwd = false;
        self.generation = self.generation.wrapping_add(1);
        self.pane = Some(pane);
        self.destination = destination.clone();
        self.loading = false;
        self.scrolls.entry(pane).or_insert_with(gpui::UniformListScrollHandle::new);
        let replace_session = self
            .browse_sessions
            .get(&pane)
            .is_none_or(|session| session.destination() != destination);
        if replace_session {
            self.browse_sessions.insert(pane, SftpBrowseSession::new(destination.clone()));
        }

        let snapshot = self
            .snapshots
            .get(&pane)
            .filter(|snapshot| snapshot.destination == destination)
            .cloned();
        if let Some(snapshot) = snapshot {
            self.path = snapshot.path;
            self.entries = snapshot.entries;
            self.error = snapshot.error;
            self.selection = snapshot.selection;
            self.shell_cwd = snapshot.shell_cwd;
            self.retry_path = snapshot.retry_path;
            return true;
        }

        // 同一个 pane 重新连到另一个目的地时，旧主机快照不能复用。
        self.snapshots.remove(&pane);
        self.path.clear();
        self.entries.clear();
        self.error = None;
        self.selection.clear();
        self.shell_cwd = None;
        self.retry_path = None;
        false
    }

    fn scroll_for(&mut self, pane: u64) -> gpui::UniformListScrollHandle {
        self.scrolls.entry(pane).or_insert_with(gpui::UniformListScrollHandle::new).clone()
    }

    pub(super) fn forget(&mut self, pane: u64) {
        self.snapshots.remove(&pane);
        self.scrolls.remove(&pane);
        self.browse_sessions.remove(&pane);
        if self.pane == Some(pane) {
            self.pane = None;
            self.destination.clear();
            self.path.clear();
            self.entries.clear();
            self.error = None;
            self.loading = false;
            self.selection.clear();
            self.shell_cwd = None;
            self.retry_path = None;
            self.pending_path = None;
            self.following_cwd = false;
            self.navigation = None;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// 解绑：聚焦回本地 pane 时调用。目录、选择和滚动位置先按 pane 停放；
    /// 连接本身由下层按目的地缓存，这里不触碰 transport。
    fn detach(&mut self) {
        self.park_active();
        self.pane = None;
        self.destination.clear();
        self.path.clear();
        self.entries.clear();
        self.error = None;
        self.loading = false;
        self.selection.clear();
        self.shell_cwd = None;
        self.retry_path = None;
        self.pending_path = None;
        self.following_cwd = false;
        self.navigation = None;
        // 世代号往前走一格：已经在路上的响应回来时会发现自己过期了。
        self.generation = self.generation.wrapping_add(1);
    }

    /// Last confirmed remote path for duplicate-tab. It remains available
    /// while the drawer is closed because `park_active` stores it per pane.
    pub(super) fn path_for(&self, pane: u64, destination: &str) -> Option<&str> {
        if self.pane == Some(pane) && self.destination == destination && !self.path.is_empty() {
            return Some(&self.path);
        }
        self.snapshots
            .get(&pane)
            .filter(|snapshot| snapshot.destination == destination && !snapshot.path.is_empty())
            .map(|snapshot| snapshot.path.as_str())
    }
}

impl NebulaWorkspace {
    /// 聚焦 pane 的远端身份，`None` 表示本地 pane 或 SSH 还没就绪。
    ///
    /// 双条件门控在 [`TerminalView::ready_ssh_destination`] 里：既要是 SSH
    /// pane，又要握手已完成。
    pub(super) fn focused_remote_pane(&self, cx: &App) -> Option<(u64, String, Option<String>)> {
        let view = self.tabs.get(self.active).and_then(super::WorkspaceTab::focused_view)?;
        let view = view.read(cx);
        let destination = view.ready_ssh_destination()?.to_owned();
        Some((view.pane_id, destination, view.remote_cwd()))
    }

    /// 每帧把抽屉路由到聚焦 pane 的身份上。
    ///
    /// 返回抽屉这一帧是否该画远端内容。这是**唯一**的判据入口：渲染、跟随、
    /// 空态文案都问它，不各自重新判断一遍，否则三处判据迟早会分叉。
    pub(super) fn route_remote_browser(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        self.sync_remote_browser_to_active(cx);
        self.remote_browser.active()
    }

    /// Shared by terminal cwd events, drawer polling and render routing. Remote
    /// paths stay with the SFTP adapter and never reach the local filesystem tree.
    pub(super) fn sync_remote_browser_to_active(&mut self, cx: &mut Context<Self>) -> bool {
        match self.focused_remote_pane(cx) {
            Some((pane, destination, cwd)) => {
                let rebind = self.remote_browser.pane != Some(pane)
                    || self.remote_browser.destination != destination;
                if rebind {
                    self.attach_remote_browser(pane, destination, cwd, cx);
                    true
                } else {
                    self.follow_remote_shell_cwd(cwd, cx)
                }
            },
            None => {
                if self.remote_browser.active() {
                    self.remote_browser.detach();
                    cx.notify();
                    return true;
                }
                false
            },
        }
    }

    /// 绑定到一个远端 pane 并开始列目录。
    fn attach_remote_browser(
        &mut self,
        pane: u64,
        destination: String,
        cwd: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        let restored = self.remote_browser.bind(pane, destination.clone());
        self.remote_files_scroll = self.remote_browser.scroll_for(pane);
        if self.follow_remote_shell_cwd(cwd, cx) {
            return;
        }
        if restored {
            cx.notify();
            return;
        }

        // A reported pane cwd takes precedence over the connection-wide probe:
        // pooled connections can contain several PTYs with different directories.
        // Only a pane with no report or snapshot needs the existing fallback.
        self.navigate_remote_to_shell_cwd(pane, destination, cx);
    }

    fn follow_remote_shell_cwd(&mut self, cwd: Option<String>, cx: &mut Context<Self>) -> bool {
        let Some(cwd) = cwd else { return false };
        if self.remote_browser.shell_cwd.as_ref() == Some(&cwd) {
            return false;
        }
        self.remote_browser.shell_cwd = Some(cwd.clone());
        if self.remote_browser.path != cwd || self.remote_browser.pending_path.is_some() {
            self.navigate_remote(cwd, cx);
            self.remote_browser.following_cwd = true;
        }
        true
    }

    /// 问远端"用户此刻在哪个目录"，然后导航过去。
    fn navigate_remote_to_shell_cwd(
        &mut self,
        pane: u64,
        destination: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.remote_browser.loading = true;
        cx.notify();
        let generation = self.bump_remote_generation();
        self.remote_browser.navigation = Some(cx.spawn(async move |this, cx| {
            let probed = remote_call(|| probe_remote_shell_cwd(destination)).await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace.remote_browser.generation != generation
                    || workspace.remote_browser.pane != Some(pane)
                {
                    return;
                }
                // 跟不上就从根开始，但那是回退而不是目标。歧义（多个终端共用
                // 一条连接）也走这条路：宁可让用户自己点，也不要打开一个可能
                // 属于另一个终端的目录。
                let start = match probed {
                    Some(crate::ssh_sftp::remote_cwd::RemoteCwd::Located(path)) => path,
                    _ => "/".to_owned(),
                };
                workspace.navigate_remote(start, cx);
            });
        }));
    }

    /// 世代号 +1 并返回新值。所有会改变"当前该显示什么"的操作都要先过这里。
    fn bump_remote_generation(&mut self) -> u64 {
        self.remote_browser.generation = self.remote_browser.generation.wrapping_add(1);
        self.remote_browser.generation
    }

    /// 列出一个远端目录并显示它。
    pub(super) fn navigate_remote(&mut self, path: String, cx: &mut Context<'_, Self>) {
        self.remote_browser.last_outcome = None;
        self.remote_browser.following_cwd = false;
        let Some(pane) = self.remote_browser.pane else { return };
        let Some(session) = self.remote_browser.browse_sessions.get(&pane).cloned() else {
            self.remote_browser.loading = false;
            self.remote_browser.error = Some(
                crate::gpui_shell::config::ui_language(cx)
                    .text(crate::i18n::Message::FilesRemoteSessionUnavailable)
                    .to_owned(),
            );
            cx.notify();
            return;
        };
        let generation = self.bump_remote_generation();
        self.remote_browser.loading = true;
        self.remote_browser.pending_path = Some(path.clone());
        cx.notify();

        let target = path.clone();
        self.remote_browser.navigation = Some(cx.spawn(async move |this, cx| {
            let listed = remote_call(|| async move { session.list_dir(&target).await }).await;
            let _ = this.update(cx, |workspace, cx| {
                // 世代号和 pane 都要对：前者防乱序，后者防用户在等待期间切到
                // 别的 pane（那时这份结果属于另一台机器）。
                if workspace.remote_browser.generation != generation
                    || workspace.remote_browser.pane != Some(pane)
                {
                    return;
                }
                workspace.remote_browser.loading = false;
                workspace.remote_browser.pending_path = None;
                workspace.remote_browser.following_cwd = false;
                match listed {
                    Some(Ok(entries)) => {
                        let same_directory = workspace.remote_browser.path == path;
                        workspace.remote_browser.entries = entries;
                        workspace.remote_browser.path = path.clone();
                        workspace.remote_browser.error = None;
                        if same_directory {
                            let rows = workspace.remote_rows();
                            let paths: Vec<_> = rows.iter().map(|row| row.path.as_str()).collect();
                            workspace.remote_browser.selection.retain(&paths);
                        } else {
                            workspace.remote_browser.selection.clear();
                            workspace
                                .remote_files_scroll
                                .scroll_to_item(0, gpui::ScrollStrategy::Top);
                        }
                        workspace.remote_browser.retry_path = None;
                    },
                    Some(Err(message)) => {
                        workspace.remote_browser.error = Some(message);
                        workspace.remote_browser.retry_path = Some(path.clone());
                    },
                    None => {
                        workspace.remote_browser.error = Some(
                            crate::gpui_shell::config::ui_language(cx)
                                .text(crate::i18n::Message::FilesRemoteConnectionUnavailable)
                                .to_owned(),
                        );
                        workspace.remote_browser.retry_path = Some(path.clone());
                    },
                }
                cx.notify();
            });
        }));
    }

    /// 回到上一级。
    fn remote_parent(&mut self, cx: &mut Context<'_, Self>) {
        let parent = crate::ssh_sftp::normalize_remote_path(&self.remote_browser.path, "..");
        self.navigate_remote(parent, cx);
    }

    /// 重新列当前目录。
    fn remote_refresh(&mut self, cx: &mut Context<'_, Self>) {
        let path = self
            .remote_browser
            .retry_path
            .clone()
            .unwrap_or_else(|| self.remote_browser.path.clone());
        self.navigate_remote(path, cx);
    }

    fn selected_remote_entries(&self) -> Vec<SftpEntry> {
        self.remote_browser
            .entries
            .iter()
            .filter(|entry| !entry.is_parent && self.remote_browser.selection.contains(&entry.path))
            .cloned()
            .collect()
    }

    fn remote_rows(&self) -> Vec<SftpEntry> {
        let mut rows = Vec::with_capacity(self.remote_browser.entries.len() + 1);
        if self.remote_browser.path != "/" {
            rows.push(SftpEntry {
                name: "..".to_owned(),
                path: crate::ssh_sftp::normalize_remote_path(&self.remote_browser.path, ".."),
                kind: SftpEntryKind::Directory,
                size: 0,
                modified: 0,
                permissions: String::new(),
                is_parent: true,
            });
        }
        rows.extend(self.remote_browser.entries.iter().cloned());
        rows
    }

    /// 单击选中，双击目录进入。
    fn remote_activate(
        &mut self,
        row: SftpEntry,
        open: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if open && matches!(row.kind, SftpEntryKind::Directory | SftpEntryKind::Symlink) {
            self.navigate_remote(row.path, cx);
            return;
        }
        if open && row.kind == SftpEntryKind::File {
            self.open_remote_document(
                self.remote_browser.destination.clone(),
                row.path,
                window,
                cx,
            );
            return;
        }
        self.remote_browser.selection.select_only(&row.path);
        cx.notify();
    }

    /// The list fills the drawer between the compact toolbar and the transfer dock.
    pub(super) fn render_remote_files(&mut self, cx: &mut Context<'_, Self>) -> gpui::AnyElement {
        let transfer_status = self.render_remote_transfer_status(cx);
        let focus = self
            .remote_browser
            .focus_handle
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let busy = self.remote_transfer_working() || self.remote_browser.preflighting;
        let ready = self.current_remote_transfer_target().is_some();
        let has_selection = !self.selected_remote_entries().is_empty();
        let has_clipboard = self.remote_browser.clipboard.is_some();
        let skip_toggle = crate::gpui_shell::widgets::NebulaSwitch::new("sftp-skip-unchanged")
            .small()
            .checked(self.remote_browser.skip_unchanged)
            .disabled(busy)
            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                this.remote_browser.skip_unchanged = *checked;
                cx.notify();
            }));
        let language = crate::gpui_shell::config::ui_language(cx);
        let modifier = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
        let weak = cx.entity().downgrade();
        let upload = Button::new("remote-files-upload")
            .icon(Icon::new(IconName::ArrowUp).size(px(16.0)))
            .label(language.text(Message::CommonUpload))
            .small()
            .h(px(32.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .dropdown_caret(true)
            .disabled(!ready || busy)
            .tooltip(format!("{} ({modifier}+Shift+U)", language.text(Message::RemoteUploadFiles)))
            .dropdown_menu(move |mut menu, _, _| {
                for (label, directory) in
                    [(Message::RemoteUploadFiles, false), (Message::RemoteUploadFolder, true)]
                {
                    let owner = weak.clone();
                    menu = menu.item(
                        gpui_component::menu::PopupMenuItem::new(language.text(label)).on_click(
                            move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if directory {
                                        this.remote_pick_upload_directory(window, cx);
                                    } else {
                                        this.remote_pick_upload_files(window, cx);
                                    }
                                });
                            },
                        ),
                    );
                }
                menu
            });
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let drop_highlight = theme.accent.opacity(0.18);
        let location_background = theme.background.darken(0.035);
        let rows = self.remote_rows();
        let path = if self.remote_browser.path.is_empty() {
            language.text(Message::RemoteLocating).to_owned()
        } else {
            self.remote_browser.path.clone()
        };
        let location_tip = format!("{} · {}", self.remote_browser.destination, path);
        let at_root = self.remote_browser.path == "/" || self.remote_browser.path.is_empty();

        v_flex()
            .id("remote-files-panel")
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .occlude()
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop_highlight))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                cx.stop_propagation();
                this.drop_upload_paths(paths.paths().to_owned(), None, window, cx);
            }))
            .drag_over::<crate::gpui_shell::file_drop::FileTreeDrag>(move |style, _, _, _| {
                style.bg(drop_highlight)
            })
            .on_drop(cx.listener(
                |this, file: &crate::gpui_shell::file_drop::FileTreeDrag, window, cx| {
                    cx.stop_propagation();
                    this.drop_upload_paths(vec![file.local_path.clone()], None, window, cx);
                },
            ))
            .child(
                h_flex()
                    .id("remote-files-location")
                    .h(px(32.0))
                    .mx(px(TEXT_INSET))
                    .mt(px(8.0))
                    .pl(px(8.0))
                    .pr(px(2.0))
                    .gap(px(4.0))
                    .flex_shrink_0()
                    .rounded(px(8.0))
                    .bg(location_background)
                    .tooltip(move |window, cx| Tooltip::new(location_tip.clone()).build(window, cx))
                    .child(div().text_xs().text_color(muted).pr(px(6.0)).child("SFTP"))
                    .child(div().flex_1().min_w_0().text_xs().truncate().child(path))
                    .child(
                        remote_button(
                            "remote-files-up",
                            IconName::ArrowUp,
                            language.text(Message::RemoteParent),
                        )
                        .w(px(28.0))
                        .h(px(28.0))
                        .disabled(at_root || self.remote_browser.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.remote_parent(cx))),
                    )
                    .child(
                        remote_button(
                            "remote-files-refresh",
                            IconName::Redo2,
                            language.text(Message::RemoteReread),
                        )
                        .w(px(28.0))
                        .h(px(28.0))
                        .on_click(cx.listener(|this, _, _, cx| this.remote_refresh(cx))),
                    ),
            )
            .child(
                h_flex()
                    .px(px(TEXT_INSET))
                    .py(px(8.0))
                    .gap(px(2.0))
                    .flex_shrink_0()
                    .child(upload)
                    .child(
                        remote_button(
                            "remote-files-download",
                            IconName::ArrowDown,
                            language.text(Message::RemoteDownload),
                        )
                        .disabled(!ready || !has_selection || busy)
                        .tooltip(format!(
                            "{} ({modifier}+Shift+D)",
                            language.text(Message::RemoteDownload)
                        ))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.remote_pick_download_directory(window, cx)
                        })),
                    )
                    .child(
                        remote_button(
                            "remote-files-copy",
                            IconName::Copy,
                            language.text(Message::RemoteCopyItem),
                        )
                        .disabled(!has_selection)
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.remote_copy_selected(window, cx)
                            }),
                        ),
                    )
                    .child(
                        remote_button(
                            "remote-files-paste",
                            IconName::Inbox,
                            language.text(Message::RemotePasteHere),
                        )
                        .disabled(!ready || !has_clipboard || busy)
                        .on_click(cx.listener(|this, _, window, cx| this.remote_paste(window, cx))),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .justify_end()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(muted)
                                    .truncate()
                                    .child(language.text(Message::RemoteSkipUnchanged)),
                            )
                            .child(skip_toggle),
                    ),
            )
            .child(
                h_flex()
                    .h(px(24.0))
                    .px(px(16.0))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().child(language.text(Message::CommonName)))
                    .child(language.text(Message::CommonSize)),
            )
            .child(
                div()
                    .id("remote-files-list")
                    .key_context(KEY_CONTEXT)
                    .track_focus(&focus)
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .on_key_down(cx.listener(Self::remote_files_key_down))
                    .on_action(cx.listener(|this, _: &UploadFiles, window, cx| {
                        cx.stop_propagation();
                        this.remote_pick_upload_files(window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &DownloadSelection, window, cx| {
                        cx.stop_propagation();
                        this.remote_pick_download_directory(window, cx);
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.remote_browser.selection.clear();
                            this.remote_browser.last_outcome = None;
                            if let Some(focus) = &this.remote_browser.focus_handle {
                                focus.focus(window, cx);
                            }
                            cx.notify();
                        }),
                    )
                    .child(
                        uniform_list(
                            "remote-files-rows",
                            rows.len(),
                            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                                range
                                    .filter_map(|index| {
                                        let row = rows.get(index)?.clone();
                                        let joined = (
                                            index
                                                .checked_sub(1)
                                                .and_then(|i| rows.get(i))
                                                .is_some_and(|row| {
                                                    this.remote_browser
                                                        .selection
                                                        .contains(&row.path)
                                                }),
                                            rows.get(index + 1).is_some_and(|row| {
                                                this.remote_browser.selection.contains(&row.path)
                                            }),
                                        );
                                        Some(this.render_remote_row(row, joined, window, cx))
                                    })
                                    .collect()
                            }),
                        )
                        .absolute()
                        .inset_0()
                        .size_full()
                        .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
                        .track_scroll(&self.remote_files_scroll),
                    ),
            )
            .child(transfer_status)
            .into_any_element()
    }

    /// Idle occupies one visible status line; only a running transfer expands the dock.
    fn render_remote_transfer_status(&mut self, cx: &mut Context<'_, Self>) -> gpui::AnyElement {
        let snapshot = self.remote_transfer_snapshot().filter(|snapshot| {
            snapshot.destination == self.remote_browser.destination
                && matches!(snapshot.phase, SftpPhase::Working | SftpPhase::Error)
        });
        let selected = self.selected_remote_entries();
        let language = crate::gpui_shell::config::ui_language(cx);
        let is_working =
            snapshot.as_ref().is_some_and(|snapshot| snapshot.phase == SftpPhase::Working);
        let progress = snapshot.as_ref().and_then(|snapshot| snapshot.progress.as_ref());
        let label = if let Some(snapshot) = &snapshot {
            if snapshot.phase == SftpPhase::Error {
                language.text(Message::TransferFailed).to_owned()
            } else {
                progress
                    .map(|p| p.label.clone())
                    .unwrap_or_else(|| language.text(Message::RemotePreparing).to_owned())
            }
        } else if let Some(notice) = self.remote_notice(language) {
            notice
        } else if !selected.is_empty() {
            let mut label = language
                .format(Message::RemoteSelectedItems, &[("count", &selected.len().to_string())]);
            if selected.iter().all(|entry| entry.kind == SftpEntryKind::File) {
                let bytes =
                    selected.iter().fold(0u64, |total, entry| total.saturating_add(entry.size));
                label.push_str(&format!(" · {}", format_transfer_bytes(bytes)));
            }
            label
        } else {
            language.format(
                Message::RemoteItems,
                &[("count", &self.remote_browser.entries.len().to_string())],
            )
        };
        let detail = snapshot.as_ref().and_then(|snapshot| snapshot.error.clone()).or_else(|| {
            progress.map(|p| {
                format!(
                    "{} / {}",
                    format_transfer_bytes(p.transferred),
                    format_transfer_bytes(p.total)
                )
            })
        });
        let percent = progress.map(|progress| progress.fraction() * 100.0).unwrap_or(0.0);
        let loading = progress.is_none_or(|progress| progress.total == 0);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let has_error = self.remote_browser.error.is_some();
        let tip = label.clone();
        let can_download = self.current_remote_transfer_target().is_some()
            && !self.remote_transfer_working()
            && !self.remote_browser.preflighting;
        v_flex()
            .w_full()
            .flex_shrink_0()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background.darken(0.025))
            .when(is_working, |dock| {
                dock.child(
                    gpui_component::progress::Progress::new("remote-files-transfer-progress")
                        .small()
                        .loading(loading)
                        .value(percent),
                )
            })
            .child(
                h_flex()
                    .h(px(40.0))
                    .px(px(TEXT_INSET))
                    .gap(px(4.0))
                    .child(
                        div()
                            .id("remote-files-status")
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(if has_error { theme.danger } else { muted })
                            .truncate()
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                            .child(label),
                    )
                    .when(is_working, |line| {
                        line.child(
                            remote_button(
                                "remote-files-cancel-transfer",
                                IconName::CircleX,
                                language.text(Message::RemoteCancel),
                            )
                            .w(px(28.0))
                            .h(px(28.0))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.remote_cancel_transfer(cx)),
                            ),
                        )
                    })
                    .when(!is_working && !selected.is_empty(), |line| {
                        line.child(
                            remote_button(
                                "remote-files-download-selection",
                                IconName::ArrowDown,
                                language.text(Message::RemoteDownload),
                            )
                            .w(px(28.0))
                            .h(px(28.0))
                            .disabled(!can_download)
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.remote_pick_download_directory(window, cx)
                                },
                            )),
                        )
                        .child(
                            remote_button(
                                "remote-files-clear-selection",
                                IconName::Close,
                                language.text(Message::RemoteClearSelection),
                            )
                            .w(px(28.0))
                            .h(px(28.0))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.remote_browser.selection.clear();
                                this.remote_browser.last_outcome = None;
                                cx.notify();
                            })),
                        )
                    }),
            )
            .when_some(detail, |dock, detail| {
                let tip = detail.clone();
                dock.child(
                    div()
                        .id("remote-files-transfer-detail")
                        .px(px(TEXT_INSET))
                        .pb(px(8.0))
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .child(detail),
                )
            })
            .into_any_element()
    }

    fn render_remote_row(
        &self,
        row: SftpEntry,
        joined: (bool, bool),
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        let theme = cx.theme();
        let selected = self.remote_browser.selection.contains(&row.path);
        let focused = self.remote_browser.selection.focus() == Some(row.path.as_str())
            && self
                .remote_browser
                .focus_handle
                .as_ref()
                .is_some_and(|f| f.contains_focused(window, cx));
        let (icon, ink) = match row.kind {
            SftpEntryKind::Directory => {
                (crate::display::side_panel::folder_icon(false), theme.accent)
            },
            SftpEntryKind::Symlink => ("\u{ea71}", theme.muted_foreground),
            SftpEntryKind::File => {
                (crate::display::side_panel::file_type_icon(&row.name), theme.muted_foreground)
            },
        };
        let group = SharedString::from(format!("remote-row-{}", row.path));
        let label = if row.kind == SftpEntryKind::Directory && !row.is_parent {
            format!("{}/", row.name)
        } else {
            row.name.clone()
        };
        let size = if row.kind == SftpEntryKind::File {
            format_transfer_bytes(row.size)
        } else {
            "—".to_owned()
        };
        let activate = row.clone();
        let press = row.clone();
        let download = row.clone();
        let copy = row.clone();
        let busy = self.remote_transfer_working() || self.remote_browser.preflighting;
        let ready = self.current_remote_transfer_target().is_some();
        let native_drag = (!row.is_parent && !busy)
            .then(|| self.current_remote_transfer_target())
            .flatten()
            .map(|source| drag::RemoteFileDrag { source, entry: row.clone() });
        let weak = cx.entity().downgrade();
        let directory = (row.kind == SftpEntryKind::Directory).then(|| row.path.clone());
        let upload_directory = directory.clone();
        let highlight = theme.accent.opacity(0.18);
        let language = crate::gpui_shell::config::ui_language(cx);

        h_flex()
            .h(px(28.0))
            .w_full()
            .px(px(8.0))
            .child(
                h_flex()
                    .id(group.clone())
                    .group(group.clone())
                    .relative()
                    .h(px(28.0))
                    .flex_1()
                    .min_w_0()
                    .px(px(7.0))
                    .gap(px(8.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(if focused {
                        theme.ring.opacity(0.6)
                    } else {
                        gpui::transparent_black()
                    })
                    .when(selected, |item| {
                        item.bg(theme.tab_active)
                            .when(joined.0, |item| item.rounded_t(px(0.0)))
                            .when(joined.1, |item| item.rounded_b(px(0.0)))
                    })
                    .when(!selected, |item| item.hover(|item| item.bg(theme.list_hover)))
                    .child(
                        div()
                            .w(px(16.0))
                            .flex_shrink_0()
                            .font_family(SharedString::from(
                                crate::font_install::SYMBOL_FONT_FAMILY,
                            ))
                            .text_size(px(16.0))
                            .text_color(ink)
                            .child(icon),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.0))
                            .text_color(theme.foreground)
                            .truncate()
                            .child(label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.muted_foreground)
                            .flex_shrink_0()
                            .child(size),
                    )
                    .when(!row.is_parent, |item| {
                        item.child(
                            h_flex()
                                .id(SharedString::from(format!("remote-actions-{}", row.path)))
                                .absolute()
                                .right(px(3.0))
                                .top_0()
                                .h(px(26.0))
                                .pl(px(8.0))
                                .bg(if selected {
                                    theme.tab_active
                                } else {
                                    theme.background.blend(theme.list_hover)
                                })
                                .invisible()
                                .group_hover(group.clone(), |actions| actions.visible())
                                .when(focused, |actions| actions.visible())
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_key_down(|event, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        cx.stop_propagation();
                                    }
                                })
                                .child(
                                    remote_button(
                                        SharedString::from(format!("remote-download-{}", row.path)),
                                        IconName::ArrowDown,
                                        language.text(Message::RemoteDownload),
                                    )
                                    .w(px(28.0))
                                    .h(px(26.0))
                                    .tab_stop(focused)
                                    .disabled(!ready || busy)
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.remote_action_selection(&download);
                                            this.remote_pick_download_directory(window, cx);
                                        },
                                    )),
                                )
                                .child(
                                    remote_button(
                                        SharedString::from(format!("remote-copy-{}", row.path)),
                                        IconName::Copy,
                                        language.text(Message::RemoteCopyItem),
                                    )
                                    .w(px(28.0))
                                    .h(px(26.0))
                                    .tab_stop(focused)
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.remote_action_selection(&copy);
                                            this.remote_copy_selected(window, cx);
                                        },
                                    )),
                                ),
                        )
                    })
                    .when_some(
                        native_drag.filter(|_| crate::platform::file_drag::supported()),
                        |item, drag| {
                            item.on_drag(drag, move |drag, _, window, cx| {
                                let label = weak
                                    .update(cx, |this, _| {
                                        this.remote_browser.selection.drag_started();
                                        let count = this.selected_remote_entries().len();
                                        if count > 1 {
                                            language.format(
                                                Message::RemoteItems,
                                                &[("count", &count.to_string())],
                                            )
                                        } else {
                                            drag.entry.name.clone()
                                        }
                                    })
                                    .unwrap_or_else(|_| drag.entry.name.clone());
                                let preview = cx.new(|_| {
                                    crate::gpui_shell::file_drop::FileDragGhost::new(label)
                                });
                                let owner = weak.clone();
                                let drag = drag.clone();
                                window.defer(cx, move |window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.begin_native_download_drag(drag, window, cx)
                                    });
                                });
                                preview
                            })
                        },
                    )
                    .when_some(directory, |item, directory| {
                        item.drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(highlight))
                            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                                cx.stop_propagation();
                                this.drop_upload_paths(
                                    paths.paths().to_owned(),
                                    Some(directory.clone()),
                                    window,
                                    cx,
                                );
                            }))
                    })
                    .when_some(upload_directory, |item, directory| {
                        item.drag_over::<crate::gpui_shell::file_drop::FileTreeDrag>(
                            move |style, _, _, _| style.bg(highlight),
                        )
                        .on_drop(cx.listener(
                            move |this,
                                  file: &crate::gpui_shell::file_drop::FileTreeDrag,
                                  window,
                                  cx| {
                                cx.stop_propagation();
                                this.drop_upload_paths(
                                    vec![file.local_path.clone()],
                                    Some(directory.clone()),
                                    window,
                                    cx,
                                );
                            },
                        ))
                    })
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.remote_row_press(&press, event, window, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        cx.stop_propagation();
                        let modifiers = event.modifiers();
                        if event.click_count() >= 2
                            && !modifiers.control
                            && !modifiers.platform
                            && !modifiers.shift
                        {
                            this.remote_activate(activate.clone(), true, window, cx);
                        } else {
                            this.remote_browser.selection.click(&activate.path);
                            cx.notify();
                        }
                    })),
            )
            .into_any_element()
    }

    /// 底栏的浏览状态：正在读、读失败、或者目录确实是空的。
    ///
    /// 三种情况必须分开说。"读不到"和"是空的"混为一谈，用户就不知道该重试
    /// 还是该换目录——这是空态里最常见也最误导人的一处偷懒。
    fn remote_notice(&self, language: crate::display::UiLanguage) -> Option<String> {
        if let Some(error) = self.remote_browser.error.as_deref() {
            return Some(language.format(Message::RemoteErrorRetry, &[("error", error)]));
        }
        if self.remote_browser.preflighting {
            return Some(language.text(crate::i18n::Message::TransferChecking).to_owned());
        }
        if let Some(snapshot) = self.remote_transfer_snapshot()
            && snapshot.phase == SftpPhase::Working
            && snapshot.destination != self.remote_browser.destination
        {
            return Some(
                language
                    .format(Message::RemoteBusyOther, &[("destination", &snapshot.destination)]),
            );
        }
        if self.remote_browser.loading {
            return Some(language.text(Message::RemoteReading).to_owned());
        }
        if let Some(outcome) = self.remote_browser.last_outcome {
            return Some(language.text(outcome).to_owned());
        }
        self.remote_browser
            .entries
            .is_empty()
            .then(|| language.text(Message::RemoteEmpty).to_owned())
    }
}

fn remote_button(id: impl Into<gpui::ElementId>, icon: IconName, label: &'static str) -> Button {
    Button::new(id)
        .icon(Icon::new(icon).size(px(16.0)))
        .ghost()
        .small()
        .w(px(32.0))
        .h(px(32.0))
        .px_0()
        .rounded(px(6.0))
        .tooltip(label)
        .aria_label(label)
}

async fn probe_remote_shell_cwd(destination: String) -> crate::ssh_sftp::remote_cwd::RemoteCwd {
    #[cfg(not(test))]
    {
        crate::ssh_sftp::remote_cwd::probe(&destination).await
    }
    #[cfg(test)]
    {
        // GUI regressions own their synthetic sessions. Never authenticate or
        // read host credentials for a connection-wide fallback in a unit test.
        let _ = destination;
        crate::ssh_sftp::remote_cwd::RemoteCwd::Ambiguous
    }
}

fn local_metadata_matches(metadata: &std::fs::Metadata, remote: &SftpEntry) -> bool {
    remote.kind == SftpEntryKind::File
        && metadata.is_file()
        && metadata.len() == remote.size
        && remote.modified != 0
        && metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|elapsed| elapsed.as_secs() == remote.modified)
}

fn format_transfer_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    for (index, unit) in UNITS.iter().enumerate() {
        if value < 1024.0 || index == UNITS.len() - 1 {
            return if value >= 10.0 {
                format!("{value:.0} {unit}")
            } else {
                format!("{value:.1} {unit}")
            };
        }
        value /= 1024.0;
    }
    unreachable!()
}

/// 把一次网络 runtime 上的调用桥到 GPUI 的执行器。
///
/// 返回 `None` 表示网络 runtime 起不来或任务被丢弃——调用方据此报"连接不
/// 可用"，而不是把它和"远端答了个错误"混为一谈。
pub(super) async fn remote_call<T, F, Fut>(work: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T> + Send,
{
    let runtime = crate::ssh_session::runtime().ok()?;
    let (mut tx, rx) = tokio::sync::oneshot::channel();
    runtime.spawn(async move {
        tokio::select! {
            biased;
            _ = tx.closed() => {},
            result = work() => { let _ = tx.send(result); },
        }
    });
    #[cfg(all(test, feature = "gpui-test-support"))]
    if follow_tests::NetworkPump::enabled() {
        return follow_tests::NetworkPump::receive(rx).await;
    }
    rx.await.ok()
}

#[cfg(test)]
mod tests {
    use super::{RemoteBrowser, SftpEntry, SftpEntryKind, format_transfer_bytes};

    fn entry(name: &str, path: &str) -> SftpEntry {
        SftpEntry {
            name: name.to_owned(),
            path: path.to_owned(),
            kind: SftpEntryKind::File,
            size: 12,
            modified: 34,
            permissions: "rw-r--r--".to_owned(),
            is_parent: false,
        }
    }

    #[test]
    fn dropped_remote_call_cancels_its_network_work() {
        struct SignalDrop(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for SignalDrop {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        crate::ssh_session::runtime().unwrap().block_on(async {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
            let caller = tokio::spawn(super::remote_call(move || async move {
                let _signal = SignalDrop(Some(dropped_tx));
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            }));
            tokio::time::timeout(std::time::Duration::from_secs(5), started_rx)
                .await
                .unwrap()
                .unwrap();
            caller.abort();
            let _ = caller.await;
            tokio::time::timeout(std::time::Duration::from_secs(5), dropped_rx)
                .await
                .unwrap()
                .unwrap();
        });
    }

    #[test]
    fn switching_remote_panes_restores_listing_selection_and_scroll_owner_without_reload() {
        let mut browser = RemoteBrowser::default();

        assert!(!browser.bind(11, "alice@alpha".to_owned()));
        browser.path = "/srv/alpha".to_owned();
        browser.entries = vec![entry("alpha.txt", "/srv/alpha/alpha.txt")];
        browser.selection.select_only("/srv/alpha/alpha.txt");

        assert!(!browser.bind(22, "bob@beta".to_owned()));
        browser.path = "/home/bob".to_owned();
        browser.entries = vec![entry("beta.txt", "/home/bob/beta.txt")];
        assert_eq!(browser.scrolls.len(), 2, "每个 pane 必须持有独立滚动句柄");

        assert!(browser.bind(11, "alice@alpha".to_owned()));
        assert_eq!(browser.path, "/srv/alpha");
        assert_eq!(browser.entries, vec![entry("alpha.txt", "/srv/alpha/alpha.txt")]);
        assert!(browser.selection.contains("/srv/alpha/alpha.txt"));
        assert_eq!(browser.path_for(11, "alice@alpha"), Some("/srv/alpha"));
        assert!(!browser.loading);
    }

    #[test]
    fn changing_a_panes_destination_discards_the_old_host_snapshot() {
        let mut browser = RemoteBrowser::default();
        assert!(!browser.bind(7, "root@old-host".to_owned()));
        browser.path = "/root".to_owned();
        browser.entries = vec![entry("old", "/root/old")];

        assert!(!browser.bind(7, "root@new-host".to_owned()));
        assert!(browser.path.is_empty());
        assert!(browser.entries.is_empty());
        assert_eq!(browser.selection.focus(), None);
        assert_eq!(browser.path_for(7, "root@old-host"), None);
    }

    #[test]
    fn transfer_byte_counts_use_binary_units() {
        assert_eq!(format_transfer_bytes(0), "0 B");
        assert_eq!(format_transfer_bytes(1024), "1.0 KiB");
        assert_eq!(format_transfer_bytes(10 * 1024), "10 KiB");
        assert_eq!(format_transfer_bytes(3 * 1024 * 1024), "3.0 MiB");
    }
}
