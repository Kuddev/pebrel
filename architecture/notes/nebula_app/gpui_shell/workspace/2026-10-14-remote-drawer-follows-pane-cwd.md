# 远端抽屉跟随 pane cwd（文件浏览器跟随 + 远端 Git 只读视图）

## Status

Accepted

## Context

聚焦 SSH pane 时抽屉三视图都不跟随当前终端目录——文件浏览器只在
首次绑定时经 `remote_cwd::probe`（SSH exec 进程树探测）定位一次，多 pane
共用池化连接时必 `Ambiguous` 回 `/`，绑定后终端 `cd` 也无人消费；Git 视图
无远端形态，渲染的是本地 `side_panel` 快照（上一个本地目录的仓库状态），
误导。pane 自己的 `TerminalView::remote_cwd()`（OSC 7 上报，pane 精确）此前
只被 tab 复制消费。

## Evidence

`remote_files.rs::attach_remote_browser`（改动前只走 probe）；
`workspace.rs::sync_side_panel_to_active` 在 `remote_browser.active()` 时早退
（本地树不跟远端路径，正确），但远端侧没有对应跟随物；
`details_panel.rs` 的 `PanelView::Git` 分支无 SSH 分流。

## Decision

1. 文件浏览器起点与跟随都以 `remote_cwd()` 为准：绑定时有上报直接
   `navigate_remote`，无上报（无集成）才退 probe；跟随判定收在纯函数
   `should_follow`（`following` 开关 + cwd≠当前 + cwd≠在途目标三重门），
   由 `route_remote_browser` 每帧消费——它是唯一判据入口的既有约束。
2. 跟随语义对齐本地树 custom root：双击进入目录/回上级停跟随，工具栏
   「跟随当前终端并刷新」按钮（复用 `files.tree.follow_refresh` 词条，
   `RemoteReread` 词条随之闲置）恢复；`following` 随 pane 快照存取
   （`RemoteDirectorySnapshot`），`requested_path` 做在途去重。
3. 远端 Git 只读视图（`remote_git.rs`）：root 取 `pebrel_git_root`（上报）
   优先、远端 cwd 兜底，不探测；数据一条 `git -C <root> status
   --porcelain=v2 --branch -z`（root 经 `ai_hook::remote::quote` 引用），
   解析复用 `git_worktree::status::parse_status`，分组与 `collect_git_info`
   同判据（冲突三处保留）。退出码 128（不在仓库/裸仓库）走
   `VcsNotRepository` 空态，不当错误渲染。
4. 复用 `VcsList`/`render_vcs_list` 渲染变更列表，新增 `readonly` 标志
   （`sync` 增参）门控行内 stage/unstage/discard/SVN 菜单与 merge 按钮；
   双击在 readonly 下改走 `open_remote_document`（root 字符串保真拼接，
   不经 `PathBuf`——Windows 会把远端 `/` 本地化）。plus/minus 行级统计与
   history 图需额外远端往返，只读形态不拉。
5. pane 关闭统一走 `forget_remote_state`（浏览 + Git 两份状态），
   workspace.rs 两个关闭路径共用防漏。

## Rejected alternatives

- 跟随继续走 probe：无法区分同主机多 pane（连接共用），也无法在 cd 后
  被动触发（probe 是拉式、按目的地）。
- 远端 Git 做读写操作：远端凭据、冲突与锁的风险面完全不同，先只读
  （用户 10-14 裁定）。
- 强制跟随（cd 永远打断手动浏览）：与本地树语义不一致（用户 10-14
  裁定对齐本地树）。

## Consequences

远端无集成（无 python3 等）时文件抽屉维持 probe→`/` 现状、
Git 视图给空态——跟随能力与集成上报绑定，属已知边界。任务分组不改：
链路已实现（见 2026-10-12 两篇 note），不跟随多为远端 rc 未升级，重连即愈。

## Validation

`remote_files::tests::follow_gate_rejects_disabled_missing_current_and_in_flight`
/ `parked_following_state_returns_with_the_snapshot`；
`remote_git::tests::root_prefers_reported_git_root_then_cwd` /
`porcelain_output_maps_to_grouped_snapshot`；
`check_architecture.py --base <PR-base>` 通过（workspace.rs 回到 2777/2778）。

## Supersedes

None

## Revisit when

远端 Git 需要写操作（stage/commit/push 走 exec）或行级
diff 统计时，扩 `remote_git.rs` 的命令面并解除 readonly 门控。