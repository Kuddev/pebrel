# SSH 主机两级分组（侧栏统一行模型）

## Status

Accepted
- Superseded by: Decision 2（打点键空间）与 Decision 4（首开序混排排序）
的排序部分由
[`2026-10-19-sidebar-local-ssh-split-and-first-seen-order.md`](2026-10-19-sidebar-local-ssh-split-and-first-seen-order.md)
取代（本地/SSH 分区 + 首见序，打点退役）；其余决策继续有效。

## Context

侧栏「当前任务」此前把 SSH pane 按目的地串拍成**扁平**一级组
（`tab_group_info` 直接 `key = ssh_destination`），同一主机上不同项目的任务
混在一个组里，也没有「主机」这一层身份；本地 pane 的项目/目录分组
（git 根优先、否则 cwd、兜底「工作区」）无法对远端复用。

## Evidence

`nebula_app/src/gpui_shell/workspace/task_groups.rs` 的
`tab_group_info`/`grouped_rows`（改动前的 SSH 分支）；远端 cwd/branch 经
OSC 7 与 `NEBULA|` 标题已可用（`res/shell/bashrc`/`zshrc` precmd），远端
仓库根经 `pebrel_git_root` UserVar 上报（见
`architecture/notes/nebula_app/platform/2026-10-12-git-root-user-var.md`）。

## Decision

1. `SidebarRow` 增加 `HostHeader`（主机顶级容器，键 `ssh:{destination}`），
   项目子组与条目带 `depth`（本地 0 / SSH 1）；`TabGroupInfo` 增加 `host`。
2. 键名空间：主机键 `ssh:{dest}`、子组键 `ssh:{dest}|{git_root|cwd|workspace}`，
   折叠表与首开序打点与本地组、`recent:` 键共用同一张表，前缀隔离。
3. 远端组键**不进** `project_group_cache`：远端 `/home/x` 与本地 `/home/x`
   同串会串组，且远端仓库根由集成直接上报、无需（也不能）本地 stat 探测。
   派生逻辑收在 `task_groups_ssh::ssh_group_info`，事件路径打点与 render
   共用一份防漂移。
4. 顶层排序：SSH 主机与本地组按首开序混排，本地兜底组全局垫底；主机内
   项目子组按首开序、主机兜底子组垫底。未打点容器按首见序回落。
5. 树线：主机头是纯容器行，**不画主机级树线**（`hostv`/`hoste` 曾实现、
   用户 10-13 裁定移除，见 Rejected）；子组及其条目**不缩进**（同日第二
   项裁定，depth 机制整体删除），与本地组共用同一套脊柱 x，本地组的既有
   线段几何逐点不变。
6. 主机头命名「SSH {目的地}」：typed 占位符词条 `sidebar.ssh_host`
   （en-US/zh-CN，其余语言回退英文），与 `settings.network.route.ssh_jump`
   （"SSH {target}"）同一先例；SSH 是技术缩写，各语言词条同形。
7. SSH 聚焦任务的「新建终端」（侧栏 + / Ctrl+Shift+T / 面板 NewTab 单一
   漏斗 `add_terminal`）继承同一主机与当前远端任务目录：纯决策函数
   `focused_ssh_origin` → 既有 `add_ssh_terminal_at`；「启动目录」是本地
   路径，对 SSH 任务不适用，SSH 分支优先。cwd 未上报时仍继承主机，目录
   回落远端默认。

## Rejected alternatives

- 组键只用远端 cwd（不引入主机层）：同仓库兄弟目录不并组，与本地分组
  语义不一致；也没有「主机」这一用户心智中的容器。
- 主机级树线（`hostv` 主机竖线 + `hoste` 横臂连到子组头中心）：`hoste`
  终点 `spine_x(1)` 与子组折叠箭头中心重合，2px 线段直接压住箭头槽
  （用户 10-13 报遮挡后移除）；主机身份由「SSH {目的地}」命名承载。
- 子组缩进一档（`depth: u8` 随行模型携带，曾实现）：主机级树线移除后
  缩进失去对齐锚点、白占横向空间（用户 10-13 同日裁定不缩进）；depth
  机制随之整体删除，不留恒 0 的死字段。
- 远端 git 根用 SSH exec 探测（`git rev-parse --show-toplevel` 走 exec
  channel）：每次 cd 一次远端往返，慢链路上不可接受；precmd 已经为分支
  调了一次 rev-parse，合并取根是零增量。
- 远端组键进 `project_group_cache`（cwd→键）：本地/远端路径字符串撞车。

## Consequences

typed-ssh（本地 pane 里手敲 `ssh`，Windows wrapper）没有
`ssh_destination`，不进主机分组——需求限定原生 SSH 任务，已知边界。
远端无集成（无 python3 等）时 cwd 未知，落主机兜底子组「工作区」，
与本地 cwd 未上报的降级形态一致。`sidebar_group_collapsed` 是内存态，
键方案变更无迁移成本。

## Validation

`task_groups` 分组/排序/折叠/树线单测（含
`ssh_tabs_nest_under_one_host_header_with_project_subgroups`、
`hosts_mix_with_local_groups_by_first_open_and_fallbacks_sink`、
`tree_guides_skip_host_headers_but_keep_nested_project_elbows`）；
`task_groups_ssh` 键派生单测；新建终端继承见
`new_terminal_from_an_ssh_task_inherits_host_and_remote_cwd`（纯决策
函数；`add_ssh_terminal_at` 会写真实 `saved_hosts`，端到端不在单测里跑）。

## Supersedes

None

## Revisit when

远端集成开始上报更细的项目身份（如 worktree），或「最近任务」
分区需要同一套主机层级时。
