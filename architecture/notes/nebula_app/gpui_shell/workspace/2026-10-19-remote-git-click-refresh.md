# 远端 Git 面板的点击沿刷新（开抽屉/切视图强制重拉）

## Status

Accepted

## Context

统一 pane cwd 之后（`2026-10-15-unified-pane-cwd-source.md`），
用户 10-19 报障：AI agent 在同一 cwd 下修改代码后，右侧抽屉的远端 Git
面板不显示新的修改记录，要切换任务再切回来才能看到。本地面板无此问题
——`SidePanel::toggle` 在开抽屉/切 Files↔Git 时置 `needs_refresh`
（`display/side_panel/mod.rs`），远端没有同款钩子。

## Evidence

`route_remote_git`（`remote_git.rs`）的重拉条件只有两个：聚焦
pane/destination 变化触发 `reset()`、仓库根字符串变化。agent 改文件不
动 pane/destination/root 任何一个，条件失灵；`toggle_side_panel`
（`details_panel.rs`）与 `select_side_panel_view`（`workspace.rs`）完全
不碰 `remote_git`；抽屉关闭期间 `render_remote_git` 不被调用，`RemoteGit`
的旧快照原样存活。切任务有效是因为 active 变 → `focused_remote_pane`
变 → `reset()` 清空 root → root 变化判据重新成立。

## Decision

1. 钩子形态：**事件路径置位 + 路由消费**——`wake_remote_git_refresh`
   （抽屉开且 Git 视图时 `request_refresh()` 幂等置位）挂在
   `toggle_side_panel`（关闭早退之后，只有打开路径走到）与
   `select_side_panel_view`（view 切换分支；转发路径经
   `toggle_side_panel`，两处互斥不叠加）；`route_remote_git` 在
   `!loading && !op_running` 的空闲帧消费标志并 `refresh_remote_git`。
   与本地 `needs_refresh`（事件置位、`sync_at` 门控消费）同构。
2. 为什么不挂路由可见性沿：`route_remote_git` 只在「抽屉开 + Git 视图 +
   remote」时被调用，抽屉关闭期它不运行，观测不到「不可见 → 可见」的
   沿；要观测就得在别处记账，等价于把钩子挪回事件路径。
3. 为什么不在事件路径直接刷：`refresh_remote_git` 需要 focused
   pane/destination/root 决策，那是 `route_remote_git` 的单一职责
   （attach_remote_browser 先例：纯内存比对通过后才动网络）；事件路径
   复制这套决策会造成两处漂移。
4. 等待语义：在途拉取（loading）或写操作（op_running）期间标志保持
   置位，收尾后下一帧补拉——恰好覆盖「在途时 agent 又改了文件」；
   root 变化重拉已覆盖请求时直接清标志，不发第二条命令。pane/
   destination 切换路径 `reset()` 清位。迟到响应由既有 generation+pane
   双校验丢弃。
5. 成本边界：每次点击至多 1 条 `git -C <root> status --porcelain=v2
   --branch -z`（池化已认证连接上的一个 exec channel，无新握手；8s
   预算 + loading UI 兜底），极端在途衔接上界 2 条。`refresh_remote_git`
   是 `cx.spawn` 异步，事件/渲染路径只翻布尔 + notify，零同步等待，
   符合「渲染回调不得同步等待子进程/网络」的工程约束。

## Rejected alternatives

- 周期轮询远端 git 状态：`2026-10-15-unified-pane-cwd-source.md`
  Decision 10 已拒绝（每 pane 每数秒一条 exec 的流量换不来）；本地侧
  2026-08-21 也已移除 4s stale 轮询（`side_panel/mod.rs` 裁定注释）。
  点击沿是离散事件，不在「周期探测」的否定范围。
- 抽屉打开期间 100ms 轮询里做远端刷新：该循环对远端 pane 早退
  （`remote_browser.active()` 分支），且轮询频率远高于点击频率，等于
  变相周期刷新。

## Consequences

远端 Git 面板获得与本地面板对齐的「打开即新鲜」体验；
「同一 cwd 下文件变化不自动刷新」的既有语义保持不变（仍由刷新按钮、
写操作收尾与本次新增的点击沿覆盖）。`RemoteGit` 增加
`refresh_requested` 状态（`pub(super)` 供测试断言）。

## Validation

`remote_git.rs`
`refresh_request_is_idempotent_and_reset_clears_it`；`workspace/tests.rs`
`git_drawer_click_edges_request_remote_refreshes`（开 Git 置位、同 view
再点关抽屉不置位、Files 打开不置位、Files→Git 切换置位；example.test
连接后台失败是 harness 既有约定，纯状态断言不受影响）。route 消费分支
的端到端（真实 ssh exec）无单测覆盖（与模块现状一致——remote_git 单测
全为纯函数），靠事件路径断言 + 手动验收兜底：SSH tab 开 Git 抽屉 →
agent 改远端文件 → 关抽屉再开出现新修改；Files↔Git 来回切换每次回 Git
重拉；切任务再切回行为不回退。

## Supersedes

None。

## Revisit when

若远端引入文件系统监视能力（如集成 shell 上报 inotify
事件），点击沿刷新可降级为兜底路径。