//! 侧栏统一列表的行模型：两个分区（当前任务/最近任务）的分区头、项目组
//! 头与条目全部展开成**一个** [`SidebarRow`] 序列，由单一窗口化滚动容器
//! 消费（见 [`NebulaWorkspace::sidebar_list_rows`]）。
//!
//! TABS 分组由 pane 的 cwd 派生：同一 git 仓库（或同一 cwd）的 tab 归一组，
//! 组头带项目名、分支 chip 与折叠箭头。组不是用户数据，**不参与 tab 顺序**；
//! 组间按**项目首开序**排列（见 [`NebulaWorkspace::note_project_group_open`]），
//! 点击切换任务不改任何排序输入。git 仓库根的探测只发生在事件路径（见
//! [`NebulaWorkspace::resolve_project_group`]），render 路径永远只查内存，
//! 不做任何磁盘 IO。
//!
//! 行高不再均匀（分区头 32–34px、组头 26px、条目两行），所以窗口化滚动
//! 用前缀和（见 [`row_top_offsets`]）；滚轮仍按整行步进。条目行留固定两行
//! 副行槽（摘要/标题自动换行最多两行），行高由 [`RowMetrics`] 纯函数推导
//! ——按内容变高会让未渲染行的前缀和失真。目录树引导线（含肘形连接）由
//! [`tree_guides`] 从同一行模型推导，垫在行底之下绘制。

use std::collections::HashMap;
use std::path::Path;

use gpui::{App, SharedString};

use nebula_settings::DensityName;

use super::*;

/// 兜底组键：cwd 尚未上报（新 pane）或非终端 tab（设置/文档/图片）。
/// 绝不会与真实组键撞车——本地 cwd 恒为绝对路径、SSH 键是目的地串。
pub(super) const WORKSPACE_GROUP_KEY: &str = "workspace";

/// 组头行高（逻辑 px）。比条目矮一档：它是分类标签，不是命中目标优先级
/// 最高的条目，但 26px 仍高于 20px 的最小命中区起点。
pub(super) const GROUP_HEADER_H: f32 = 26.0;

/// 组头折叠箭头槽宽（最左列）与分支图标边长（与 tag 字号同档）。
pub(super) const GROUP_CHEVRON_SLOT_W: f32 = 16.0;
const GROUP_BRANCH_ICON_SIZE: f32 = 12.0;

/// 分组条目（第一行标题 + 最多两行副行摘要）的行高派生输入：密度决定
/// 第一行带，界面字号决定副行行高（大字号下不被固定带裁掉）。
#[derive(Clone, Copy, Debug)]
pub(super) struct RowMetrics {
    density: DensityName,
    label_px: f32,
}

impl RowMetrics {
    pub(super) fn new(density: DensityName, label_px: f32) -> Self {
        Self { density, label_px }
    }
}

/// 副行字号（tag 档）与行高。
pub(super) fn subline_font_px(metrics: &RowMetrics) -> f32 {
    metrics.label_px * SIDEBAR_TAG_SCALE
}

pub(super) fn subline_line_height(metrics: &RowMetrics) -> f32 {
    (subline_font_px(metrics) * 1.30).ceil()
}

/// 条目行高 = 第一行带（Standard 30 / Compact 26）+ **固定两行**副行槽 +
/// 2px 底距。副行槽固定两行：窗口化滚动要求行高由 (row, metrics) 纯函数
/// 推导，按内容变高会让未渲染行的前缀和失真；一行摘要也保留两行带，
/// 留白换稳定。
/// 条目第一行带高度（Standard 30 / Compact 26）。
pub(super) fn headline_band(metrics: &RowMetrics) -> f32 {
    match metrics.density {
        DensityName::Standard => 30.0,
        DensityName::Compact => 26.0,
    }
}

pub(super) fn sidebar_entry_height(metrics: &RowMetrics) -> f32 {
    headline_band(metrics) + 2.0 * subline_line_height(metrics) + 2.0
}

/// 目录树引导线的 x 坐标（相对内容列左缘）：一级竖线对齐组头折叠箭头
/// 中心；条目图标列（[`tree_icon_left`]）在摘要交点的右侧再让出一档，
/// 给摘要肘形横臂留出可见长度。
pub(super) const TREE_X_L1: f32 = 16.0;
pub(super) const TREE_X_L2: f32 = 32.0;
/// 条目行图标列左缘（一级竖线 → 摘要交点 → 图标列三档缩进）。
pub(super) const TREE_ICON_LEFT: f32 = 44.0;
/// 树线粗细：1px 在高 DPI 下几乎不可读，统一 2px 才能读出「T」字形。
pub(super) const GUIDE_THICKNESS: f32 = 2.0;

/// 条目行图标列左缘（一级竖线 → 摘要交点 → 图标列三档缩进）。SSH 主机的
/// 子组不缩进（用户 10-13 裁定），与本地组共用同一档位。
pub(super) fn tree_icon_left() -> f32 {
    TREE_ICON_LEFT
}

/// 条目第一行任务名与第二行摘要/标题的文字左缘（图标列 + gap，两者对齐）。
pub(super) fn tree_text_left() -> f32 {
    tree_icon_left() + TAB_LABEL_ICON_W + 8.0
}

/// 一段树线（内容列坐标系里的矩形）。selector 为
/// `sidebar-guide-{kind}-{group}-{tag}`：kind 取 `l1`（一级竖线，tag=0）、
/// `elbow`（任务名肘形横臂）、`sumv`（摘要悬挂竖线）、`sume`（摘要肘形
/// 横臂），tag=组内子行序号，保证唯一。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TreeGuide {
    pub(super) group: usize,
    pub(super) kind: &'static str,
    pub(super) tag: usize,
    /// (x, y, w, h)。
    pub(super) bounds: (f32, f32, f32, f32),
}

/// 统一行模型的目录树线段（两级肘形：任务名是项目的子节点，摘要又挂
/// 在任务名之下）：
///
/// ```text
/// ▾ SSH host.example.com  ← 主机头是纯容器行：不画主机级树线、子组不
///                            缩进（用户 10-13 裁定，与本地组同几何）
/// ▾ [project]             ← 项目子组：与本地组同档位的任务肘形
/// ┊ ── 任务名              ← 任务肘形（T 字：非末子的一级竖线继续向下）
/// ┊   ── 摘要第一行        ← 摘要悬挂竖线 + 肘形横臂指向摘要首行
/// ┊       第二行续          （摘要块是任务名的唯一子树，肘形收口）
/// ┊ ── 下一个任务名
/// ```
///
/// SSH 子组与本地组共用同一套脊柱 x（[`TREE_X_L1`]/[`TREE_X_L2`]/
/// [`tree_icon_left`]）。折叠容器没有子行（行模型不含），天然不产出线段。
/// y 相对可见窗口首行，越出视口的线段按视口裁剪、空段丢弃。
pub(super) fn tree_guides(
    rows: &[SidebarRow],
    offsets: &[f32],
    first_visible_top: f32,
    viewport_h: f32,
    metrics: &RowMetrics,
) -> Vec<TreeGuide> {
    fn push_vertical(
        guides: &mut Vec<TreeGuide>,
        group: usize,
        kind: &'static str,
        tag: usize,
        x: f32,
        top: f32,
        bottom: f32,
        viewport_h: f32,
    ) {
        let top = top.max(0.0);
        let bottom = bottom.min(viewport_h);
        if bottom - top >= 1.0 {
            guides.push(TreeGuide {
                group,
                kind,
                tag,
                bounds: (x, top, GUIDE_THICKNESS, bottom - top),
            });
        }
    }

    fn push_horizontal(
        guides: &mut Vec<TreeGuide>,
        group: usize,
        kind: &'static str,
        tag: usize,
        x: f32,
        right: f32,
        y: f32,
        viewport_h: f32,
    ) {
        if y >= 0.0 && y <= viewport_h && right - x >= 1.0 {
            guides.push(TreeGuide {
                group,
                kind,
                tag,
                bounds: (x, y - GUIDE_THICKNESS * 0.5, right - x, GUIDE_THICKNESS),
            });
        }
    }

    let elbow_dy = headline_band(metrics) * 0.5;
    let line_h = subline_line_height(metrics);
    let entry_h = sidebar_entry_height(metrics);
    // 摘要第一行的中心：副行带自底向上排（底 2px 边距 + 两行行高）。
    let summary_elbow_dy = entry_h - 2.0 - 2.0 * line_h + line_h * 0.5;
    let mut guides = Vec::new();
    let mut group = 0usize;
    let mut i = 0usize;
    while i < rows.len() {
        // SSH 主机头是纯容器行，不画主机级树线：主机横臂的终点会正落在
        // 子组折叠箭头槽上，压住子组的折叠箭头（用户 10-13 裁定移除）。
        // 只让过行本身：子级（项目组头）由外层循环继续各自处理，产出它们
        // 自己的任务肘形。
        if matches!(rows[i], SidebarRow::HostHeader { .. }) {
            i += 1;
            continue;
        }
        let group_header = match &rows[i] {
            SidebarRow::GroupHeader { collapsed, .. } => Some(*collapsed),
            _ => None,
        };
        let Some(collapsed) = group_header else {
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < rows.len() && matches!(rows[j], SidebarRow::Tab { .. }) {
            j += 1;
        }
        if !collapsed && j > i + 1 {
            let spine = TREE_X_L1;
            let summary_spine = TREE_X_L2;
            // 摘要肘形横臂终点：摘要文字左缘左侧留 4px 呼吸。
            let summary_arm_right = tree_text_left() - 4.0;
            let header_center = offsets[i] + GROUP_HEADER_H * 0.5 - first_visible_top;
            let last_elbow = offsets[j - 1] + elbow_dy - first_visible_top;
            // 一级竖线：组头中心 → 末子任务肘形（末子的 └ 收口）。
            push_vertical(
                &mut guides,
                group,
                "l1",
                0,
                spine,
                header_center,
                last_elbow,
                viewport_h,
            );
            for (tag, child) in ((i + 1)..j).enumerate() {
                let top = offsets[child] - first_visible_top;
                let elbow_y = top + elbow_dy;
                // 任务名肘形：横臂从一级竖线直达图标列（T 字横杠）。
                push_horizontal(
                    &mut guides,
                    group,
                    "elbow",
                    tag,
                    spine,
                    tree_icon_left(),
                    elbow_y,
                    viewport_h,
                );
                // 摘要悬挂竖线：任务肘形交点下方 → 摘要肘形（└ 收口，
                // 摘要是任务名的唯一子树）。
                let summary_y = top + summary_elbow_dy;
                push_vertical(
                    &mut guides,
                    group,
                    "sumv",
                    tag,
                    summary_spine,
                    elbow_y,
                    summary_y,
                    viewport_h,
                );
                // 摘要肘形横臂：指向摘要第一行文字。
                push_horizontal(
                    &mut guides,
                    group,
                    "sume",
                    tag,
                    summary_spine,
                    summary_arm_right,
                    summary_y,
                    viewport_h,
                );
            }
        }
        group += 1;
        i = j.max(i + 1);
    }
    guides
}

/// 一个 tab 的分组归属，由 [`NebulaWorkspace::tab_group_info`] 推导。
#[derive(Clone, Debug)]
pub(super) struct TabGroupInfo {
    /// `Some(destination)`：SSH pane（`TerminalLaunch::Ssh`），在侧栏呈
    /// 「主机头 → 项目子组」两级；本地 pane 为 `None`，直接一级组。
    pub(super) host: Option<SharedString>,
    /// 组键：git 仓库根 / cwd / [`WORKSPACE_GROUP_KEY`]；SSH 为
    /// `ssh:{dest}|{git_root|cwd|workspace}`（见 [`super::task_groups_ssh`]）。
    pub(super) key: SharedString,
    /// 组头显示名（组键的末级目录名；SSH 兜底子组为
    /// [`WORKSPACE_GROUP_KEY`] 哨兵，渲染层翻译成「工作区」）。
    pub(super) name: SharedString,
    /// 分支 chip 文本；空分支不画。
    pub(super) branch: Option<SharedString>,
}

/// cwd 是否够格成组：Windows 绝对路径（有盘符）或 Unix 风格绝对路径。
/// 后者是 WSL/MSYS 家族 shell（fish、bash）经 OSC 7 上报的形态——
/// Windows 的 `Path::is_absolute` 对 `/mnt/d/…` 判 false，直接拒掉会让
/// 「进入 fish 后 cd」的 tab 永远落在「工作区」兜底组（用户 10-10 报障）。
/// 远端（SSH）cwd 是 Unix 绝对路径，同一判据直接复用。
pub(super) fn groupable_cwd(cwd: &str) -> bool {
    let trimmed = cwd.trim();
    !trimmed.is_empty() && (Path::new(trimmed).is_absolute() || trimmed.starts_with('/'))
}

/// 挂载形态的 cwd → 宿主盘路径串，非挂载形态原样返回。折叠本身在
/// [`crate::shell_detect::mounted_host_path`] 单点实现——左侧分组、本地文件树
/// 与 Git 视图（经 `TerminalView::local_cwd`）共用同一变换，两侧不会各折
/// 各的。折回宿主路径后：git 根探测、与本地 tab 的组键合并都自然成立；
/// `/home/…` 等来宾自有路径保持 Unix 串，组名仍取末级目录。
fn host_group_path(cwd: &str) -> String {
    let trimmed = cwd.trim();
    crate::shell_detect::mounted_host_path(trimmed)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| trimmed.to_owned())
}

/// 本地 pane 的项目身份：cwd →（组键根 + 当前分支）。两者都从 cwd 派生
/// （根走 stat 链、分支读 `.git/HEAD`），**不看标题协议**——fish 等无集成
/// 嵌套 shell 里标题静默，分组与分支 chip 仍跟着 cwd 走（用户 10-15 裁定：
/// 先拿 cwd，再由 cwd 取 git 状态；有则项目分组+分支，无则目录分组）。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LocalProject {
    pub(super) root: SharedString,
    /// 仓库根的当前分支；非仓库为 `None`。
    pub(super) branch: Option<SharedString>,
}

/// 侧栏列表的一行：分区头、主机头、项目组头或条目（tab / 最近会话）。
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SidebarRow {
    /// 「当前任务」分区头（列表首行）。
    TabsSectionHeader {
        count: usize,
        collapsed: bool,
    },
    /// SSH 主机头（顶级容器，键 `ssh:{dest}`）：内含项目子组
    /// （[`SidebarRow::GroupHeader`]），不直接含条目。子组不缩进（用户
    /// 10-13 裁定），与本地组同一几何档位。
    HostHeader {
        key: SharedString,
        /// 主机显示名（SSH 目的地全串，或 profile 标签）。
        name: SharedString,
        tabs: usize,
        collapsed: bool,
    },
    GroupHeader {
        key: SharedString,
        name: SharedString,
        branch: Option<SharedString>,
        /// 组内 tab 数（组头右侧的小计数）。
        tabs: usize,
        collapsed: bool,
    },
    Tab {
        ix: usize,
    },
}

/// 把每个 tab 的分组归属聚合成分组行列表（本地一级组 + SSH「主机 → 项目」
/// 两级容器）。
///
/// 顺序：**顶层容器**（本地组与 SSH 主机）按首开序混排，本地兜底组全局垫底；
/// 主机内的项目子组按首开序，主机兜底子组（cwd 未知）垫底。首开序来自
/// [`NebulaWorkspace::note_project_group_open`] 打点（本地打组键、SSH 打
/// 主机键与子组键），点击切换任务不改任何输入，顺序自然稳定。未打点的组按
/// 首见序垫后。组内保持 `tabs` Vec 顺序——分组是呈现，不改变存储顺序，右键/
/// 键盘等按下标寻址的路径全部不受影响。
pub(super) fn grouped_rows(
    infos: &[TabGroupInfo],
    open_order: &HashMap<SharedString, u32>,
    collapsed: &HashMap<SharedString, bool>,
) -> Vec<SidebarRow> {
    struct Group {
        info: TabGroupInfo,
        tabs: Vec<usize>,
        seen: usize,
    }
    /// 顶层容器：本地组各自成项；SSH 子组挂进主机桶。
    enum Top {
        Local(Group),
        Host { host: SharedString, destination: SharedString, groups: Vec<Group>, seen: usize },
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (ix, info) in infos.iter().enumerate() {
        match index.get(info.key.as_ref()) {
            Some(&at) => {
                let group = &mut groups[at];
                group.tabs.push(ix);
                if group.info.branch.is_none() {
                    group.info.branch = info.branch.clone();
                }
            },
            None => {
                index.insert(info.key.as_ref(), groups.len());
                groups.push(Group { info: info.clone(), tabs: vec![ix], seen: groups.len() });
            },
        }
    }
    let mut tops: Vec<Top> = Vec::new();
    let mut top_index: HashMap<SharedString, usize> = HashMap::new();
    for group in groups {
        let top_key = group.info.host.as_ref().map_or_else(
            || group.info.key.clone(),
            |destination| super::task_groups_ssh::host_key(destination),
        );
        match top_index.get(&top_key) {
            Some(&at) => match &mut tops[at] {
                Top::Host { groups: children, .. } => children.push(group),
                // 同一 top 键不可能既是本地组又是主机：主机键带 `ssh:` 前缀。
                Top::Local(_) => unreachable!("host keys never collide with local group keys"),
            },
            None => {
                top_index.insert(top_key.clone(), tops.len());
                match group.info.host.clone() {
                    Some(destination) => tops.push(Top::Host {
                        host: top_key,
                        destination,
                        groups: vec![group],
                        seen: tops.len(),
                    }),
                    None => tops.push(Top::Local(group)),
                }
            },
        }
    }
    let first_open = |key: &SharedString| open_order.get(key).copied().unwrap_or(u32::MAX);
    // 主机内子组排序：兜底子组垫底，其余按首开序（打点键即子组键）。
    for top in &mut tops {
        if let Top::Host { destination, groups, .. } = top {
            let fallback = super::task_groups_ssh::fallback_child_key(destination);
            groups.sort_by(|a, b| {
                let rank = |group: &Group| (group.info.key == fallback) as u8;
                rank(a)
                    .cmp(&rank(b))
                    .then(first_open(&a.info.key).cmp(&first_open(&b.info.key)))
                    .then(a.seen.cmp(&b.seen))
            });
        }
    }
    // 顶层排序键：本地组用组键、SSH 主机用主机键（都是首开序打点键）。
    // 闭包写不出「返回值借用参数」的生命周期，用嵌套 fn 的省略规则。
    fn top_key(top: &Top) -> &SharedString {
        match top {
            Top::Local(group) => &group.info.key,
            Top::Host { host, .. } => host,
        }
    }
    tops.sort_by(|a, b| {
        let rank = |top: &Top| match top {
            // 本地兜底组全局垫底；SSH 主机与普通本地组同场按首开序混排。
            Top::Local(group) if group.info.key == WORKSPACE_GROUP_KEY => 1,
            _ => 0,
        };
        // 未打点容器的回落序：首见序（本地组=组首见、主机=主机首见，都随
        // infos 顺序单调递增）。
        let seen = |top: &Top| match top {
            Top::Local(group) => group.seen,
            Top::Host { seen, .. } => *seen,
        };
        rank(a)
            .cmp(&rank(b))
            .then(first_open(top_key(a)).cmp(&first_open(top_key(b))))
            .then(seen(a).cmp(&seen(b)))
    });
    let mut rows = Vec::new();
    for top in &tops {
        match top {
            Top::Local(group) => {
                let collapsed = collapsed.get(&group.info.key).copied().unwrap_or(false);
                rows.push(SidebarRow::GroupHeader {
                    key: group.info.key.clone(),
                    name: group.info.name.clone(),
                    branch: group.info.branch.clone(),
                    tabs: group.tabs.len(),
                    collapsed,
                });
                if !collapsed {
                    rows.extend(group.tabs.iter().map(|&ix| SidebarRow::Tab { ix }));
                }
            },
            Top::Host { host, destination, groups, .. } => {
                let host_collapsed = collapsed.get(host).copied().unwrap_or(false);
                rows.push(SidebarRow::HostHeader {
                    key: host.clone(),
                    name: destination.clone(),
                    tabs: groups.iter().map(|group| group.tabs.len()).sum(),
                    collapsed: host_collapsed,
                });
                if host_collapsed {
                    continue;
                }
                for group in groups {
                    let collapsed = collapsed.get(&group.info.key).copied().unwrap_or(false);
                    rows.push(SidebarRow::GroupHeader {
                        key: group.info.key.clone(),
                        name: group.info.name.clone(),
                        branch: group.info.branch.clone(),
                        tabs: group.tabs.len(),
                        collapsed,
                    });
                    if !collapsed {
                        rows.extend(group.tabs.iter().map(|&ix| SidebarRow::Tab { ix }));
                    }
                }
            },
        }
    }
    rows
}

pub(super) fn row_height(row: &SidebarRow, metrics: &RowMetrics) -> f32 {
    match row {
        SidebarRow::TabsSectionHeader { .. } => super::tab_scroll::tab_row_height(metrics.density),
        SidebarRow::GroupHeader { .. } | SidebarRow::HostHeader { .. } => GROUP_HEADER_H,
        SidebarRow::Tab { .. } => sidebar_entry_height(metrics),
    }
}

/// n 行总高：间距只在行之间，末行后不留（与旧均匀版 `rows_h` 同口径）。
pub(super) fn rows_total_height(heights: &[f32], gap: f32) -> f32 {
    if heights.is_empty() {
        0.0
    } else {
        heights.iter().sum::<f32>() + gap * (heights.len() - 1) as f32
    }
}

/// 视口能容纳几行。未量到高度（<= 1px）先展示全部，折叠动画的裁剪高度
/// 绝不写回窗口——否则混合行高下同样会把列表锁死成零行。
pub(super) fn visible_row_count(heights: &[f32], avail_h: f32, gap: f32) -> usize {
    if heights.is_empty() {
        return 0;
    }
    if avail_h <= 1.0 {
        return heights.len();
    }
    let mut show = heights.len();
    while show > 0 && rows_total_height(&heights[..show], gap) > avail_h + 0.5 {
        show -= 1;
    }
    show
}

/// 每行顶边的前缀偏移（含行间 gap）；第 0 行为 0。thumb 换算与 reveal 共用。
pub(super) fn row_top_offsets(heights: &[f32], gap: f32) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(heights.len());
    let mut top = 0.0;
    for height in heights {
        offsets.push(top);
        top += height + gap;
    }
    offsets
}

pub(super) fn max_row_scroll(len: usize, show: usize) -> usize {
    len.saturating_sub(show)
}

pub(super) fn clamp_row_scroll(scroll: usize, max: usize) -> usize {
    scroll.min(max)
}

pub(super) fn row_index_visible(ix: usize, scroll: usize, show: usize) -> bool {
    show > 0 && ix >= scroll && ix < scroll + show
}

/// 激活行滚进窗口：在窗口上方对齐窗口顶，在下方对齐窗口底。
pub(super) fn reveal_row(row_ix: usize, scroll: usize, show: usize) -> usize {
    if show == 0 {
        return 0;
    }
    if row_ix < scroll {
        row_ix
    } else if row_ix >= scroll + show {
        row_ix + 1 - show
    } else {
        scroll
    }
}

/// 条目副行文本：`摘要 · 14:05`。无任务标题时摘要回落「新会话」；非终端
/// tab 没有创建时刻，只剩摘要本身。
pub(super) fn task_subline_label(
    summary: Option<&str>,
    spawned_at: Option<std::time::SystemTime>,
    fallback: &str,
) -> String {
    let clock = spawned_at.map(|at| {
        chrono::DateTime::<chrono::Utc>::from(at)
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string()
    });
    match (summary, clock) {
        (Some(summary), Some(clock)) => format!("{summary} · {clock}"),
        (Some(summary), None) => summary.to_owned(),
        (None, Some(clock)) => format!("{fallback} · {clock}"),
        (None, None) => fallback.to_owned(),
    }
}

impl NebulaWorkspace {
    /// 行高派生输入（密度 + 当前界面字号）。
    pub(super) fn row_metrics(&self, cx: &App) -> RowMetrics {
        let label_px = cx
            .try_global::<crate::gpui_shell::config::Settings>()
            .map(|settings| settings.ui_font_size_px)
            .unwrap_or(15.0);
        RowMetrics::new(self.density, label_px)
    }

    /// 组键解析：cwd 首次出现时向上找一次 git 仓库根（纯 stat 链，与右键
    /// 菜单 `git_repository_root` 同一判据），此后每个提示符重读一次
    /// `.git/HEAD` 刷新分支（checkout 会换支）。根与分支都进缓存，render
    /// 只查内存。只允许在事件路径调用——render 需要时只查
    /// [`Self::project_group_cache`]。
    pub(super) fn resolve_project_group(
        &mut self,
        view: &gpui::Entity<crate::gpui_shell::terminal::view::TerminalView>,
        cx: &App,
    ) {
        let view = view.read(cx);
        if let Some(destination) = view.ssh_destination.as_deref() {
            // SSH 主机分组：主机键与项目子键都打首开点（排序输入）；子键派生
            // 与 render 共用 `ssh_group_info`，防两处漂移。远端组键不进
            // `project_group_cache`——远端路径与本地路径是两个键空间；远端
            // 仓库根由集成上报，集成哑掉（fish 等嵌套 shell）时由按 pane 的
            // `rev-parse` 探测补位，不碰本地缓存。
            // 远端项目身份探测（`remote_git.rs` 的按 pane rev-parse）由后续
            // 变更提供；探测缺席时 `ssh_group_info` 回落远端 cwd 键——与
            // 探测未命中的行为一致，不是降级路径。
            let info = super::task_groups_ssh::ssh_group_info(destination, &view, None);
            self.note_project_group_open(&super::task_groups_ssh::host_key(destination));
            self.note_project_group_open(&info.key);
            return;
        }
        let cwd = view.cwd.as_str();
        if !groupable_cwd(cwd) {
            return;
        }
        // 已有条目：根的向上探测只在首见时做一次（深目录无仓库时那条 stat 链
        // 不便宜），分支却可能被 checkout 换掉——逐次重读一次 `.git/HEAD`，
        // 一次文件读的量级，换来 fish 等无集成嵌套 shell 里 chip 不停在旧支。
        if let Some(entry) = self.project_group_cache.get_mut(cwd) {
            let root = std::path::PathBuf::from(entry.root.as_ref());
            entry.branch = crate::display::side_panel::git_head_branch(&root)
                .map(|branch| SharedString::from(branch));
            return;
        }
        // WSL automount（`/mnt/<盘>/…`）与宿主盘是同一工作树：折回
        // `X:\…` 再探测，让 WSL 里的 tab 与本地 tab 归同一组。
        let normalized = host_group_path(cwd);
        let key = if Path::new(&normalized).is_absolute() {
            crate::display::side_panel::git_repository_root(Path::new(&normalized))
                .map(|root| root.to_string_lossy().into_owned())
                .unwrap_or(normalized)
        } else {
            // 来宾自有路径（`/home/…`）：宿主读不到，不探测 git 根，以自身成组。
            normalized
        };
        let branch = crate::display::side_panel::git_head_branch(Path::new(&key));
        let entry =
            LocalProject { root: SharedString::from(key), branch: branch.map(SharedString::from) };
        self.note_project_group_open(&entry.root);
        self.project_group_cache.insert(cwd.to_owned(), entry);
    }

    /// 记录组键首次被观测的序号（项目首开序）。事件路径专用；render 只读
    /// [`Self::project_group_open_order`]。同一键幂等。
    pub(super) fn note_project_group_open(&mut self, key: &SharedString) {
        let next = self.project_group_open_order.len() as u32;
        self.project_group_open_order.entry(key.clone()).or_insert(next);
    }

    /// 单个 tab 的分组归属（render 路径，纯内存读）。缓存未命中时先用 cwd
    /// 自身成组——显示名恰是目录名，git 根解析回填后子目录自然并组。
    pub(super) fn tab_group_info(&self, ix: usize, cx: &App) -> TabGroupInfo {
        let workspace = || TabGroupInfo {
            host: None,
            key: WORKSPACE_GROUP_KEY.into(),
            name: WORKSPACE_GROUP_KEY.into(),
            branch: None,
        };
        let Some(view) = self.tabs.get(ix).and_then(super::WorkspaceTab::focused_view) else {
            return workspace();
        };
        let view = view.read(cx);
        if let Some(destination) = view.ssh_destination.as_deref() {
            return super::task_groups_ssh::ssh_group_info(destination, &view, None);
        }
        let cwd = view.cwd.as_str();
        if !groupable_cwd(cwd) {
            return workspace();
        }
        // 分组与分支 chip 都从 cwd 派生的缓存里读；标题协议的分支不再上
        // chip——它在无集成嵌套 shell（fish 等）里是旧仓库的遗物。
        let entry = self.project_group_cache.get(cwd);
        let key = entry
            .map(|entry| entry.root.clone())
            .unwrap_or_else(|| SharedString::from(host_group_path(cwd)));
        let name = Path::new(key.as_ref())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| key.to_string());
        let branch = entry.and_then(|entry| entry.branch.clone());
        TabGroupInfo { host: None, key, name: name.into(), branch }
    }

    /// 本帧的侧栏行模型。O(n·log n)（n = 本窗口 tab 数），所有输入都在内存。
    pub(super) fn sidebar_rows(&self, cx: &App) -> Vec<SidebarRow> {
        let infos: Vec<TabGroupInfo> =
            (0..self.tabs.len()).map(|ix| self.tab_group_info(ix, cx)).collect();
        grouped_rows(&infos, &self.project_group_open_order, &self.sidebar_group_collapsed)
    }

    /// 统一侧栏列表的行模型：两个分区头与其内容展开成**一个**序列，由单一
    /// 窗口化滚动容器消费。分区折叠只是行数变化，不再有独立的动画槽位。
    /// 例外：「最近任务」**折叠时**分区头不进列表，由侧栏根钉在滚动区底部
    /// （用户 10-10 裁定：收起即沉底，不占列表顶部位置）。
    pub(super) fn sidebar_list_rows(&self, cx: &App) -> Vec<SidebarRow> {
        let mut rows = vec![SidebarRow::TabsSectionHeader {
            count: self.tabs.len(),
            collapsed: self.tabs_section_collapsed,
        }];
        if !self.tabs_section_collapsed {
            rows.extend(self.sidebar_rows(cx));
        }
        rows
    }

    /// 组头行：文件夹图标 + 项目名 + 分支 chip + tab 数 + 折叠箭头。
    /// 行宽必须显式像素（同 tab 行的教训：容器 overflow_hidden 下 w_full
    /// 会回落 shrink-to-fit）。SSH 主机下的项目子组不缩进（用户 10-13
    /// 裁定），与本地组同几何：箭头中心落在 [`TREE_X_L1`]。
    pub(super) fn render_group_header(
        &self,
        row_w: f32,
        row_ix: usize,
        key: &SharedString,
        name: &SharedString,
        branch: &Option<SharedString>,
        tabs: usize,
        collapsed: bool,
        cx: &Context<Self>,
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
            language.text(crate::i18n::Message::SidebarExpandGroup)
        } else {
            language.text(crate::i18n::Message::SidebarCollapseGroup)
        };
        // 兜底组（非终端 tab / cwd 未上报）显示翻译名，不是裸键字符串。
        let display_name: SharedString = if name.as_ref() == WORKSPACE_GROUP_KEY {
            language.text(crate::i18n::Message::SidebarGroupWorkspace).into()
        } else {
            name.clone()
        };
        let toggle_key = key.clone();
        let count: SharedString = tabs.to_string().into();
        // 项目名固定「[名称]」形态 + 浅绿前景（参考图组头）。
        let name_text: SharedString = format!("[{display_name}]").into();
        let name_selector: SharedString = format!("sidebar-group-name-{key}").into();
        let chevron_selector: SharedString = format!("sidebar-group-chevron-{key}").into();
        let branch_selector: SharedString = format!("sidebar-group-branch-{key}").into();
        h_flex()
            .id(("sidebar-group", row_ix))
            .debug_selector(move || format!("sidebar-group-{key}"))
            .w(px(row_w))
            .h(px(GROUP_HEADER_H))
            .overflow_hidden()
            // 左内边距 8px：折叠箭头中心 16 = TREE_X_L1（组脊柱同线）。
            .pl(px(8.0))
            .pr(px(8.0))
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
            // 同条目行：click 合成链路依赖视图节点的 mouse_down listener
            // 参与 bubble 派发，去掉它组头的 on_click 不会触发。
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, _, _| {}),
            )
            // 折叠箭头固定最左槽（参考图组头第一列），不再钉在行尾。
            .child(
                h_flex()
                    .debug_selector(move || chevron_selector.to_string())
                    .w(px(GROUP_CHEVRON_SLOT_W))
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
                    .debug_selector(move || name_selector.to_string())
                    .min_w_0()
                    .max_w(px(row_w * 0.5))
                    .truncate()
                    .font_family(chrome_family)
                    .text_size(px(label_px * SIDEBAR_TITLE_SCALE))
                    .text_color(theme.success)
                    .child(name_text),
            )
            // 分支块整体在项目名右侧（间隔略大），图标与文字同一前景色。
            .when_some(branch.as_ref(), |row, branch| {
                row.child(
                    h_flex()
                        .debug_selector(move || branch_selector.to_string())
                        .flex_shrink_0()
                        .ml(px(10.0))
                        .max_w(px(row_w * 0.4))
                        .gap(px(3.0))
                        .items_center()
                        .text_color(theme.warning)
                        .child(
                            Icon::new(Icon::empty())
                                .path(crate::gpui_shell::assets::nav::GIT_BRANCH)
                                .size(px(GROUP_BRANCH_ICON_SIZE))
                                .text_color(theme.warning),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(label_px * SIDEBAR_TAG_SCALE))
                                .child(branch.clone()),
                        ),
                )
            })
            // 名称/分支之后补弹性空位，计数仍靠右。
            .child(div().flex_1())
            .when(tabs > 1, |row| {
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

    /// 折叠/展开一个项目分组。组行数随之变化，就地精确钳制滚动窗口。
    pub(super) fn toggle_sidebar_group(&mut self, key: SharedString, cx: &mut Context<Self>) {
        let collapsed = self.sidebar_group_collapsed.entry(key).or_insert(false);
        *collapsed = !*collapsed;
        // 行移位，右键菜单锚定的那一行不在原处了（同 `toggle_tabs_section`）。
        self.tab_menu = None;
        self.clamp_rows_after_group_toggle(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(key: &str) -> TabGroupInfo {
        TabGroupInfo { host: None, key: key.into(), name: key.into(), branch: None }
    }

    fn host_info(destination: &str, path: &str) -> TabGroupInfo {
        let key = format!("{HOST_PREFIX}{destination}{SEP}{path}");
        TabGroupInfo {
            host: Some(destination.into()),
            name: Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_owned())
                .into(),
            key: key.into(),
            branch: None,
        }
    }

    /// 测试侧的键构造捷径（与 task_groups_ssh 的拼接同式）。
    const HOST_PREFIX: &str = "ssh:";
    const SEP: char = '|';

    #[test]
    fn unix_style_cwds_are_groupable_and_mounted_drives_fold_to_host_paths() {
        // WSL/fish 经 OSC 7 上报的 Unix 路径必须能成组（is_absolute 在
        // Windows 上对 `/…` 判 false，不能作为唯一判据）。
        assert!(groupable_cwd("/mnt/d/src/pebrel"));
        assert!(groupable_cwd("/home/user/proj"));
        // Windows 盘符串只在 Windows 宿主上是绝对路径（`Path::is_absolute`
        // 按宿主判定）；POSIX 宿主的本地 pane 不会出现这种 cwd，判不可组
        // 落「工作区」兜底。
        if std::env::consts::OS == "windows" {
            assert!(groupable_cwd(r"D:\src\pebrel"));
        } else {
            assert!(!groupable_cwd(r"D:\src\pebrel"));
        }
        assert!(!groupable_cwd(""));
        assert!(!groupable_cwd("  "));
        assert!(!groupable_cwd("relative/junk"));
        // automount 折回宿主路径：盘符大写、分隔符反转，与本地 tab 的
        // cwd（`D:\…`）产生同一个组键。Cygwin 的 /cygdrive 挂载同规则。
        assert_eq!(host_group_path("/mnt/d/src/pebrel"), r"D:\src\pebrel");
        assert_eq!(host_group_path("/mnt/C/"), r"C:\");
        assert_eq!(host_group_path("/cygdrive/d/gopath/src/pebrel"), r"D:\gopath\src\pebrel");
        assert_eq!(host_group_path("/cygdrive/c"), r"C:\");
        // 非「单盘符首段」不是 automount，原样保留。
        assert_eq!(host_group_path("/mnt/share/file"), "/mnt/share/file");
        assert_eq!(host_group_path("/home/user/proj"), "/home/user/proj");
        assert_eq!(host_group_path(r"D:\x"), r"D:\x");
    }

    #[test]
    fn same_key_tabs_share_one_group_in_vec_order() {
        let infos = [info("D:/repo"), info("D:/other"), info("D:/repo")];
        let rows = grouped_rows(&infos, &HashMap::new(), &HashMap::new());
        assert_eq!(
            rows,
            vec![
                SidebarRow::GroupHeader {
                    key: "D:/repo".into(),
                    name: "D:/repo".into(),
                    branch: None,
                    tabs: 2,
                    collapsed: false
                },
                SidebarRow::Tab { ix: 0 },
                SidebarRow::Tab { ix: 2 },
                SidebarRow::GroupHeader {
                    key: "D:/other".into(),
                    name: "D:/other".into(),
                    branch: None,
                    tabs: 1,
                    collapsed: false
                },
                SidebarRow::Tab { ix: 1 },
            ]
        );
    }

    /// SSH 主机的两级呈现：主机头承载全部计数，项目子组各带
    /// 自己的条目；同一主机的子组之间保持首开序。
    #[test]
    fn ssh_tabs_nest_under_one_host_header_with_project_subgroups() {
        let infos = [
            host_info("build@10.0.0.7", "/srv/work/pebrel"),
            host_info("build@10.0.0.7", "/srv/work/pebrel"),
            host_info("build@10.0.0.7", "/home/user/other"),
            info("D:/local"),
        ];
        let rows = grouped_rows(&infos, &HashMap::new(), &HashMap::new());
        assert_eq!(
            rows,
            vec![
                SidebarRow::HostHeader {
                    key: "ssh:build@10.0.0.7".into(),
                    name: "build@10.0.0.7".into(),
                    tabs: 3,
                    collapsed: false
                },
                SidebarRow::GroupHeader {
                    key: "ssh:build@10.0.0.7|/srv/work/pebrel".into(),
                    name: "pebrel".into(),
                    branch: None,
                    tabs: 2,
                    collapsed: false,
                },
                SidebarRow::Tab { ix: 0 },
                SidebarRow::Tab { ix: 1 },
                SidebarRow::GroupHeader {
                    key: "ssh:build@10.0.0.7|/home/user/other".into(),
                    name: "other".into(),
                    branch: None,
                    tabs: 1,
                    collapsed: false,
                },
                SidebarRow::Tab { ix: 2 },
                SidebarRow::GroupHeader {
                    key: "D:/local".into(),
                    name: "D:/local".into(),
                    branch: None,
                    tabs: 1,
                    collapsed: false
                },
                SidebarRow::Tab { ix: 3 },
            ]
        );
    }

    /// 主机折叠只剩主机头；子组折叠只留子组头——两级共用同一张折叠表，
    /// 键前缀隔离。
    #[test]
    fn host_and_subgroup_collapse_independently() {
        let infos = [
            host_info("build@10.0.0.7", "/srv/pebrel"),
            host_info("build@10.0.0.7", "/home/other"),
        ];
        let host_collapsed = HashMap::from([("ssh:build@10.0.0.7".to_string().into(), true)]);
        assert_eq!(
            grouped_rows(&infos, &HashMap::new(), &host_collapsed),
            vec![SidebarRow::HostHeader {
                key: "ssh:build@10.0.0.7".into(),
                name: "build@10.0.0.7".into(),
                tabs: 2,
                collapsed: true
            }]
        );
        let subgroup_collapsed =
            HashMap::from([("ssh:build@10.0.0.7|/srv/pebrel".to_string().into(), true)]);
        let rows = grouped_rows(&infos, &HashMap::new(), &subgroup_collapsed);
        assert_eq!(
            rows,
            vec![
                SidebarRow::HostHeader {
                    key: "ssh:build@10.0.0.7".into(),
                    name: "build@10.0.0.7".into(),
                    tabs: 2,
                    collapsed: false
                },
                SidebarRow::GroupHeader {
                    key: "ssh:build@10.0.0.7|/srv/pebrel".into(),
                    name: "pebrel".into(),
                    branch: None,
                    tabs: 1,
                    collapsed: true,
                },
                SidebarRow::GroupHeader {
                    key: "ssh:build@10.0.0.7|/home/other".into(),
                    name: "other".into(),
                    branch: None,
                    tabs: 1,
                    collapsed: false,
                },
                SidebarRow::Tab { ix: 1 },
            ]
        );
    }

    /// 顶层排序：SSH 主机与本地组按首开序混排（主机键、本地组键各自打点），
    /// 本地兜底组仍全局垫底；主机内的兜底子组（cwd 未知）垫在主机内底部。
    #[test]
    fn hosts_mix_with_local_groups_by_first_open_and_fallbacks_sink() {
        let infos = [
            info(WORKSPACE_GROUP_KEY),
            info("D:/local"),
            host_info("build@10.0.0.7", "workspace"),
            host_info("build@10.0.0.7", "/srv/pebrel"),
        ];
        let open_order = HashMap::from([
            ("ssh:build@10.0.0.7".to_string().into(), 0u32),
            ("D:/local".to_string().into(), 1u32),
        ]);
        let rows = grouped_rows(&infos, &open_order, &HashMap::new());
        let kinds: Vec<String> = rows
            .iter()
            .map(|row| match row {
                SidebarRow::HostHeader { key, .. } => format!("host:{key}"),
                SidebarRow::GroupHeader { key, .. } => format!("group:{key}"),
                SidebarRow::Tab { ix } => format!("tab{ix}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "host:ssh:build@10.0.0.7".to_owned(),
                // 主机内：项目子组在前，兜底子组（cwd 未知）垫底。
                "group:ssh:build@10.0.0.7|/srv/pebrel".to_owned(),
                "tab3".to_owned(),
                "group:ssh:build@10.0.0.7|workspace".to_owned(),
                "tab2".to_owned(),
                "group:D:/local".to_owned(),
                "tab1".to_owned(),
                // 本地兜底组全局垫底。
                format!("group:{WORKSPACE_GROUP_KEY}"),
                "tab0".to_owned(),
            ]
        );
    }

    #[test]
    fn groups_follow_first_open_order_and_workspace_stays_last() {
        let infos = [info(WORKSPACE_GROUP_KEY), info("D:/a"), info("D:/b")];
        let open_order =
            HashMap::from([("D:/a".to_string().into(), 1u32), ("D:/b".to_string().into(), 0u32)]);
        // 首开序 b(0) → a(1)；兜底组恒垫底，与打点无关。
        let rows = grouped_rows(&infos, &open_order, &HashMap::new());
        let keys: Vec<&SharedString> = rows
            .iter()
            .filter_map(|row| match row {
                SidebarRow::GroupHeader { key, .. } => Some(key),
                _ => None,
            })
            .collect();
        let (a, b, ws): (SharedString, SharedString, SharedString) =
            ("D:/a".into(), "D:/b".into(), WORKSPACE_GROUP_KEY.into());
        assert_eq!(keys, vec![&b, &a, &ws]);
    }

    #[test]
    fn unnoted_groups_sink_by_first_seen_order() {
        // 无打点（git 根解析前）的临时组按首见序垫在打点组之后。
        let infos = [info("D:/b"), info("D:/tmp"), info("D:/a")];
        let open_order =
            HashMap::from([("D:/a".to_string().into(), 0u32), ("D:/b".to_string().into(), 1u32)]);
        let rows = grouped_rows(&infos, &open_order, &HashMap::new());
        let keys: Vec<String> = rows
            .iter()
            .filter_map(|row| match row {
                SidebarRow::GroupHeader { key, .. } => Some(key.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(keys, vec!["D:/a".to_string(), "D:/b".to_string(), "D:/tmp".to_string()]);
    }

    #[test]
    fn collapsed_group_keeps_only_its_header() {
        let infos = [info("D:/repo"), info("D:/repo")];
        let collapsed = HashMap::from([("D:/repo".to_string().into(), true)]);
        let rows = grouped_rows(&infos, &HashMap::new(), &collapsed);
        assert_eq!(
            rows,
            vec![SidebarRow::GroupHeader {
                key: "D:/repo".into(),
                name: "D:/repo".into(),
                branch: None,
                tabs: 2,
                collapsed: true,
            }]
        );
    }

    #[test]
    fn first_branch_seen_wins_for_the_group_chip() {
        let mut first = info("D:/repo");
        first.branch = Some("main".into());
        let mut second = info("D:/repo");
        second.branch = Some("dev".into());
        let rows = grouped_rows(&[first, second], &HashMap::new(), &HashMap::new());
        match &rows[0] {
            SidebarRow::GroupHeader { branch, .. } => {
                assert_eq!(branch.as_deref(), Some("main"));
            },
            other => panic!("expected a group header, got {other:?}"),
        }
    }

    #[test]
    fn mixed_pitch_window_fits_whole_rows_only() {
        let heights = [26.0, 46.0, 46.0, 26.0, 46.0];
        let gap = 4.0;
        // 26+4+46+4+46 = 126 > 100；前三行 26+4+46+4+26 = 106 > 100；前二 76 <= 100。
        assert_eq!(visible_row_count(&heights, 100.0, gap), 2);
        // 未量到高度先全显；空列表为 0。
        assert_eq!(visible_row_count(&heights, 0.0, gap), 5);
        assert_eq!(visible_row_count(&[], 100.0, gap), 0);
    }

    #[test]
    fn top_offsets_include_inter_row_gaps_only() {
        let offsets = row_top_offsets(&[26.0, 46.0], 4.0);
        assert_eq!(offsets, vec![0.0, 30.0]);
        assert_eq!(rows_total_height(&[26.0, 46.0], 4.0), 76.0);
        assert_eq!(rows_total_height(&[], 4.0), 0.0);
    }

    #[test]
    fn reveal_and_clamp_follow_row_indices() {
        assert_eq!(max_row_scroll(5, 2), 3);
        assert_eq!(clamp_row_scroll(9, 3), 3);
        assert!(!row_index_visible(0, 2, 2));
        assert!(row_index_visible(3, 2, 2));
        assert_eq!(reveal_row(0, 2, 2), 0);
        assert_eq!(reveal_row(2, 2, 2), 2);
        assert_eq!(reveal_row(4, 0, 2), 3);
    }

    fn guide_rows() -> Vec<SidebarRow> {
        vec![
            SidebarRow::TabsSectionHeader { count: 3, collapsed: false },
            SidebarRow::GroupHeader {
                key: "D:/repo".into(),
                name: "D:/repo".into(),
                branch: None,
                tabs: 2,
                collapsed: false,
            },
            SidebarRow::Tab { ix: 0 },
            SidebarRow::Tab { ix: 1 },
        ]
    }

    #[test]
    fn tree_guides_form_two_levels_of_elbows() {
        let metrics = RowMetrics::new(DensityName::Standard, 15.0);
        let rows = guide_rows();
        let heights: Vec<f32> = rows.iter().map(|row| row_height(row, &metrics)).collect();
        let offsets = row_top_offsets(&heights, 8.0);
        let entry_h = sidebar_entry_height(&metrics);
        let line_h = subline_line_height(&metrics);
        let elbow_dy = headline_band(&metrics) * 0.5;
        let summary_dy = entry_h - 2.0 - 2.0 * line_h + line_h * 0.5;
        let guides = tree_guides(&rows, &offsets, 0.0, f32::INFINITY, &metrics);
        let find = |kind: &str, tag: usize| {
            guides
                .iter()
                .find(|guide| guide.kind == kind && guide.tag == tag)
                .unwrap_or_else(|| panic!("missing {kind} guide"))
        };
        // 一级竖线：组头中心 → 末子任务肘形（└ 收口），粗 2px。
        let l1 = find("l1", 0);
        assert_eq!(l1.bounds.0, TREE_X_L1);
        assert_eq!(l1.bounds.2, GUIDE_THICKNESS);
        assert_eq!(l1.bounds.1, offsets[1] + GROUP_HEADER_H * 0.5);
        assert_eq!(l1.bounds.1 + l1.bounds.3, offsets[3] + elbow_dy);
        // 任务肘形：横臂从一级竖线直达图标列（T 字横杠）。
        assert_eq!(guides.iter().filter(|g| g.kind == "elbow").count(), 2);
        let elbow0 = find("elbow", 0);
        assert_eq!(elbow0.bounds.0, TREE_X_L1);
        assert_eq!(elbow0.bounds.2, tree_icon_left() - TREE_X_L1);
        assert_eq!(elbow0.bounds.3, GUIDE_THICKNESS);
        assert_eq!(elbow0.bounds.1 + GUIDE_THICKNESS * 0.5, offsets[2] + elbow_dy);
        // 摘要悬挂竖线：任务肘形交点 → 摘要肘形；摘要肘形横臂指向摘要首行
        // （终点在摘要文字左缘左侧 4px）。
        assert_eq!(guides.iter().filter(|g| g.kind == "sumv").count(), 2);
        assert_eq!(guides.iter().filter(|g| g.kind == "sume").count(), 2);
        let sumv0 = find("sumv", 0);
        assert_eq!(sumv0.bounds.0, TREE_X_L2);
        assert_eq!(sumv0.bounds.1, offsets[2] + elbow_dy);
        assert_eq!(sumv0.bounds.1 + sumv0.bounds.3, offsets[2] + summary_dy);
        let sume0 = find("sume", 0);
        assert_eq!(sume0.bounds.0, TREE_X_L2);
        assert_eq!(sume0.bounds.2, tree_text_left() - 4.0 - TREE_X_L2);
        assert_eq!(sume0.bounds.1 + GUIDE_THICKNESS * 0.5, offsets[2] + summary_dy);
    }

    #[test]
    fn tree_guides_clip_to_the_viewport_and_skip_collapsed_groups() {
        let metrics = RowMetrics::new(DensityName::Standard, 15.0);
        let rows = guide_rows();
        let heights: Vec<f32> = rows.iter().map(|row| row_height(row, &metrics)).collect();
        let offsets = row_top_offsets(&heights, 8.0);
        // 视口只露出一级竖线的前 5px：底被夹到视口高，仍存在。竖线顶在
        // offsets[1]+13=55、底在末子肘形 163，视口 60 → 段 [55, 60]。
        let tiny = tree_guides(&rows, &offsets, 0.0, 60.0, &metrics);
        let l1 = tiny.iter().find(|guide| guide.kind == "l1").expect("clipped l1");
        assert_eq!(l1.bounds.1, 55.0);
        assert_eq!(l1.bounds.1 + l1.bounds.3, 60.0);
        // 组头滚出窗口（first_visible_top 下移）：竖线顶按视口夹取，不出现
        // 负坐标段。
        let scrolled = tree_guides(&rows, &offsets, offsets[2], f32::INFINITY, &metrics);
        assert!(scrolled.iter().all(|guide| guide.bounds.1 >= 0.0));
        // 折叠组没有子行 → 不产出任何线段。
        let mut collapsed_rows = guide_rows();
        collapsed_rows.truncate(2);
        collapsed_rows[1] = SidebarRow::GroupHeader {
            key: "D:/repo".into(),
            name: "D:/repo".into(),
            branch: None,
            tabs: 2,
            collapsed: true,
        };
        let collapsed_heights: Vec<f32> =
            collapsed_rows.iter().map(|row| row_height(row, &metrics)).collect();
        let collapsed_offsets = row_top_offsets(&collapsed_heights, 8.0);
        assert!(
            tree_guides(&collapsed_rows, &collapsed_offsets, 0.0, f32::INFINITY, &metrics)
                .is_empty()
        );
    }
    /// SSH 主机块的树线：主机头是纯容器行，不产出主机级线段（hostv/hoste
    /// 已按用户 10-13 裁定移除——横臂终点压住子组折叠箭头槽）；子组也不
    /// 缩进，任务肘形与本地组同几何产出；主机折叠后整块无线段（行模型已
    /// 无子行）。
    #[test]
    fn tree_guides_skip_host_headers_but_keep_nested_project_elbows() {
        let metrics = RowMetrics::new(DensityName::Standard, 15.0);
        let rows = vec![
            SidebarRow::TabsSectionHeader { count: 2, collapsed: false },
            SidebarRow::HostHeader {
                key: "ssh:build@10.0.0.7".into(),
                name: "build@10.0.0.7".into(),
                tabs: 2,
                collapsed: false,
            },
            SidebarRow::GroupHeader {
                key: "ssh:build@10.0.0.7|/srv/pebrel".into(),
                name: "pebrel".into(),
                branch: None,
                tabs: 2,
                collapsed: false,
            },
            SidebarRow::Tab { ix: 0 },
            SidebarRow::Tab { ix: 1 },
        ];
        let heights: Vec<f32> = rows.iter().map(|row| row_height(row, &metrics)).collect();
        let offsets = row_top_offsets(&heights, 8.0);
        let guides = tree_guides(&rows, &offsets, 0.0, f32::INFINITY, &metrics);
        // 主机级线段不存在；主机头的存在也不吞掉子组线段。
        assert!(
            guides.iter().all(|guide| guide.kind != "hostv" && guide.kind != "hoste"),
            "host-level guides were removed by the 10-13 ruling"
        );
        // 子组不缩进：l1 / 任务肘形 / 摘要线全部落在本地档位。
        let l1 = guides.iter().find(|g| g.kind == "l1").unwrap();
        assert_eq!(l1.bounds.0, TREE_X_L1);
        let elbow0 = guides.iter().find(|g| g.kind == "elbow" && g.tag == 0).unwrap();
        assert_eq!(elbow0.bounds.0, TREE_X_L1);
        assert_eq!(elbow0.bounds.0 + elbow0.bounds.2, tree_icon_left());
        let sumv0 = guides.iter().find(|g| g.kind == "sumv" && g.tag == 0).unwrap();
        assert_eq!(sumv0.bounds.0, TREE_X_L2);
        // 主机折叠：整块不产线段。
        let mut collapsed = rows.clone();
        collapsed.truncate(2);
        collapsed[1] = SidebarRow::HostHeader {
            key: "ssh:build@10.0.0.7".into(),
            name: "build@10.0.0.7".into(),
            tabs: 2,
            collapsed: true,
        };
        let collapsed_heights: Vec<f32> =
            collapsed.iter().map(|row| row_height(row, &metrics)).collect();
        let collapsed_offsets = row_top_offsets(&collapsed_heights, 8.0);
        assert!(
            tree_guides(&collapsed, &collapsed_offsets, 0.0, f32::INFINITY, &metrics).is_empty()
        );
    }
}
