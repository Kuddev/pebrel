//! 侧栏「最近任务」分区：本机 AI CLI 历史会话，并入统一侧栏列表（行模型
//! 供给见 [`recent_sidebar_rows`]，渲染与 TABS 条目同一解剖：第一行品牌 +
//! 相对时间，第二行标题两行省略；项目组头可折叠，键带 [`RECENT_GROUP_KEY_PREFIX`]
//! 前缀与 TABS 组隔离）。
//!
//! 数据源与命令面板「恢复 AI 会话」共用 [`crate::ai_sessions::scan`]（每文件
//! 只读头 64KB）。展示为两级结构：**项目 → 任务**——组键与「当前任务」分区
//! 同一身份口径：会话目录向上折到 git 仓库根（同判据
//! [`crate::display::side_panel::git_repository_root`]），同一仓库子目录里的
//! 会话与当前任务 tab 归并同一个 `[项目名]` 组；非路径串（claude 不可逆编码
//! 目录名）与无 `.git` 的目录自身成组。解析只发生在后台扫描线程（stat 链），
//! render 只读 [`RecentSessionsState::groups`] 这份内存、零磁盘 IO。项目按组内
//! 最新会话时间降序取最近 [`PROJECT_CAP`] 个，组内按会话时间降序取最近
//! [`TASKS_PER_PROJECT_CAP`] 条。
//!
//! 生命周期照 `details_panel` 的门控轮询（5s 一 tick，纯内存判定）：展开按
//! 刷新间隔/身份置脏扫描；**收起也保留一次基线统计**（钉底分区头的数量
//! 徽标要有真数字），此后休眠直到置脏或重新展开（见 [`should_scan`]）。
//! 侧栏整体不可见（折叠 / Top 模式 / keep-session 隐藏）时轮询才真正退出。
//! epoch 单调递增，迟到结果按 epoch 拒绝；扫描 Task 存在 `pending` 里，
//! 状态销毁随 Task drop 取消。
//!
//! render 无法拥有 `&mut self` 起轮询，门控探针挂在统一列表容器的 canvas 上
//! （与 `remember_tabs_list_bounds` 合一）：prepaint 回主线程 `update` 做一次
//! 幂等的 `ensure_recent_sessions_polling`。

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;

use super::sidebar_tree::TABS_DISCLOSURE_SLOT_W;
use super::task_groups::{RecentSessionRow, SidebarRow};
use super::*;
use crate::i18n::Message;

/// 单次扫描拿多少条（`scan` 只为这批读文件头；上限 = 项目 × 任务两档封顶）。
const SCAN_LIMIT: usize = PROJECT_CAP * TASKS_PER_PROJECT_CAP + 20;
/// 展示最近多少个项目（按组内最新会话时间降序）。
const PROJECT_CAP: usize = 10;
/// 每个项目展示最近多少条会话（按会话时间降序）。
const TASKS_PER_PROJECT_CAP: usize = 10;
/// 刷新间隔：磁盘上的会话列表不需要秒级新鲜度。
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// 轮询 tick：门控判定是纯内存读，间隔只影响「重新变可见 → 恢复扫描」的
/// 响应速度与身份置脏后的收敛速度。
const POLL_TICK: Duration = Duration::from_secs(5);

/// 进行中的扫描：epoch 是发布凭据，`_task` 持有即「扫描在途」。
struct PendingScan {
    epoch: u64,
    _task: gpui::Task<()>,
}

pub(super) struct RecentSessionsState {
    /// 已发布的展示分组（组键已折到 git 仓库根）。render 只读这份内存。
    groups: Vec<RecentGroup>,
    pending: Option<PendingScan>,
    next_epoch: u64,
    collapsed: bool,
    /// 门控轮询在跑。置 false 的唯一路径是循环自己发现门控失效。
    polling: bool,
    /// 上次成功发布时刻；`None` = 脏（身份事件置脏，下一 tick 重扫）。
    last_refresh: Option<Instant>,
}

impl Default for RecentSessionsState {
    fn default() -> Self {
        Self {
            groups: Vec::new(),
            pending: None,
            next_epoch: 0,
            // 启动默认收起：分区头钉在侧栏底部，用户点开才轮询扫描
            // （用户 10-10 裁定）。
            collapsed: true,
            polling: false,
            last_refresh: None,
        }
    }
}

impl RecentSessionsState {
    /// 发布一次扫描结果。epoch 不匹配（有过更新的发布或重扫）即拒绝，
    /// 旧结果不得覆盖新状态。
    fn accept(&mut self, epoch: u64, groups: Vec<RecentGroup>) -> bool {
        if self.pending.as_ref().is_none_or(|pending| pending.epoch != epoch) {
            return false;
        }
        self.pending = None;
        self.groups = groups;
        self.last_refresh = Some(Instant::now());
        true
    }

    /// 测试注入：绕过扫描直接铺数据并标新鲜（分组渲染单测用；组键取
    /// 会话项目串本身，不做 git 根解析）。
    #[cfg(test)]
    pub(super) fn install_test_sessions(&mut self, rows: Vec<crate::ai_sessions::AiSession>) {
        let identity = |session: &crate::ai_sessions::AiSession| session.project.trim().to_owned();
        self.groups = recent_groups_by(&rows, identity);
        self.last_refresh = Some(Instant::now());
    }

    /// 已加载的会话条数（分区头计数与滚动粗钳制用）。
    pub(super) fn row_count(&self) -> usize {
        self.groups.iter().map(|group| group.sessions.len()).sum()
    }

    pub(super) fn groups(&self) -> &[RecentGroup] {
        &self.groups
    }

    pub(super) fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// 扫描在途且还没有数据（占位行显示「扫描中」而非「暂无」）。
    pub(super) fn is_scanning_empty(&self) -> bool {
        self.pending.is_some() && self.groups.is_empty()
    }
}

/// 最近组的折叠键前缀：组键是 git 根/项目目录，与 cwd/git 根的 TABS 组键
/// 同源，前缀隔离后共用 [`NebulaWorkspace::sidebar_group_collapsed`]。
pub(super) const RECENT_GROUP_KEY_PREFIX: &str = "recent:";

/// 会话列表 → 统一行模型的最近分区段：项目组头 + 会话条目；空数据出一行
/// 状态占位。两级封顶（[`PROJECT_CAP`]×[`TASKS_PER_PROJECT_CAP`]）由
/// [`recent_groups_by`] 保持。
pub(super) fn recent_sidebar_rows(
    groups: &[RecentGroup],
    collapsed: &std::collections::HashMap<SharedString, bool>,
    scanning: bool,
) -> Vec<SidebarRow> {
    if groups.is_empty() {
        return vec![SidebarRow::RecentStatus { scanning }];
    }
    let mut rows = Vec::new();
    for group in groups {
        let collapse_key: SharedString = format!("{RECENT_GROUP_KEY_PREFIX}{}", group.key).into();
        let is_collapsed = collapsed.get(&collapse_key).copied().unwrap_or(false);
        rows.push(SidebarRow::RecentGroupHeader {
            collapse_key: collapse_key.clone(),
            name: group.name.clone().into(),
            sessions: group.sessions.len(),
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            rows.extend(group.sessions.iter().map(|session| {
                SidebarRow::RecentSession(RecentSessionRow {
                    source: session.source,
                    id: session.id.clone().into(),
                    title: session.title.clone().into(),
                    project: session.project.clone().into(),
                    modified: session.modified,
                    resumable: session.source.resume_command(&session.id).is_some(),
                })
            }));
        }
    }
    rows
}

/// 一个项目的展示组：组名 + 组内最近的会话（时间降序、已封顶）。
#[derive(Clone, Debug)]
pub(super) struct RecentGroup {
    /// 组键 = git 仓库根（与 TABS 分组同判据）或原样项目串。
    key: String,
    name: String,
    /// 组内最新会话时刻：组间排序键。
    latest: std::time::SystemTime,
    sessions: Vec<crate::ai_sessions::AiSession>,
}

/// 组键 → 组头显示名：真实路径取尾段目录名；claude 不可逆编码目录名
/// （无路径分隔符）取最后一个 `-` 后的尾段——`place_label` 的同款口径，
/// 只是身份从「会话项目串」换成了「组键」。
fn group_display_name(key: &str) -> String {
    if key.contains('/') || key.contains('\\') {
        return std::path::Path::new(key)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    key.rsplit('-').next().unwrap_or("").to_owned()
}

/// 扫描结果 → 展示分组：同组键聚一组；组按**组内最新会话时间**降序取
/// 最近 [`PROJECT_CAP`] 个（不依赖输入已排序——首见顺序会被交错的时间序
/// 打乱）；组内按会话时间降序取最近 [`TASKS_PER_PROJECT_CAP`] 条。组名取
/// 组键尾段（[`group_display_name`]），空名组由调用方兜底翻译。
fn recent_groups_by(
    rows: &[crate::ai_sessions::AiSession],
    mut key_of: impl FnMut(&crate::ai_sessions::AiSession) -> String,
) -> Vec<RecentGroup> {
    let mut groups: Vec<RecentGroup> = Vec::new();
    for session in rows {
        let key = key_of(session);
        match groups.iter_mut().find(|group| group.key == key) {
            Some(group) => {
                group.latest = group.latest.max(session.modified);
                group.sessions.push(session.clone());
            },
            None => groups.push(RecentGroup {
                name: group_display_name(&key),
                key,
                latest: session.modified,
                sessions: vec![session.clone()],
            }),
        }
    }
    for group in &mut groups {
        group.sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
        group.sessions.truncate(TASKS_PER_PROJECT_CAP);
    }
    // 稳定排序：同刻并列时保持首见序。
    groups.sort_by(|a, b| b.latest.cmp(&a.latest));
    groups.truncate(PROJECT_CAP);
    groups
}

/// 扫描结果的组键解析：绝对路径向上折到 git 仓库根（与「当前任务」分区
/// 同一判据 [`crate::display::side_panel::git_repository_root`]），子目录
/// 会话与仓库根归并同组；非路径串与空白原样自组。**只在后台扫描线程调用**
/// （stat 链是磁盘 IO，render 路径零 IO）。`repository_root` 注入化是为
/// 单测喂假探测器，生产入口见 [`resolve_recent_groups`]。
fn resolve_recent_groups_with(
    sessions: &[crate::ai_sessions::AiSession],
    repository_root: impl Fn(&std::path::Path) -> Option<std::path::PathBuf>,
) -> Vec<RecentGroup> {
    let mut cache: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    recent_groups_by(sessions, |session| {
        let raw = session.project.trim();
        if raw.is_empty() || !std::path::Path::new(raw).is_absolute() {
            return raw.to_owned();
        }
        cache
            .entry(raw.to_owned())
            .or_insert_with(|| {
                repository_root(std::path::Path::new(raw))
                    .map(|root| root.to_string_lossy().into_owned())
                    .unwrap_or_else(|| raw.to_owned())
            })
            .clone()
    })
}

/// 生产入口：真实 git 根探测（后台线程）。
fn resolve_recent_groups(sessions: Vec<crate::ai_sessions::AiSession>) -> Vec<RecentGroup> {
    resolve_recent_groups_with(&sessions, crate::display::side_panel::git_repository_root)
}

/// 是否该发起新扫描：从没扫过、脏了、或距上次发布超过刷新间隔。
fn should_refresh(last_refresh: Option<Instant>) -> bool {
    last_refresh.is_none_or(|at| at.elapsed() >= REFRESH_INTERVAL)
}

/// 扫描调度决策：展开按刷新间隔走常规节奏；**收起也保留一次基线统计**
/// （钉底分区头的数量徽标要有真数字，用户 10-10 裁定），此后休眠直到
/// 身份置脏（`last_refresh` 被清空）或重新展开。
fn should_scan(collapsed: bool, last_refresh: Option<Instant>) -> bool {
    if !collapsed {
        return should_refresh(last_refresh);
    }
    last_refresh.is_none()
}

/// 会话项目串 → 新终端 cwd 候选。空串/纯空白不成路径；是否真的是目录由
/// 事件路径的 `is_dir` 再裁（render 不做磁盘 IO）。
fn recent_session_cwd(project: &str) -> Option<std::path::PathBuf> {
    let trimmed = project.trim();
    (!trimmed.is_empty()).then(|| std::path::PathBuf::from(trimmed))
}

/// 一条最近会话的最小可执行身份：render 时拷出，点击时在事件路径重建命令。
#[derive(Clone)]
struct RecentSessionLaunch {
    source: crate::ai_sessions::AiSessionSource,
    id: String,
    project: String,
}

impl RecentSessionLaunch {
    fn of(session: &crate::ai_sessions::AiSession) -> Self {
        Self { source: session.source, id: session.id.clone(), project: session.project.clone() }
    }

    fn resume_command(&self) -> Option<String> {
        self.source.resume_command(&self.id)
    }
}

impl NebulaWorkspace {
    /// 门控：侧栏形态可见、窗口未被 keep-session 隐藏、agent 侧栏模式开启
    /// （「最近任务」分区只属于新样式；经典列表不渲染探针，轮询也须真正
    /// 退出，不留后台磁盘扫描）。分区收起**不再**阻断轮询——循环存活但只
    /// 按 [`should_scan`] 补基线/置脏扫描；侧栏整体不可见（Top 模式/折叠/
    /// 隐藏）时才真正退出。
    fn recent_sessions_visible(&self, cx: &gpui::App) -> bool {
        !self.sidebar_collapsed
            && self.tabs_position == nebula_settings::TabsPositionName::Sidebar
            && !self.window_hidden
            && crate::gpui_shell::config::ai_agent_sidebar(cx)
    }

    /// canvas 探针的幂等回调（见模块头）。每帧最多一次真实工作。
    pub(super) fn remember_recent_sessions_gate(&mut self, cx: &mut gpui::Context<Self>) {
        if self.recent_sessions.polling || !self.recent_sessions_visible(cx) {
            return;
        }
        self.ensure_recent_sessions_polling(cx);
    }

    /// 启动门控轮询循环。循环自身裁定去留：门控失效即置 `polling = false`
    /// 并退出，探针会在下次可见时重新拉起。
    fn ensure_recent_sessions_polling(&mut self, cx: &mut gpui::Context<Self>) {
        if self.recent_sessions.polling {
            return;
        }
        self.recent_sessions.polling = true;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                let keep = this
                    .update(cx, |workspace, cx| {
                        if !workspace.recent_sessions_visible(cx) {
                            workspace.recent_sessions.polling = false;
                            return false;
                        }
                        workspace.maybe_start_recent_scan(cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
                executor.timer(POLL_TICK).await;
            }
        })
        .detach();
    }

    /// tick 内的扫描调度：无在途扫描且 [`should_scan`] 到期才发起新一轮
    /// （展开按刷新间隔；收起只在从未统计或身份置脏时补一次）。
    fn maybe_start_recent_scan(&mut self, cx: &mut gpui::Context<Self>) {
        if self.recent_sessions.pending.is_some()
            || !should_scan(self.recent_sessions.collapsed, self.recent_sessions.last_refresh)
        {
            return;
        }
        let epoch = self.recent_sessions.next_epoch;
        self.recent_sessions.next_epoch += 1;
        let task = cx.spawn(async move |this, cx| {
            // 扫描与组键解析（git 根 stat 链）永远在后台线程（纯文件 IO）；
            // 主线程只收分组结果。
            let worker = cx
                .background_executor()
                .spawn(async move { resolve_recent_groups(crate::ai_sessions::scan(SCAN_LIMIT)) });
            let groups = worker.await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace.recent_sessions.accept(epoch, groups) {
                    cx.notify();
                }
            });
        });
        self.recent_sessions.pending = Some(PendingScan { epoch, _task: task });
    }

    /// 身份事件（hook 上报新会话）置脏：下一 tick 提前刷新，不等待整段间隔。
    pub(super) fn dirty_recent_sessions(&mut self) {
        self.recent_sessions.last_refresh = None;
    }

    pub(super) fn toggle_recent_sessions(&mut self, cx: &mut gpui::Context<Self>) {
        self.recent_sessions.collapsed = !self.recent_sessions.collapsed;
        // 行移位：右键菜单锚定的行不在原处了（同 `toggle_tabs_section`）。
        self.tab_menu = None;
        self.clamp_rows_after_group_toggle(cx);
        cx.notify();
    }

    /// 「最近任务」分区头行：折叠箭头 + 标题 + 计数（无动作按钮）。
    /// Top 模式 / 侧栏折叠时整个统一列表不渲染，探针随之消失，轮询自灭。
    pub(super) fn render_recent_section_header(
        &self,
        row_w: f32,
        count: usize,
        collapsed: bool,
        cx: &gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        let settings = cx.try_global::<crate::gpui_shell::config::Settings>();
        let label_px = settings.map(|settings| settings.ui_font_size_px).unwrap_or(15.0);
        let language = crate::gpui_shell::config::ui_language(cx);
        let title = language.text(Message::SidebarRecentTasks);
        let count_text: SharedString = count.to_string().into();
        h_flex()
            .id("sidebar-recent-toggle")
            .w(px(row_w))
            .h(px(super::tab_scroll::tab_row_height(self.density)))
            .pb_1()
            .pl_2()
            .items_center()
            .cursor_pointer()
            .flex_shrink_0()
            .debug_selector(|| "sidebar-recent-toggle".to_owned())
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_recent_sessions(cx);
            }))
            // click 合成依赖 bubble 阶段的 mouse_down listener（同组头/条目行）。
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, _, _| {}),
            )
            .child(
                h_flex()
                    .w(px(TABS_DISCLOSURE_SLOT_W))
                    .h_full()
                    .flex_shrink_0()
                    .items_center()
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall()
                        .text_color(muted),
                    ),
            )
            .child(
                div()
                    .text_size(px(label_px * SIDEBAR_TITLE_SCALE))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(muted)
                    .child(title),
            )
            .child(
                h_flex()
                    .ml_2()
                    .h(px((label_px * 1.22 * SIDEBAR_TITLE_SCALE * 1.18).max(11.0)))
                    .min_w(px(label_px * 0.62 + 8.0))
                    .px_2()
                    .justify_center()
                    .items_center()
                    .rounded_full()
                    .bg(theme.muted)
                    .text_size(px(label_px * SIDEBAR_TITLE_SCALE))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(faint)
                    .child(count_text),
            )
            // 数量徽标右侧的水平 hairline：填满标题行剩余宽度（本分区头无
            // 动作钮，线直达右缘）。
            .child(
                div()
                    .debug_selector(|| "sidebar-recent-hairline".to_owned())
                    .flex_1()
                    .ml_2()
                    .h(px(1.0))
                    .bg(crate::gpui_shell::theme::card_divider_color(cx)),
            )
            .into_any_element()
    }

    /// 最近任务的项目组头：与 TABS 组头同一解剖（chevron + 文件夹 +
    /// `[名]` + 右计数），点击折叠/展开该组；`collapse_key` 已带 `recent:`
    /// 前缀，与 TABS 组共用折叠状态表但互不干扰。
    pub(super) fn render_recent_group_header(
        &self,
        row_w: f32,
        row_ix: usize,
        collapse_key: &SharedString,
        name: &SharedString,
        sessions: usize,
        collapsed: bool,
        cx: &gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let hover_bg = theme.list_hover;
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        let settings = cx.try_global::<crate::gpui_shell::config::Settings>();
        let label_px = settings.map(|settings| settings.ui_font_size_px).unwrap_or(15.0);
        let chrome_family = theme.mono_font_family.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let tooltip = if collapsed {
            language.text(Message::SidebarExpandGroup)
        } else {
            language.text(Message::SidebarCollapseGroup)
        };
        // 空名组（项目串为空）用兜底翻译，不是裸空串。
        let display_name: SharedString = if name.trim().is_empty() {
            language.text(Message::SidebarRecentOthers).into()
        } else {
            name.clone()
        };
        let toggle_key = collapse_key.clone();
        let count: SharedString = sessions.to_string().into();
        let name_text: SharedString = format!("[{display_name}]").into();
        // 选择器用裸组键（剥掉 `recent:` 前缀再消毒），避免「recent-group-recent_…」
        // 的重复前缀。
        let raw_key =
            collapse_key.strip_prefix(RECENT_GROUP_KEY_PREFIX).unwrap_or(collapse_key.as_ref());
        let selector = sanitize_selector_slug(raw_key);
        h_flex()
            .id(("recent-group", row_ix))
            .debug_selector(move || format!("recent-group-{selector}"))
            .w(px(row_w))
            .h(px(super::task_groups::GROUP_HEADER_H))
            .overflow_hidden()
            .px_2()
            .gap_1()
            .items_center()
            .rounded(px(crate::display::UI_CORNER_RADIUS_LOGICAL))
            .font_weight(FontWeight::NORMAL)
            .cursor_pointer()
            .text_color(muted)
            .hover(|style| style.bg(hover_bg))
            .tooltip(move |window, cx| tab_presentation::tooltip(tooltip.into(), window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_sidebar_group(toggle_key.clone(), cx);
            }))
            // 同条目行：click 合成链路依赖视图节点的 mouse_down listener。
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, _, _| {}),
            )
            .child(
                h_flex()
                    .w(px(super::task_groups::GROUP_CHEVRON_SLOT_W))
                    .h_full()
                    .flex_shrink_0()
                    .items_center()
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall()
                        .text_color(muted),
                    ),
            )
            .child(Icon::new(IconName::Folder).xsmall().text_color(muted))
            .child(
                div()
                    .min_w_0()
                    .max_w(px(row_w * 0.5))
                    .truncate()
                    .font_family(chrome_family)
                    .text_size(px(label_px * SIDEBAR_TITLE_SCALE))
                    .text_color(theme.success)
                    .child(name_text),
            )
            // 名称之后补弹性空位，计数靠右（同 TABS 组头）。
            .child(div().flex_1())
            .when(sessions > 1, |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(label_px * SIDEBAR_TAG_SCALE))
                        .text_color(faint)
                        .child(count),
                )
            })
            .into_any_element()
    }

    /// 扫描中/暂无历史的占位行。
    pub(super) fn render_recent_status_row(
        &self,
        row_w: f32,
        scanning: bool,
        cx: &gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        let settings = cx.try_global::<crate::gpui_shell::config::Settings>();
        let label_px = settings.map(|settings| settings.ui_font_size_px).unwrap_or(15.0);
        let language = crate::gpui_shell::config::ui_language(cx);
        let text = if scanning {
            language.text(Message::SidebarRecentScanning)
        } else {
            language.text(Message::SidebarRecentEmpty)
        };
        div()
            .w(px(row_w))
            .h(px(super::task_groups::GROUP_HEADER_H))
            .pl_2()
            .flex()
            .items_center()
            .flex_shrink_0()
            .overflow_hidden()
            .text_size(px(label_px * SIDEBAR_TAG_SCALE))
            .text_color(faint)
            .child(text)
            .into_any_element()
    }

    /// 单条最近会话（与 TABS 条目同一解剖）：第一行 = 图标 + 来源品牌 +
    /// 右对齐相对时间；第二行 = 会话标题，自动换行最多两行、末行省略。
    /// 无 resume 命令的来源渲染为禁用态（灰字、无点击）。
    pub(super) fn render_recent_session_row(
        &self,
        row_ix: usize,
        row_w: f32,
        session: &RecentSessionRow,
        cx: &gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        let hover_bg = theme.list_hover;
        let dark = theme.is_dark();
        let chrome_family = theme.mono_font_family.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let metrics = self.row_metrics(cx);
        let entry_h = super::task_groups::sidebar_entry_height(&metrics);
        let subline_line_h = super::task_groups::subline_line_height(&metrics);
        let subline_font_px = super::task_groups::subline_font_px(&metrics);
        // 副行带高：第一行内容在剩余高度里垂直居中，副行走 absolute 槽位。
        let subline_h = 2.0 * subline_line_h + 2.0;
        // 标题带左缘与 TABS 条目的任务名文字对齐。
        let title_left = super::task_groups::tree_text_left();
        let launch = RecentSessionLaunch {
            source: session.source,
            id: session.id.to_string(),
            project: session.project.to_string(),
        };
        let resumable = launch.resume_command().is_some();
        let source: SharedString = session.source.display_name().to_owned().into();
        let time: SharedString =
            crate::ai_sessions::relative_label(session.modified, language).into();
        let logo = crate::display::ai_logo_for_program(session.source.slug())
            .and_then(|logo| self.sidebar_logo_images.get(&(logo, dark)).cloned());
        let glyph = logo.is_none().then(|| crate::display::program_icon(session.source.slug()));
        let tooltip: SharedString = if resumable {
            format!("{}\n{}", language.text(Message::SidebarResumeSession), session.title).into()
        } else {
            session.title.clone()
        };
        let selector_launch = launch.clone();
        let click_launch = launch;
        h_flex()
            .id(("recent-session", row_ix))
            .debug_selector(move || format!("recent-session-{}", selector_launch.selector_slug()))
            .w(px(row_w))
            .h(px(entry_h))
            // 底部留出两行副行带；左侧让位目录树肘形线（同 TABS 条目）。
            .pb(px(subline_h))
            .pl(px(super::task_groups::tree_icon_left()))
            .pr(px(8.0))
            .gap_2()
            .items_center()
            .flex_shrink_0()
            .overflow_hidden()
            .rounded(px(crate::display::UI_CORNER_RADIUS_LOGICAL))
            .font_weight(FontWeight::NORMAL)
            .text_color(if resumable { muted } else { faint })
            .when(resumable, |row| row.cursor_pointer().hover(|style| style.bg(hover_bg)))
            .tooltip(move |window, cx| tab_presentation::tooltip(tooltip.clone(), window, cx))
            .when(resumable, |row| {
                row.on_click(cx.listener(move |this, _, window, cx| {
                    let Some(command) = click_launch.resume_command() else { return };
                    let cwd =
                        recent_session_cwd(&click_launch.project).filter(|path| path.is_dir());
                    this.add_terminal_at(cwd, Some(command), window, cx);
                }))
                // 同条目行：click 合成依赖 bubble mouse_down listener。
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _: &MouseDownEvent, _, _| {}),
                )
            })
            // 第一行：品牌身份 + 右对齐相对时间。
            .when_some(logo, |row, image| {
                row.child(
                    img(image)
                        .size(px(TAB_LABEL_ICON_SIZE))
                        .flex_shrink_0()
                        .object_fit(ObjectFit::Contain),
                )
            })
            .when_some(glyph, |row, glyph| {
                row.child(
                    div()
                        .w(px(TAB_LABEL_ICON_W))
                        .flex_shrink_0()
                        .truncate()
                        .text_color(if resumable { muted } else { faint })
                        .child(glyph),
                )
            })
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(subline_font_px))
                    .text_color(if resumable { muted } else { faint })
                    .child(source),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(subline_font_px))
                    .text_color(faint)
                    .child(time),
            )
            // 第二行：会话标题两行省略（同 TABS 摘要的 line_clamp 组合，
            // 不能用 .truncate()——nowrap 会关掉换行）。
            .child(
                div()
                    .absolute()
                    .left(px(title_left))
                    .bottom(px(2.0))
                    .w(px((row_w - title_left - 16.0).max(40.0)))
                    .h(px(2.0 * subline_line_h))
                    .line_height(px(subline_line_h))
                    .line_clamp(2)
                    .text_ellipsis()
                    .font_family(chrome_family)
                    .text_size(px(subline_font_px))
                    .text_color(if resumable { muted } else { faint })
                    .child(session.title.clone()),
            )
            .into_any_element()
    }
}

/// debug_selector 的稳定短名：同一来源的 id 已是 slug 形态（uuid / 文件名），
/// 只留安全字符，避免选择器串里出现路径分隔符。
fn sanitize_selector_slug(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' })
        .collect()
}

impl RecentSessionLaunch {
    fn selector_slug(&self) -> String {
        sanitize_selector_slug(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_is_due_when_never_run_dirty_or_stale() {
        assert!(should_refresh(None), "first scan is always due");
        assert!(should_refresh(Some(Instant::now() - REFRESH_INTERVAL)), "stale is due");
        assert!(!should_refresh(Some(Instant::now())), "freshly published data is not due");
    }

    #[test]
    fn collapsed_still_scans_once_for_the_baseline_count() {
        // 启动默认收起：从未统计 → 补一次基线（数量徽标要有真数字）。
        assert!(should_scan(true, None), "collapsed startup still scans once");
        // 基线到手后收起态休眠，不再按间隔刷新。
        assert!(!should_scan(true, Some(Instant::now())));
        assert!(!should_scan(true, Some(Instant::now() - REFRESH_INTERVAL)));
        // 身份置脏（last_refresh 清空）即使在收起态也补扫，数字跟上新会话。
        assert!(should_scan(true, None));
        // 展开恢复常规节奏。
        assert!(should_scan(false, None));
        assert!(!should_scan(false, Some(Instant::now())));
        assert!(should_scan(false, Some(Instant::now() - REFRESH_INTERVAL)));
    }

    fn session(project: &str, id: &str, minutes_ago: u64) -> crate::ai_sessions::AiSession {
        crate::ai_sessions::AiSession::test_session_at(
            crate::ai_sessions::AiSessionSource::Codex,
            id,
            &format!("task {id}"),
            project,
            std::time::SystemTime::now() - Duration::from_secs(minutes_ago * 60),
        )
    }

    #[test]
    fn groups_follow_recency_and_cap_both_levels() {
        let identity = |session: &crate::ai_sessions::AiSession| session.project.trim().to_owned();
        // scan 已按时间降序喂入：p-new 的会话最新，p-new 因此排最前。
        let rows = vec![
            session("D:/new", "n1", 1),
            session("D:/old", "o1", 60),
            session("D:/new", "n2", 5),
            session("D:/mid", "m1", 30),
        ];
        let groups = recent_groups_by(&rows, identity);
        assert_eq!(groups[0].key, "D:/new");
        assert_eq!(groups[0].name, "new");
        assert_eq!(
            groups[0].sessions.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            ["n1", "n2"]
        );
        assert_eq!(groups[1].key, "D:/mid");
        assert_eq!(groups[2].key, "D:/old");

        // 组内封顶：同项目第 11 条不再进组。
        let many: Vec<_> =
            (0..12).map(|ix| session("D:/bulk", &format!("b{ix}"), ix + 1)).collect();
        let groups = recent_groups_by(&many, identity);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].sessions.len(), TASKS_PER_PROJECT_CAP);

        // 项目封顶：第 11 个项目被丢弃（输入按时间降序，前 10 个更新）。
        let wide: Vec<_> = (0..12).map(|ix| session(&format!("D:/p{ix}"), "s", ix + 1)).collect();
        let groups = recent_groups_by(&wide, identity);
        assert_eq!(groups.len(), PROJECT_CAP);
        assert_eq!(groups.last().unwrap().key, "D:/p9");
    }

    #[test]
    fn blank_projects_share_one_group_and_cwd_candidates_need_meaning() {
        let identity = |session: &crate::ai_sessions::AiSession| session.project.trim().to_owned();
        let rows = vec![session("", "a", 1), session("  ", "b", 2), session("D:/x", "c", 3)];
        let groups = recent_groups_by(&rows, identity);
        // 空白项目串归同一组；组名留给渲染层兜底翻译。
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "");
        assert_eq!(groups[0].sessions.len(), 2);
        assert_eq!(recent_session_cwd("  "), None);
        assert_eq!(recent_session_cwd(""), None);
        assert_eq!(
            recent_session_cwd(r" D:\repo ").as_deref(),
            Some(std::path::Path::new(r"D:\repo"))
        );
    }

    /// 仓库根解析（后台线程跑，判据与当前任务分区同源）注入化：单测喂假
    /// root 探测器，不碰文件系统。
    #[test]
    fn subdirectory_sessions_merge_into_the_repository_group() {
        // 路径必须对当前平台是绝对路径（`is_absolute` 是归并解析的门）：
        // Windows 上 `/repo` 无盘符不算绝对，根探测器根本不会被调用。
        // 路径必须对当前平台是绝对路径（`is_absolute` 是归并解析的门）：
        // Windows 上 `/x` 无盘符不算绝对，根探测器根本不会被调用。运行时
        // 探测不占 platform cfg 预算。
        let repo = if std::path::Path::new("/repo").is_absolute() { "/repo" } else { "C:/repo" };
        let standalone = if std::path::Path::new("/standalone").is_absolute() {
            "/standalone"
        } else {
            "C:/standalone"
        };
        let rows = vec![
            session(&format!("{repo}/nebula_app"), "sub", 1),
            session(repo, "root", 2),
            session(standalone, "solo", 3),
        ];
        let groups = resolve_recent_groups_with(&rows, |path| {
            path.starts_with(repo).then(|| std::path::PathBuf::from(repo))
        });
        // 同一仓库的子目录会话与仓库根会话归并成一组，组名取仓库根尾段
        // ——与当前任务分区的 [pebrel] 组同一身份口径。
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, repo);
        assert_eq!(groups[0].name, "repo");
        assert_eq!(
            groups[0].sessions.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            ["sub", "root"]
        );
        // 无 .git 的目录保持自身成组。
        assert_eq!(groups[1].key, standalone);
        // 非 git 仓库的会话 cwd 不受影响（恢复仍按会话自己的目录起终端）。
        assert_eq!(groups[1].sessions[0].project, standalone);
    }

    #[test]
    fn encoded_project_labels_stay_their_own_group_with_a_tail_name() {
        // claude 不可逆编码目录名（无路径分隔符）不进 git 根解析，自身成组；
        // 组名取编码尾段（place_label 的同款口径）。
        let rows = vec![session("-Users-apple-work-nebula", "a", 1)];
        let groups = resolve_recent_groups_with(&rows, |_| None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "-Users-apple-work-nebula");
        assert_eq!(groups[0].name, "nebula");
    }

    #[test]
    fn stale_epoch_results_never_overwrite_newer_state() {
        let identity = |session: &crate::ai_sessions::AiSession| session.project.trim().to_owned();
        let groups_of = |rows: &[crate::ai_sessions::AiSession]| recent_groups_by(rows, identity);
        let mut state = RecentSessionsState::default();
        state.pending = Some(PendingScan { epoch: 7, _task: gpui::Task::ready(()) });
        let fresh = groups_of(&[session("D:/new", "new", 1)]);
        assert!(state.accept(7, fresh), "matching epoch publishes");
        assert!(state.last_refresh.is_some());
        let stale = groups_of(&[session("D:/old", "old", 99)]);
        assert!(!state.accept(7, stale), "an already-consumed epoch is rejected");
        assert_eq!(state.groups.len(), 1, "old results must not overwrite new state");
        assert_eq!(state.groups[0].sessions[0].id, "new");
    }

    #[test]
    fn selector_slugs_drop_unsafe_characters() {
        assert_eq!(sanitize_selector_slug("a/b\\c d"), "a_b_c_d");
        assert_eq!(sanitize_selector_slug("0199a213-c2a4"), "0199a213-c2a4");
    }
}
