# `pebrel_git_root` UserVar 协议（远端仓库根上报）

## Status

Accepted

## Context

侧栏 SSH 主机分组需要远端「项目/目录」身份，与本地分组逻辑一致
要求**仓库根**（同仓库的兄弟目录并组）。远端 cwd/branch 已有通道
（OSC 7 + `NEBULA|cwd|branch` 标题），仓库根没有。

## Evidence

`res/shell/bashrc`/`zshrc` 的 precmd 已经为分支 chip 调了一次
`git rev-parse --abbrev-ref HEAD`（每 prompt 一次 fork）；仓库根可由同一
次调用附带（`--show-toplevel`）。`nebula_terminal/src/osc_cwd.rs::
parse_user_var` 现成支持 `SetUserVar=<name>=<base64>`：名字白名单
`[A-Za-z0-9_]`、UTF-8、8KiB 上限，未知名字在旧客户端被无害忽略且保持
线序（event_loop 的 UserVar 不与数据交错）。

## Decision

仓库根走 **OSC 1337 `SetUserVar=pebrel_git_root=<base64>`**：

1. precmd 把分支与根合并为一次 `git rev-parse --abbrev-ref HEAD
   --show-toplevel`（fork 数不变）；标题协议 `NEBULA|cwd|branch` 形状
   **不动**。
2. 只在根**变化沿**发射（进出仓库各一次；base64 fork 只发生在跳变沿，
   每 prompt 零额外 fork）；离开仓库发空值，接收侧清空。
3. 裸仓库只输出分支行：单行输出即「无工作树根」。
4. 接收侧 `TerminalView::git_root: String`（空 = 无）在 `UserVar` 臂维护，
   值变化发 `TitleChanged` 走 `resolve_project_group` 现成打点链。
   rc 文件本地/远端共享（`platform/shell_integration.rs` 与 SSH 集成
   include 同一份）：本地 pane 只存不用，消费严格门控
   `ssh_destination.is_some()`。

## Rejected alternatives

- `NEBULA|` 标题第 5 字段：旧客户端 `splitn(3)` 会把 `NEBULA|cwd|branch||root`
  的第 5 段并进 program 槽 → `running_program = Some("|root")` →
  `program_is_work` 恒真 → 该远端 pane **永久 Running**——正是
  远端状态修复要消灭的故障形态的镜像。版本偏斜（新客户端已重推 rc 到
  共享远端、旧客户端连接）下必然触发，否决。
- SSH exec channel 远端探测：每次 cd 一次往返，慢链路不可接受。

## Consequences

新客户端 + 旧 rc（升级后首连未及重推）→ var 不来 → 子组
回落 cwd/兜底，一次重连后集成自动升级（manifest 资产哈希只拦用户改过的
文件，自家旧版可覆盖）。旧客户端 + 新 rc → var 被旧解析无害吞掉，标题
形状未变，零污染。

## Validation

`platform/shell_integration.rs` 的
`zsh_precmd_reports_git_branch_via_nebula_title` /
`bash_precmd_reports_git_branch_via_nebula_title`（真 shell + 真 git 仓库：
变化沿恰好一次、根值 base64、非仓库不发射）；
`terminal/view/activity_tests.rs::git_root_user_var_updates_only_on_change`。

## Supersedes

None

## Revisit when

标题协议整体升级（例如改结构化 OSC）时把根并回同一通道。