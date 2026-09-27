//! Workspace snapshot capture and cold restoration.

use super::*;

impl NebulaWorkspace {
    pub(super) fn restore_update_session(
        &mut self,
        session: &crate::session::Session,
        resume_ai: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut restored = false;
        for tab in &session.tabs {
            restored |= self.restore_tab(tab, resume_ai, window, cx);
        }
        if restored {
            self.active = session.active_tab.min(self.tabs.len().saturating_sub(1));
            self.focus_active(window, cx);
        }
        restored
    }

    /// 启动恢复：断路器跳闸就隔离现场并走干净路径；恢复成功弹一条
    /// 自动消失的提示（崩溃现场多一句来源说明）。返回是否恢复出了 tab。
    pub(super) fn try_restore_session(
        &mut self,
        resume_ai: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::display::ToastKind;

        let Some(mut session) = crate::session::load() else { return false };
        if !crate::session::should_restore(&session) {
            if !session.tabs.is_empty() {
                // 连续几次启动都没活到第一次自动保存：把「一恢复就崩」的
                // 现场挪去隔离文件（唯一的诊断材料），本次干净启动。
                if let Some(path) = crate::session::quarantine() {
                    crate::gpui_shell::toast::banner(
                        window,
                        cx,
                        ToastKind::Warning,
                        format!("连续多次启动未完成恢复，已跳过；现场保存在 {}", path.display()),
                    );
                }
            }
            return false;
        }
        let crashed = crate::session::was_crash(&session);
        crate::session::mark_boot_attempt(&mut session);
        let mut restored = 0usize;
        for tab in &session.tabs {
            if self.restore_tab(tab, resume_ai, window, cx) {
                restored += 1;
            }
        }
        if restored == 0 {
            return false;
        }
        self.active = session.active_tab.min(self.tabs.len().saturating_sub(1));
        self.focus_active(window, cx);
        let text = if crashed {
            format!("上次未正常退出，已恢复 {restored} 个标签")
        } else {
            format!("已恢复 {restored} 个标签")
        };
        crate::gpui_shell::toast::toast(window, cx, ToastKind::Success, text);
        cx.notify();
        true
    }

    /// 按保存的逐 pane 启动环境重建分屏树。旧文件缺少逐 pane 描述时，
    /// 首 pane 沿用 tab launch，其余 pane 保持旧格式的默认 shell 回退。
    pub(super) fn restore_tab(
        &mut self,
        tab: &crate::session::TabSession,
        resume_ai: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::session::{LaunchSession, LayoutSession};

        let layout = tab.layout.clone().unwrap_or(LayoutSession::Pane {
            launch: None,
            cwd: tab.cwd.clone(),
            agent: None,
        });
        // v1-v3 / 早期 GPUI 快照没有 launch，按共享 schema 回退 Default；
        // v4 的 Shell/Profile/Ssh 必须原样用于首 Pane，不能再次读取当前默认。
        let saved_launch = tab.launch.clone().unwrap_or(LaunchSession::Default);
        let grid = self.initial_grid;
        let mut panes: Vec<TerminalPane> = Vec::new();
        for (index, leaf) in layout.leaves().into_iter().enumerate() {
            let LayoutSession::Pane { cwd, agent, launch } = leaf else { continue };
            let mut launch_session = launch.clone().unwrap_or_else(|| {
                if index == 0 {
                    saved_launch.clone()
                } else {
                    super::shell_launch::configured_local_launch(cx)
                }
            });
            if matches!(launch_session, LaunchSession::Default) {
                launch_session = super::shell_launch::configured_local_launch(cx);
            }
            let guest_directory =
                tab_duplication::inherit_guest_directory(&mut launch_session, cwd);
            let local_cwd = if guest_directory { None } else { crate::session::valid_dir(cwd) };
            let launch = Self::terminal_launch_from_session(&launch_session, local_cwd);
            let pane = self.new_pane(grid, launch, None, window, cx);
            if !matches!(launch_session, LaunchSession::Default) {
                pane.view.update(cx, |view, _| view.session_launch = launch_session);
            }
            // Display and persist the restored location while the guest shell
            // starts; subsequent OSC reports remain authoritative.
            if guest_directory {
                pane.view.update(cx, |view, cx| {
                    view.seed_restored_cwd(cwd.clone(), cx);
                });
            }
            if resume_ai && let Some(agent) = agent {
                pane.view.update(cx, |view, cx| view.restore_agent(agent.clone(), cx));
            }
            panes.push(pane);
        }
        if panes.is_empty() {
            return false;
        }
        let mut ids = panes.iter().map(|pane| pane.id).collect::<Vec<_>>().into_iter();
        let (tree, _) = crate::gpui_shell::session_restore::tree_from_layout(&layout, &mut || {
            ids.next().unwrap_or(0)
        });
        let focused =
            panes.get(tab.active_pane).or_else(|| panes.first()).map(|pane| pane.id).unwrap_or(0);
        // 恢复期保持文件里的既有次序，不套「新标签插入位置」策略。
        // 重命名与色标随会话一起回来（旧壳同合同）。
        let at = self.tabs.len();
        self.insert_tab_at(
            at,
            WorkspaceTab::Terminal { panes, tree, focused, zoomed: false, broadcast: false },
            TabMeta {
                custom_name: tab.custom_name.clone(),
                color: tab.color,
                shell_tag: Self::launch_shell_tag(&saved_launch),
                launch: Some(saved_launch),
                has_bell: false,
            },
        );
        true
    }

    /// 当前工作区 → 共享 v4 快照。设置/文档/图片 tab 不进会话（旧壳同
    /// 合同）；AI 会话身份优先取 hook 直报的精确 id，退而取可解析的前台
    /// 程序名（claude 无 id 恢复成 `--continue`，安全判定在 schema 层）。
    ///
    /// 快照一律带 `boot_attempts = 0`：断路器只回答「这次启动活到了第一次
    /// 自动保存没有」（session.rs 模块合同）。agent 恢复目标尚未被 hook /
    /// 探针确认是另一回事——codex 停在 hooks 信任、目录选择或「已在别处
    /// 打开」的提示上可以是几小时，正常退出照样算一次失败的话，三次就把
    /// 整个工作区（含所有普通 tab）隔离掉了。未确认的目标本身仍随
    /// `session_agent()` 落盘，下次启动照旧接续。
    pub(crate) fn snapshot_session(&self, cx: &App) -> crate::session::Session {
        use crate::session::{AgentSession, LaunchSession, Session, TabSession};

        let mut tabs = Vec::new();
        let mut active_out = 0usize;
        for (ix, tab) in self.tabs.iter().enumerate() {
            let WorkspaceTab::Terminal { panes, tree, focused, .. } = tab else { continue };
            if ix == self.active {
                active_out = tabs.len();
            }
            let leaf_data = |id: u64| -> (String, Option<AgentSession>, Option<LaunchSession>) {
                let Some(pane) = panes.iter().find(|pane| pane.id == id) else {
                    return (String::new(), None, None);
                };
                let view = pane.view.read(cx);
                let agent = view.session_agent();
                (view.cwd.clone(), agent, Some(view.session_launch.clone()))
            };
            let layout = crate::gpui_shell::session_restore::layout_from_tree(tree, &leaf_data);
            let cwd = panes
                .iter()
                .find(|pane| pane.id == *focused)
                .map(|pane| pane.view.read(cx).cwd.clone())
                .unwrap_or_default();
            let meta = self.meta(ix);
            let first_leaf = tree.first_leaf();
            let launch = meta.launch.clone().unwrap_or_else(|| {
                // 兼容本次修复前已经在内存中的 Tab：SSH 仍可从首 Pane 取回；
                // 旧本地 Tab 已经没有身份信息，只能诚实落为 Default。
                panes
                    .iter()
                    .find(|pane| pane.id == first_leaf)
                    .and_then(|pane| pane.view.read(cx).ssh_destination.clone())
                    .map(|host| LaunchSession::Ssh { host })
                    .unwrap_or(LaunchSession::Default)
            });
            let active_pane = tree.leaves().iter().position(|id| id == focused).unwrap_or(0);
            tabs.push(TabSession {
                cwd,
                custom_name: meta.custom_name,
                color: meta.color,
                launch: Some(launch),
                layout: Some(layout),
                active_pane,
            });
        }
        Session::new(active_out, tabs)
    }
}
