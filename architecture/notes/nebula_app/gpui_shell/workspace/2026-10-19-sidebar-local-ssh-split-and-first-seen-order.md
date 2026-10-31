# 侧栏本地/SSH 上下分区与首见序排序（首开序打点退役）

## Status

Accepted

## Context

侧栏「当前任务」此前把本地项目组与 SSH 主机按首开序混排在同一
顶层序列（`grouped_rows` 的 `Top` 混排 + `project_group_open_order` 打点）。
用户 10-19 裁定：本地任务与 SSH 主机是两类工作重心，分成上下两部分
（本地在上、SSH 在下）；任务打开顺序记录（首开序打点）不再需要，排序
使用默认即可。同一批裁定还包括：SSH 主机头图标换掉 Nerd Font 字位、
任务名肘形横臂与图标之间的间距与摘要行对齐。

## Evidence

`nebula_app/src/gpui_shell/workspace/task_groups.rs` 的
`grouped_rows`（改动前的 `first_open` 排序链与 `Top` 混排）、
`workspace.rs` 的 `project_group_open_order` 字段；
`task_groups_ssh.rs` 的 `\u{f0716}` 字位渲染（缺 Nerd Font 的环境里
渲染近似「H+」，用户报）；`tree_guides` 的 elbow 终点
`tree_icon_left()`（零间距贴住图标）。

## Decision

1. 分区形态：**单一滚动列表内分区**（用户在两种形态中选定）——本地段
   在上、SSH 段在下，`SectionDivider`（非交互 hairline 分隔行，仅两段
   都有内容时出现）+ `SshSectionHeader`（「SSH 主机」子分区头，
   `sidebar.ssh_section` 词条，en-US/zh-CN）标记 SSH 段。
2. SSH 段子分区头可独立折叠（`ssh_section_collapsed`，内存态）：收起
   全部主机块只留子头。**只要有 SSH 内容它就出现**（纯 SSH 场景它是
   唯一的段折叠开关，随段折叠一起消失会造成无法展开的死锁）；分隔行
   才是仅在两段都非空时出现。
3. 「当前任务」总折叠沿用既有 `tabs_section_collapsed`：折叠收起本地 +
   SSH 全部进行中任务（用户同一批裁定的要求，现状语义已满足）。
4. 排序回落**首见序**（组/主机在 `infos` 中首次出现 ≈ 每个项目第一个
   tab 的创建顺序）：本地兜底组 `workspace` 垫本地段底（从「全局垫底」
   收窄为「本地段内垫底」——分区的自然结果）、主机内兜底子组垫主机底、
   组内条目保持 `tabs` Vec 顺序（不变）。
5. 首开序打点整体退役：删除 `project_group_open_order` 字段、
   `note_project_group_open` 及其全部调用（SSH 主机键/子组键/本地组键）。
   `resolve_project_group` 的本地 git 根探测缓存职责保留；其 SSH 分支
   只剩纯计算，直接早退（远端身份探测在 `refresh_remote_project` 独立
   进行）。
6. 主机头图标：Nerd Font 字位 U+F0716 → 矢量图标 `IconName::Network`
   （`xsmall`，与 ssh_dialog 的 SSH 图标同一语义；宽度槽 `TAB_LABEL_ICON_W`
   不变）。pane_header 的同字位不在本次范围。
7. 任务名肘形（elbow）横臂终点 `tree_icon_left()` → `tree_icon_left() - 4.0`：
   与摘要肘形（sume）的 4px 呼吸间距一致，不再贴住条目图标。

## Rejected alternatives

- 两个独立滚动区域（各自滚动条/分隔条）：需要把单份的窗口化状态
  （`tabs_viewport_h`/`tabs_scroll`/`tabs_list_origin`）与滚动条数学拆成
  两套，用户在形态确认时已否决。
- 仅加分隔行、不加「SSH 主机」子分区头：弱于用户确认的视觉，且纯 SSH
  场景没有段身份与段开关。
- 保留打点但只用于段内排序：多一套仅为排序存在的可变状态，收益不抵
  复杂度（用户明示「使用默认就行」）。

## Consequences

混排场景的视觉顺序变化是需求本体；本地兜底组从全局垫底
（SSH 主机之后）变为本地段内垫底（SSH 段之前）。「未打点组垫后」概念
随打点消亡——所有组一律首见序就位。`SidebarRow` 新增两个变体，
`row_height` 相应加臂（`SECTION_DIVIDER_H = 14.0`、子头用分区头行高），
前缀和窗口化自动适配。

## Validation

`task_groups.rs` 单测重写/新增
（`locals_precede_ssh_hosts_with_a_divider_and_fallbacks_sink`、
`groups_follow_first_seen_order_and_workspace_stays_last`、
`ssh_section_collapse_keeps_only_its_header_and_locals_have_none`、
`ssh_tabs_nest_under_one_host_header_with_project_subgroups` 等 21 个）；
`workspace/tests.rs` 的 `tabs_section_collapse_hides_local_and_ssh_rows`
（总折叠回归）；`settings_navigation/ui_tests.rs` 肘形断言更新为
`tree_icon_left() - 4.0`。手动验收：本地+SSH 混开分区显示、仅一段无
divider、总折叠/段折叠独立、Network 图标、肘形间距肉眼核对。

## Supersedes

`2026-10-12-ssh-host-project-groups.md` 的 Decision 2（打点
键空间）与 Decision 4（首开序混排排序）中的排序部分；该 note 的键派生、
树线、命名决策继续有效（旧 note 已加回指指针）。

## Revisit when

若未来需要「最近使用」语义的任务排序，应基于显式的用户
动作事件（激活切换）重新设计并全量评估，而不是复活首开序打点。
