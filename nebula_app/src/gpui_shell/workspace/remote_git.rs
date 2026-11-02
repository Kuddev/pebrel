//! 远端 Git 面板：抽屉 Git 视图在 SSH pane 上的形态。
//!
//! 聚焦远端 pane 时，Git 视图此前渲染的是**本地** `side_panel` 的快照——
//! 上一个本地目录的仓库状态，既不跟随也误导。远端形态把状态与写操作都
//! 放到远端执行：状态用 `git status --porcelain=v2 --branch -z` 一条命令
//! 拿全（分支、ahead/behind、变更列表），解析复用
//! [`crate::git_worktree::status::parse_status`]，分组判据与本地
//! `collect_git_info` 一致；写操作（stage/unstage/discard/commit/pull/push）
//! 与本地 [`crate::display::side_panel::SidePanel`] 同一命令面，经同一
//! exec 通道跑在远端仓库上。merge 编辑器与 SVN 入口是本地专属，远端不画。
//!
//! # 仓库根从哪来
//!
//! 集成上报的 `pebrel_git_root`（UserVar）优先，且必须仍覆盖 cwd
//! （[`git_root_covers`]：用户切进没有集成的嵌套
//! shell——fish 等——后 UserVar 静默，陈旧的根会把面板钉在已离开的项目）。
//! 集成哑掉时由**项目身份探测**（`NebulaWorkspace::refresh_remote_project`）
//! 补位：在
//! cwd 变化沿上对远端跑一次与集成同款的 `git rev-parse`，结果按 pane 缓存
//! （[`RemoteProject`]），侧栏分组键、分支 chip 与面板根三处共用。都没有时
//! 用远端 cwd（OSC 7）——后者可能在仓库外，`git status` 会以退出码 128
//! 收场，那是目录身份的正常态，走「不在仓库」空态而不是错误行。
//!
//! # 异步回界
//!
//! 与远端文件浏览器同一模式：exec 跑在网络 runtime，oneshot 桥回 GPUI
//! 执行器（[`super::remote_files::remote_call`]），世代号 + pane 双校验
//! 丢弃过期响应。root 跟随终端变化重拉，在途目标记 `requested_root`
//! 去重，避免每帧重发。写操作收尾后**无论成败**都重拉状态——失败的
//! pull 也可能改写工作副本，旧快照比没快照更误导。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, IntoElement as _, ParentElement as _, SharedString, Styled as _, Window, div, px,
};

use super::remote_files::remote_call;
use super::*;
use crate::ai_hook::remote::quote;
use crate::display::side_panel::{GitInfo, GitPanelView, VcsKind};
use crate::i18n::Message;

/// `git status` 的时间预算。大仓库首次 status 超 3 秒并不罕见（探测脚本
/// 的 3 秒预算在这里太紧），但面板有加载态，等满 8 秒再报错也不算失礼。
const STATUS_BUDGET: Duration = Duration::from_secs(8);

/// 写操作的时间预算。push/pull 走网络，慢链路上几十秒是常态；命令没有
/// 进度回流，预算内界面用转圈表达「在跑」，超时才报错。
const OP_BUDGET: Duration = Duration::from_secs(30);

/// git 根是否仍覆盖当前 cwd：远端集成上报的根只在覆盖时可信，切出仓库
/// 即失效。面板根与探测根补位决策共用这一判定（侧栏分组键在 fork 线的
/// task_groups_ssh 里保持同一语义）。
fn git_root_covers(root: &str, cwd: &str) -> bool {
    let root = root.trim();
    if !root.starts_with('/') {
        return false;
    }
    let cwd = cwd.trim();
    cwd.is_empty() || cwd == root || cwd.starts_with(&format!("{root}/"))
}

/// 项目身份探测（`git rev-parse`，单叉读操作）的时间预算。
const PROBE_BUDGET: Duration = Duration::from_secs(3);

/// 单个 SSH pane 的远端项目身份：`git rev-parse --abbrev-ref HEAD
/// --show-toplevel` 一次探测的结果（与远端集成同一条命令、同一输出契约）。
/// 集成在线时不需要它——集成上报的根更权威也更及时；它只在集成哑掉
/// （fish 等无集成的嵌套 shell）时补位，供左侧分组键、分支 chip 与本
/// 面板的根决策共用：左右两侧读同一份身份。
pub(super) struct RemoteProject {
    pub(super) destination: String,
    /// 仓库根；空 = 探测过，不在仓库（或远端无 git）。
    pub(super) root: String,
    pub(super) branch: String,
    /// 本次结果对应的 cwd（变化沿判据：同一目录或仍在根下时不重探）。
    pub(super) cwd: String,
}

/// 远端 Git 面板的状态。单个当前视图：切 pane 时重置，不按 pane 留快照
/// ——Git 状态本就要刷新，快照只会制造「切回来看到旧状态」的误导。
#[derive(Default)]
pub(super) struct RemoteGit {
    /// 当前绑定的 pane；`None` 表示此刻没有远端 Git 视图在跑。
    pane: Option<u64>,
    destination: String,
    /// 当前拉取（或已显示）的仓库根，空 = 无目录身份。
    root: String,
    snapshot: Option<Arc<GitInfo>>,
    loading: bool,
    /// 写操作在途（stage/commit/push…），期间其余操作与刷新按钮禁用。
    op_running: bool,
    error: Option<String>,
    /// 世代号：晚到响应的作废凭据（与 RemoteBrowser 同一合同）。
    generation: u64,
    /// 已发出的在途 root，跟随去重用。
    requested_root: Option<String>,
    /// 点击沿（开抽屉/切到 Git 视图）请求的强制刷新，由
    /// [`NebulaWorkspace::route_remote_git`] 消费（与本地 `needs_refresh`
    /// 同构：事件只置位，路由统一发命令）。
    pub(super) refresh_requested: bool,
    /// 每个 SSH pane 的项目身份探测缓存（侧栏分组对所有 SSH pane 可见，
    /// 不随抽屉聚焦切换——只在 pane 关闭时清）。
    projects: HashMap<u64, RemoteProject>,
    /// 探测在途的 pane，防同一 pane 并发重复发。
    probing: HashSet<u64>,
}

impl RemoteGit {
    /// pane 关闭时清理；只在还绑着这个 pane 时动状态（与
    /// `RemoteBrowser::forget` 同一合同）。
    pub(super) fn forget(&mut self, pane: u64) {
        self.projects.remove(&pane);
        self.probing.remove(&pane);
        if self.pane == Some(pane) {
            self.reset();
        }
    }

    /// pane 的远端项目身份（分组键/分支 chip/面板根共用）。非 SSH pane 或
    /// 尚未探测时为 `None`。
    pub(super) fn project(&self, pane: u64) -> Option<&RemoteProject> {
        self.projects.get(&pane)
    }

    /// 已缓存的探测身份是否仍覆盖当前 cwd（同一目录，或仍在探测出的根下
    /// ——与集成的变化沿语义一致，cd 不出仓库不重探）。
    fn project_covers(&self, pane: u64, destination: &str, cwd: &str) -> bool {
        self.projects.get(&pane).is_some_and(|project| {
            project.destination == destination
                && (project.cwd == cwd || git_root_covers(&project.root, cwd))
        })
    }

    fn reset(&mut self) {
        self.pane = None;
        self.destination.clear();
        self.root.clear();
        self.snapshot = None;
        self.loading = false;
        self.op_running = false;
        self.error = None;
        self.requested_root = None;
        // pane/destination 变化路径本就触发 root 重拉，待处理请求无残留必要。
        self.refresh_requested = false;
        self.generation = self.generation.wrapping_add(1);
    }

    /// 点击沿（开抽屉/切到 Git 视图）请求一次强制刷新：幂等置位，由
    /// `route_remote_git` 在空闲（无在途拉取/写操作）时消费。在途期间置位
    /// 不丢失——落地后下一帧补拉，恰好覆盖「在途时 agent 又改了文件」。
    pub(super) fn request_refresh(&mut self) {
        self.refresh_requested = true;
    }

    /// 变更行双击的远端文档目标。root 以字符串形态保真拼接——`PathBuf`
    /// 的 join 在 Windows 上会把分隔符本地化，远端路径必须原样带 `/`。
    pub(super) fn document_target(&self, relative_path: &str) -> Option<(String, String)> {
        if self.destination.is_empty() || self.root.is_empty() {
            return None;
        }
        Some((self.destination.clone(), format!("{}/{relative_path}", self.root)))
    }
}

/// 面板根的决策（与侧栏分组键同一优先序，fork 线的 task_groups_ssh 共用此判定）：
/// 集成上报的根（须仍覆盖 cwd，嵌套无集成 shell 里 UserVar 静默——见
/// [`git_root_covers`]）→ 探测根（`rev-parse` 补位源，
/// 由 cwd 在途探测异步回填）→ 远端 cwd；都缺席才无身份。探测根顺带修掉
/// cwd 兜底锚在仓库**子目录**的错位：porcelain 路径相对仓库根，锚点必须
/// 是真根。
fn remote_git_root(
    git_root: Option<&str>,
    cwd: Option<&str>,
    project: Option<&RemoteProject>,
) -> Option<String> {
    let cwd = cwd.map(str::trim);
    let cwd_text = cwd.unwrap_or("");
    // 探测身份有效 = 探的就是这个目录，或 cwd 仍在探测出的根下（变化沿）；
    // 根为空（探测过不在仓库）时落回 cwd，让 status 128 自己说明目录身份。
    let probed = project
        .filter(|project| project.cwd == cwd_text || git_root_covers(&project.root, cwd_text))
        .and_then(|project| {
            let root = project.root.trim();
            (!root.is_empty()).then_some(root)
        });
    git_root
        .filter(|root| git_root_covers(root, cwd.unwrap_or("")))
        .or(probed)
        .or(cwd)
        .filter(|root| root.starts_with('/'))
        .map(|root| root.to_owned())
}

/// porcelain v2 输出 → 面板快照。分组判据与本地 `collect_git_info` 相同：
/// 冲突路径三处都保留（显示层单独分组并从另两组过滤重复）；行级增删统计
/// 与历史图需要额外的远端往返，远端形态不拉。
fn remote_git_info(output: &str) -> Result<GitInfo, &'static str> {
    let status = crate::git_worktree::status::parse_status(output)?;
    let mut info = GitInfo {
        vcs: VcsKind::Git,
        branch: status.branch,
        ahead: status.ahead,
        ..GitInfo::default()
    };
    for entry in status.entries {
        if entry.conflict {
            info.conflicts.push(('U', entry.path.clone()));
        }
        if entry.staged() {
            info.staged.push((entry.index, entry.path.clone()));
        }
        if entry.unstaged() {
            info.unstaged.push((entry.worktree, entry.path));
        }
    }
    Ok(info)
}

/// 远端 git 命令行：`git -C <root> --no-optional-locks <args…>`。每个参数
/// 单引号包裹（与远端集成 bootstrap 同一转义规则，路径与提交信息里的空格、
/// 引号、`$` 都安全）；`--no-optional-locks` 与本地写操作同款——不抢
/// index.lock，不惊动远端可能开着的 IDE。
fn remote_git_command(root: &str, args: &[&str]) -> String {
    let mut command = format!("git -C {} --no-optional-locks", quote(root));
    for arg in args {
        command.push(' ');
        command.push_str(&quote(arg));
    }
    command
}

/// 单条 `rev-parse` 输出的首个非空行。
fn probe_line(stdout: &str) -> Option<&str> {
    stdout.lines().map(str::trim).find(|line| !line.is_empty())
}

/// 单条探测的根字段：必须是 POSIX 绝对路径，其余形态（失败输出、欢迎
/// 噪声、Windows 盘符）一律无身份。
fn probe_root(stdout: &str) -> Option<String> {
    probe_line(stdout).filter(|line| line.starts_with('/')).map(str::to_owned)
}

/// 单条探测的分支字段：**不允许长得像绝对路径**——远端 git/wrapper 对
/// 组合参数的输出次序不可假设，上游把根塞进分支槽时宁可空着也不错挂
/// （用户 10-15 现场正是分支槽里出现路径）。
fn probe_branch(stdout: &str) -> Option<String> {
    probe_line(stdout).filter(|line| !line.starts_with('/')).map(str::to_owned)
}

impl NebulaWorkspace {
    /// 聚焦 pane 上报的远端仓库根（`pebrel_git_root` UserVar）。与
    /// [`Self::focused_remote_pane`] 同一 ready 门控：连接没就绪时不给身份。
    fn focused_remote_git_root(&self, cx: &App) -> Option<String> {
        let view = self.tabs.get(self.active).and_then(super::WorkspaceTab::focused_view)?;
        let view = view.read(cx);
        view.ready_ssh_destination()?;
        let root = view.git_root.trim();
        (!root.is_empty()).then(|| root.to_owned())
    }

    /// 远端 Git 视图的每帧路由：聚焦身份变化重置，仓库根变化重拉，点击沿
    /// （开抽屉/切到 Git 视图，`refresh_requested`）强制重拉。render 路径发
    /// 请求的先例是 `attach_remote_browser`——纯内存比对通过后才动网络，
    /// 不会每帧重发。
    fn route_remote_git(&mut self, cx: &mut Context<'_, Self>) {
        let Some((pane, destination, cwd)) = self.focused_remote_pane(cx) else {
            if self.remote_git.pane.is_some() {
                self.remote_git.reset();
                cx.notify();
            }
            return;
        };
        if self.remote_git.pane != Some(pane) || self.remote_git.destination != destination {
            self.remote_git.reset();
            self.remote_git.pane = Some(pane);
            self.remote_git.destination = destination.clone();
        }
        let git_root = self.focused_remote_git_root(cx);
        let project =
            self.remote_git.project(pane).filter(|project| project.destination == destination);
        let Some(root) = remote_git_root(git_root.as_deref(), cwd.as_deref(), project) else {
            // 无目录身份（集成缺位且 cwd 未上报）：空态，不残留旧主机的快照。
            if !self.remote_git.root.is_empty() || self.remote_git.snapshot.is_some() {
                self.remote_git.root.clear();
                self.remote_git.snapshot = None;
                self.remote_git.error = None;
                cx.notify();
            }
            return;
        };
        if self.remote_git.root != root
            && self.remote_git.requested_root.as_deref() != Some(root.as_str())
        {
            self.remote_git.root = root.clone();
            // root 变化的重拉已覆盖本次点击请求，不吃掉也不叠加第二条命令。
            self.remote_git.refresh_requested = false;
            self.refresh_remote_git(pane, destination, root, cx);
        } else if self.remote_git.refresh_requested
            && !self.remote_git.loading
            && !self.remote_git.op_running
        {
            // 点击沿的强制刷新（开抽屉/切到 Git 视图）：root 未变也要重拉
            // ——同一 cwd 下 agent 改文件不会动 root，旧快照会一直显示到
            // 切 pane（用户 10-19 报障）。等待在途拉取/写操作收尾后再发。
            self.remote_git.refresh_requested = false;
            self.refresh_remote_git(pane, destination, root, cx);
        }
    }

    /// 抽屉打开且停在 Git 视图时，请求一次远端 Git 面板的强制刷新（用户
    /// 10-19 报障：agent 在同一 cwd 下改文件不动 root，`route_remote_git`
    /// 的 root 变化判据失灵，旧快照一直显示到切 pane）。与本地
    /// `SidePanel::toggle` 置 `needs_refresh` 同构：事件路径只置位，由
    /// `route_remote_git` 统一消费（空闲时发命令，在途时等待）。
    pub(super) fn wake_remote_git_refresh(&mut self) {
        if self.side_panel.open
            && self.side_panel.view == crate::display::side_panel::PanelView::Git
        {
            self.remote_git.request_refresh();
        }
    }

    /// 事件路径（TitleChanged）：SSH pane 的远端项目身份探测。
    ///
    /// 集成上报的根仍覆盖 cwd 时不发——集成权威且零流量，探测只在集成哑掉
    /// （fish 等无集成嵌套 shell，UserVar/标题静默而 OSC 7 照发）时的**变化
    /// 沿**上补位：cwd 移出已知根才发一次，命令与集成同一条（`rev-parse
    /// --abbrev-ref HEAD --show-toplevel`，单叉、同输出契约），结果按 pane
    /// 缓存，供分组键、分支 chip 与面板根共用——左右两侧同一个源。在途去重
    /// 按 pane；完成后若 cwd 又移走，下一次 TitleChanged 自然再探。
    pub(super) fn refresh_remote_project(
        &mut self,
        view: &gpui::Entity<crate::gpui_shell::terminal::view::TerminalView>,
        cx: &mut Context<'_, Self>,
    ) {
        let (pane, destination, cwd, git_root) = {
            let view = view.read(cx);
            let Some(destination) = view.ready_ssh_destination() else { return };
            (
                view.pane_id,
                destination.to_owned(),
                view.cwd.trim().to_owned(),
                view.git_root.trim().to_owned(),
            )
        };
        if cwd.is_empty()
            || git_root_covers(&git_root, &cwd)
            || self.remote_git.project_covers(pane, &destination, &cwd)
            || self.remote_git.probing.contains(&pane)
        {
            return;
        }
        // 两条**单目的** exec（分支、根各一条），复用与 status 相同的命令
        // 构造器——那是真实主机上验证过的引号与参数形态；不引入任何额外
        // shell 语法（`sh -c`/`$()`/printf 转义都不要：上一版标记行方案在
        // 用户主机上整条死掉，chip 与面板双双落空）。
        let branch_command = remote_git_command(&cwd, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let root_command = remote_git_command(&cwd, &["rev-parse", "--show-toplevel"]);
        self.remote_git.probing.insert(pane);
        cx.spawn(async move |this, cx| {
            let branch_destination = destination.clone();
            let branch = remote_call(|| async move {
                crate::ssh_session::exec_capture(
                    &branch_destination,
                    &branch_command,
                    b"",
                    PROBE_BUDGET,
                )
                .await
            })
            .await
            .and_then(|result| result.ok())
            .and_then(|stdout| probe_branch(&stdout));
            let root_destination = destination.clone();
            let root = remote_call(|| async move {
                crate::ssh_session::exec_capture(
                    &root_destination,
                    &root_command,
                    b"",
                    PROBE_BUDGET,
                )
                .await
            })
            .await
            .and_then(|result| result.ok())
            .and_then(|stdout| probe_root(&stdout));
            let _ = this.update(cx, |workspace, cx| {
                workspace.remote_git.probing.remove(&pane);
                // 任一字段缺席都按无身份记录（`None` → 空串）：失败与「不在
                // 仓库」不区分，变化沿判据靠它不重发。
                workspace.remote_git.projects.insert(
                    pane,
                    RemoteProject {
                        destination,
                        root: root.unwrap_or_default(),
                        branch: branch.unwrap_or_default(),
                        cwd,
                    },
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// 在远端跑一次 `git status` 并落快照。
    fn refresh_remote_git(
        &mut self,
        pane: u64,
        destination: String,
        root: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.remote_git.loading = true;
        self.remote_git.error = None;
        self.remote_git.requested_root = Some(root.clone());
        self.remote_git.generation = self.remote_git.generation.wrapping_add(1);
        let generation = self.remote_git.generation;
        cx.notify();

        let command = format!(
            "git -C {} status --porcelain=v2 --branch -z",
            crate::ai_hook::remote::quote(&root)
        );
        cx.spawn(async move |this, cx| {
            let output = remote_call(|| async move {
                crate::ssh_session::exec_capture(&destination, &command, b"", STATUS_BUDGET).await
            })
            .await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace.remote_git.generation != generation
                    || workspace.remote_git.pane != Some(pane)
                {
                    return;
                }
                workspace.remote_git.loading = false;
                if workspace.remote_git.requested_root.as_deref() == Some(root.as_str()) {
                    workspace.remote_git.requested_root = None;
                }
                match output {
                    Some(Ok(stdout)) => match remote_git_info(&stdout) {
                        Ok(info) => {
                            workspace.remote_git.snapshot = Some(Arc::new(info));
                            workspace.remote_git.error = None;
                        },
                        Err(_) => {
                            workspace.remote_git.snapshot = None;
                            workspace.remote_git.error =
                                Some("远端 Git 状态输出无法解析".to_owned());
                        },
                    },
                    Some(Err(err)) => {
                        // 退出码 128 基本就是「不在仓库 / 裸仓库」——目录身份的
                        // 正常态，走空态文案，不当错误渲染。
                        if err.to_string().contains("status 128") {
                            workspace.remote_git.snapshot = None;
                            workspace.remote_git.error = None;
                        } else {
                            workspace.remote_git.error = Some(err.to_string());
                        }
                    },
                    None => {
                        workspace.remote_git.error = Some("远端连接不可用，请稍后重试".to_owned());
                    },
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 在远端仓库上执行一次写操作，收尾后无论成败都重拉状态。
    fn run_remote_git_op(&mut self, args: &[&str], cx: &mut Context<'_, Self>) {
        let Some(pane) = self.remote_git.pane else { return };
        if self.remote_git.op_running
            || self.remote_git.root.is_empty()
            || self.remote_git.destination.is_empty()
        {
            return;
        }
        let destination = self.remote_git.destination.clone();
        let root = self.remote_git.root.clone();
        let command = remote_git_command(&root, args);
        self.remote_git.op_running = true;
        self.remote_git.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = remote_call(|| async move {
                crate::ssh_session::exec_capture(&destination, &command, b"", OP_BUDGET).await
            })
            .await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace.remote_git.pane != Some(pane) {
                    return;
                }
                workspace.remote_git.op_running = false;
                match result {
                    Some(Ok(_)) => workspace.remote_git.error = None,
                    Some(Err(err)) => workspace.remote_git.error = Some(err.to_string()),
                    None => {
                        workspace.remote_git.error = Some("远端连接不可用，请稍后重试".to_owned());
                    },
                }
                // 失败的操作也可能改写工作副本（半途的 pull、冲突的 push），
                // 旧快照比没快照更误导，重拉不看成败。
                let destination = workspace.remote_git.destination.clone();
                let root = workspace.remote_git.root.clone();
                if !root.is_empty() {
                    workspace.refresh_remote_git(pane, destination, root, cx);
                }
            });
        })
        .detach();
    }

    /// `git add -- <path>`：行内 ＋。参数是 porcelain 的仓库相对路径。
    pub(super) fn remote_git_stage(&mut self, path: &str, cx: &mut Context<'_, Self>) {
        self.run_remote_git_op(&["add", "--", path], cx);
    }

    /// `git restore --staged -- <path>`：行内 −。
    pub(super) fn remote_git_unstage(&mut self, path: &str, cx: &mut Context<'_, Self>) {
        self.run_remote_git_op(&["restore", "--staged", "--", path], cx);
    }

    /// `git restore -- <path>`：丢弃工作区改动（确认交互在行内，同本地）。
    pub(super) fn remote_git_discard(&mut self, path: &str, cx: &mut Context<'_, Self>) {
        self.run_remote_git_op(&["restore", "--", path], cx);
    }

    /// `git add -A`：全部暂存。
    fn remote_git_stage_all(&mut self, cx: &mut Context<'_, Self>) {
        self.run_remote_git_op(&["add", "-A"], cx);
    }

    /// `git commit -m <msg>`：提交暂存区。
    fn remote_git_commit(&mut self, message: &str, cx: &mut Context<'_, Self>) {
        self.run_remote_git_op(&["commit", "-m", message], cx);
    }

    /// `git push`：发布 ahead 的提交。
    fn remote_git_push(&mut self, cx: &mut Context<'_, Self>) {
        self.run_remote_git_op(&["push"], cx);
    }

    /// `git pull --ff-only`：只快进，不制造隐式合并提交（与本地同款）。
    fn remote_git_pull(&mut self, cx: &mut Context<'_, Self>) {
        self.run_remote_git_op(&["pull", "--ff-only"], cx);
    }

    /// 远端提交输入框的提交动作：读共享草稿、发命令、清空（与本地
    /// `submit_vcs_commit` 同一交互）。
    fn submit_remote_vcs_commit(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let message = self.git_commit_input.input.read(cx).value().trim().to_owned();
        if message.is_empty() {
            return;
        }
        self.remote_git_commit(&message, cx);
        self.git_commit_input.input.update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    /// 抽屉 Git 视图的远端形态。
    pub(super) fn render_remote_git(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        self.route_remote_git(cx);
        self.git_commit_input.sync_language(window, cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let destination: SharedString = self.remote_git.destination.clone().into();
        let root = self.remote_git.root.clone();
        let snapshot = self.remote_git.snapshot.clone();
        let loading = self.remote_git.loading;
        let op_running = self.remote_git.op_running;
        let error = self.remote_git.error.clone();
        let has_root = !root.is_empty();
        let no_status = snapshot.is_none() && error.is_none();
        let refresh = has_root && !loading && !op_running;
        let pane = self.remote_git.pane;
        let refresh_destination = self.remote_git.destination.clone();
        let refresh_root = root.clone();

        // 同一个 VcsList 复用给远端：Arc 指针 + root + remote 参与相等
        // 判断，本地/远端交替时列表自会重建。
        self.vcs_list.sync(
            snapshot.clone(),
            GitPanelView::Changes,
            has_root.then(|| PathBuf::from(&root)),
            true,
        );

        let summary = snapshot.as_ref().map(|info| {
            h_flex()
                .h(px(30.0))
                .items_center()
                .gap_2()
                .child(
                    div()
                        .font_family(SharedString::from(crate::font_install::REQUIRED_FONT_FAMILY))
                        .text_sm()
                        .text_color(muted)
                        .child("\u{ea68}"),
                )
                .child(div().flex_1().min_w_0().text_sm().truncate().child(info.branch.clone()))
                .when(info.ahead > 0, |row| {
                    row.child(
                        div().text_xs().text_color(theme.primary).child(format!("↑{}", info.ahead)),
                    )
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.success)
                        .child(format!("+{}", info.unstaged.len() + info.staged.len())),
                )
        });

        // 操作面与本地 Git 视图同构：提交草稿是同一份（本地/远端共享输入
        // 实体），动作按当前形态分发到远端。
        let staged_len = snapshot.as_ref().map_or(0, |info| info.staged.len());
        let unstaged_len = snapshot.as_ref().map_or(0, |info| info.unstaged.len());
        let ahead = snapshot.as_ref().map_or(0, |info| info.ahead);
        let commit_ready = !op_running && staged_len > 0;
        let commit_row = snapshot.is_some().then(|| {
            h_flex()
                .gap_1()
                .items_center()
                .child(div().flex_1().min_w_0().child(Input::new(&self.git_commit_input.input)))
                .child(
                    Button::new("remote-git-commit")
                        .label(language.text(Message::VcsCommit))
                        .small()
                        .disabled(!commit_ready)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.submit_remote_vcs_commit(window, cx);
                        })),
                )
        });
        let action_strip =
            snapshot.is_some().then(|| {
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("remote-git-stage-all")
                            .label(language.text(Message::VcsStageAll))
                            .small()
                            .disabled(op_running || unstaged_len == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.remote_git_stage_all(cx))),
                    )
                    .child(
                        Button::new("remote-git-pull")
                            .label(language.text(Message::VcsPull))
                            .small()
                            .disabled(op_running)
                            .on_click(cx.listener(|this, _, _, cx| this.remote_git_pull(cx))),
                    )
                    .child(
                        Button::new("remote-git-push")
                            .label(if ahead > 0 {
                                SharedString::from(language.format(
                                    Message::VcsPushAhead,
                                    &[("count", &ahead.to_string())],
                                ))
                            } else {
                                SharedString::from(language.text(Message::VcsPush))
                            })
                            .small()
                            .disabled(op_running || ahead == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.remote_git_push(cx))),
                    )
                    .when(op_running, |row| row.child(Spinner::new().xsmall()))
            });

        v_flex()
            .h_full()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .p_2()
            .gap_2()
            .occlude()
            // 主机名单独一行：远端视图最危险的误操作是「以为在另一台机器上」，
            // 与远端文件浏览器同一安全设计。
            .child(
                h_flex().px(px(super::remote_files::TEXT_INSET)).items_center().gap_1().child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.foreground)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(destination),
                ),
            )
            .child(
                div()
                    .px(px(super::remote_files::TEXT_INSET))
                    .text_xs()
                    .text_color(muted)
                    .child(language.text(Message::VcsRemoteRunsOverSsh)),
            )
            .when_some(summary, |panel, summary| panel.child(summary))
            .when_some(error, |panel, error| {
                panel.child(
                    div()
                        .px(px(super::remote_files::TEXT_INSET))
                        .text_xs()
                        .text_color(theme.danger)
                        .child(error),
                )
            })
            .when_some(commit_row, |panel, row| panel.child(row))
            .when_some(action_strip, |panel, row| panel.child(row))
            .when(loading, |panel| {
                panel.child(
                    div()
                        .px(px(super::remote_files::TEXT_INSET))
                        .text_xs()
                        .text_color(muted)
                        .child(language.text(Message::RemoteReading)),
                )
            })
            .when(!has_root && !loading, |panel| {
                panel.child(
                    div()
                        .py_3()
                        .text_sm()
                        .text_color(muted)
                        .child(language.text(Message::VcsNotRepository)),
                )
            })
            .when(has_root && !loading && no_status, |panel| {
                // root 有身份但没快照也没错误：git 以 128 收场的「不在仓库」。
                panel.child(
                    div()
                        .py_3()
                        .text_sm()
                        .text_color(muted)
                        .child(language.text(Message::VcsNotRepository)),
                )
            })
            .child(self.render_vcs_list(cx))
            .child(
                h_flex().justify_end().gap_1().child(
                    Button::new("remote-git-refresh")
                        .icon(IconName::Redo2)
                        .ghost()
                        .xsmall()
                        .disabled(!refresh)
                        .tooltip(language.format(Message::VcsRefreshStatus, &[("vcs", "Git")]))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(pane) = pane
                                && !refresh_root.is_empty()
                            {
                                this.refresh_remote_git(
                                    pane,
                                    refresh_destination.clone(),
                                    refresh_root.clone(),
                                    cx,
                                );
                            }
                        })),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(destination: &str, root: &str, branch: &str, cwd: &str) -> RemoteProject {
        RemoteProject {
            destination: destination.to_owned(),
            root: root.to_owned(),
            branch: branch.to_owned(),
            cwd: cwd.to_owned(),
        }
    }

    /// 点击沿刷新标志的生命周期：置位幂等，`reset` 清位（pane/destination
    /// 变化路径的 root 重拉已覆盖请求，无残留必要）。
    #[test]
    fn refresh_request_is_idempotent_and_reset_clears_it() {
        let mut remote_git = RemoteGit::default();
        assert!(!remote_git.refresh_requested);
        remote_git.request_refresh();
        remote_git.request_refresh();
        assert!(remote_git.refresh_requested);
        remote_git.reset();
        assert!(!remote_git.refresh_requested);
    }

    /// 根决策的三级回落：上报的仓库根优先（且必须是绝对路径、仍覆盖 cwd），
    /// 其次探测根（fish 等无集成嵌套 shell 的补位源），最后远端 cwd，全都
    /// 没有才无身份。探测根把 cwd 兜底的锚点从「可能在仓库子目录」修成真根。
    #[test]
    fn root_prefers_reported_then_probed_then_cwd() {
        assert_eq!(
            remote_git_root(Some("/srv/work/pebrel"), Some("/srv/work/pebrel/src"), None),
            Some("/srv/work/pebrel".to_owned())
        );
        assert_eq!(
            remote_git_root(None, Some("/home/user/proj"), None),
            Some("/home/user/proj".to_owned())
        );
        assert_eq!(remote_git_root(None, None, None), None);
        assert_eq!(
            remote_git_root(Some(""), Some("/home/user"), None),
            Some("/home/user".to_owned())
        );
        // 相对路径不构成远端身份。
        assert_eq!(remote_git_root(Some("relative"), None, None), None);
        assert_eq!(remote_git_root(None, Some("relative"), None), None);
        // 嵌套无集成 shell（fish 等）：UserVar 静默、OSC 7 照发，cwd 移出
        // 上报根后根即陈旧，探测根接管——即便 fish 停在仓库子目录，锚的
        // 也是 rev-parse 给出的真根而不是 cwd。
        let probe = project("d", "/srv/work/new", "main", "/srv/work/new/src/app");
        assert_eq!(
            remote_git_root(Some("/srv/old"), Some("/srv/work/new/src/app"), Some(&probe)),
            Some("/srv/work/new".to_owned())
        );
        // 探测过但不在仓库：根回落 cwd（status 128 自己说明身份）。
        let bare = project("d", "", "", "/home/user/plain");
        assert_eq!(
            remote_git_root(Some("/srv/old"), Some("/home/user/plain"), Some(&bare)),
            Some("/home/user/plain".to_owned())
        );
        // 探测根不覆盖 cwd（又 cd 走了）：不得接管。
        assert_eq!(
            remote_git_root(Some("/srv/old"), Some("/srv/third"), Some(&probe)),
            Some("/srv/third".to_owned())
        );
        // 上报根仍可信时探测不越位（集成权威）。
        assert_eq!(
            remote_git_root(
                Some("/srv/work/pebrel"),
                Some("/srv/work/pebrel/src"),
                Some(&project("d", "/srv/other", "x", "/"))
            ),
            Some("/srv/work/pebrel".to_owned())
        );
        // cwd 从未上报（无 OSC 7）时无从判陈旧，根保持可信。
        assert_eq!(remote_git_root(Some("/srv/old"), None, None), Some("/srv/old".to_owned()));
    }

    /// 单字段探测解析：根必须 POSIX 绝对；分支槽出现绝对路径形态 = 上游
    /// 把根塞错了槽（用户 10-15 现场），宁可空着也不错挂。
    #[test]
    fn probe_fields_reject_path_shaped_branches_and_non_posix_roots() {
        assert_eq!(
            probe_branch(
                "main
"
            )
            .as_deref(),
            Some("main")
        );
        assert_eq!(
            probe_branch(
                "  feature/x 
"
            )
            .as_deref(),
            Some("feature/x")
        );
        assert_eq!(
            probe_branch(
                "HEAD
"
            )
            .as_deref(),
            Some("HEAD"),
            "detached 与 --abbrev-ref 同口径"
        );
        assert_eq!(
            probe_branch(
                "/srv/work/pebrel
"
            ),
            None,
            "路径形态不是分支"
        );
        assert_eq!(probe_branch(""), None);
        assert_eq!(
            probe_root(
                "/srv/work/pebrel
"
            )
            .as_deref(),
            Some("/srv/work/pebrel")
        );
        assert_eq!(
            probe_root(
                "main
"
            ),
            None,
            "分支形态不是根"
        );
        assert_eq!(
            probe_root(
                "D:/gopath
"
            ),
            None,
            "Windows 盘符不是远端身份"
        );
    }

    /// porcelain → GitInfo 的分组与本地一致：staged/unstaged/conflicts 三组，
    /// 冲突路径同时保留在另外两组（显示层过滤重复）。
    #[test]
    fn porcelain_output_maps_to_grouped_snapshot() {
        let output = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0\
            # branch.ab +2 -1\0\
            1 .M N... 100644 100644 100644 abcdef1 abcdef2 modified.txt\0\
            1 M. N... 100644 100644 100644 abcdef1 abcdef2 staged.txt\0\
            ? untracked.txt\0\
            u UU N... 100644 100644 100644 100644 abcdef1 abcdef2 abcdef3 merged.txt\0";
        let info = remote_git_info(output).unwrap();
        assert_eq!(info.vcs, VcsKind::Git);
        assert_eq!(info.branch, "main");
        assert_eq!(info.ahead, 2);
        assert_eq!(
            info.unstaged.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(),
            ["modified.txt", "untracked.txt", "merged.txt"]
        );
        assert_eq!(
            info.staged.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(),
            ["staged.txt", "merged.txt"]
        );
        assert_eq!(
            info.conflicts.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(),
            ["merged.txt"]
        );
    }

    /// 解析失败收敛到 `Err`，render 层据此显示错误行而不是半份快照。
    #[test]
    fn malformed_output_is_an_error_not_a_partial_snapshot() {
        assert!(remote_git_info("not porcelain at all").is_err());
        assert!(remote_git_info("").is_err());
    }

    /// 命令行构造：路径与提交信息按 POSIX 单引号转义——空格、单引号、`$`
    /// 都必须活着到达远端 git，不能被远端登录 shell 先展开一步。
    #[test]
    fn command_line_quotes_root_paths_and_messages() {
        assert_eq!(
            remote_git_command("/srv/work/pebrel", &["add", "--", "src/main.rs"]),
            "git -C '/srv/work/pebrel' --no-optional-locks 'add' '--' 'src/main.rs'"
        );
        assert_eq!(
            remote_git_command("/srv/app", &["restore", "--staged", "--", "my file.txt"]),
            "git -C '/srv/app' --no-optional-locks 'restore' '--staged' '--' 'my file.txt'"
        );
        // 单引号按 POSIX 折叠规则转义（'\''），提交信息里的引号原样保留。
        assert_eq!(
            remote_git_command("/srv/app", &["commit", "-m", "fix: it's $HOME"]),
            "git -C '/srv/app' --no-optional-locks 'commit' '-m' 'fix: it'\\''s $HOME'"
        );
    }
}
