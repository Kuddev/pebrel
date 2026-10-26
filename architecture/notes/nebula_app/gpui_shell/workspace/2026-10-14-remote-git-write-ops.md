# 远端 Git 面板的写操作（exec 通道、命令面与本地同构）

## Status

Accepted

## Context

远端 Git 视图（见
[2026-10-14-remote-drawer-follows-pane-cwd](2026-10-14-remote-drawer-follows-pane-cwd.md)）
首版只读——写操作按钮全部隐藏。用户 10-14 裁定补齐：SSH 主机任务的右侧
抽屉 Git 状态要能直接 stage/commit/push，不再退回终端手敲。

## Evidence

本地写操作的命令面在 `display/side_panel/vcs.rs`（`git_stage_path`
= `add --`、`git_unstage_path` = `restore --staged --`、`git_discard_path`
= `restore --`、`vcs_commit_message` = `commit -m`、`git_push` = `push`、
`git_pull` = `pull --ff-only`、`git_stage_all` = `add -A`，WSL 路径带
`--no-optional-locks`）；`ssh_session::exec_capture` 已承载同等命令行
（`git -C '<root>' …` 与远端集成 bootstrap 同一转义规则）。

## Decision

1. 写操作全部经同一 exec 通道：`git -C <root> --no-optional-locks <args>`，
   参数（含路径与提交信息）逐个单引号包裹（`ai_hook::remote::quote`），
   空格/引号/`$` 不会被远端登录 shell 展开。`--no-optional-locks` 与本地
   同款：不抢 index.lock，不惊动远端开着的 IDE。
2. 命令面与本地一一对应（上列七条），语义差异为零——用户在本地和远端
   面板学到的操作可以互搬。写操作预算 30s（push/pull 走网络），状态
   读取维持 8s。
3. 操作收尾**无论成败**重拉 status：失败的 pull/push 也可能改写工作副本
   或 refs，旧快照比没快照更误导。`op_running` 在途时禁用其余操作与
   刷新按钮（与本地 side_panel 的排他一致）。
4. 行内按钮的分发在 `vcs_panel/rows.rs`：`VcsList` 的形态标志由 `readonly`
   改名 `remote`（写操作回归后"只读"名不副实）——stage/unstage/discard
   两种形态都画、按 `remote` 分发到 `remote_git_*` 或 `side_panel`；
   merge 编辑器与 SVN 入口是本地专属，`local_only` 门控不画。discard 的
   二次确认沿用 `vcs_discard_confirm`（行 key 是 root join 的相对路径，
   本地/远端各自自洽）。
5. 提交草稿共享本地那份 `CommitInput`：本地/远端是同一根输入框的两种
   提交目的地，切形态不丢草稿；`submit_remote_vcs_commit` 与本地
   `submit_vcs_commit` 同一交互（读草稿→发命令→清空）。

## Rejected alternatives

- 远端写操作弹确认对话框（"这些命令将在远端执行"）：目的地已由主机名
  行（`remote_files` 同款安全设计）持续可见，逐操作确认只会训练用户
  无脑点掉；真正不可逆的 discard 已有两段式确认。
- 每个操作单独拉一次全量 status 再 diff：多一次远端往返且没有增量收益。
- push/pull 加 `--quiet` 之外的进度解析：exec 通道没有增量输出回流，
  面板用转圈 + 完成后刷新表达，与本地 spawn 版一致。

## Consequences

远端 git 不存在（最小容器）时写操作报 exec 错误（错误行
已渲染），面板不崩。SVN 远端、merge 冲突编辑器、行级 diff 统计仍不提供
——远端形态只做 Git。旧 note 的「只读」结论由本 note 取代决策 3/4 两点，
其余（跟随、root 决策）不变。

## Validation

`remote_git::tests::command_line_quotes_root_paths_and_messages`
（转义规则：空格路径、`it's $HOME` 消息）；workspace 模块 243 测试无回归
（4 个失败为分支既有，与本改动无关，已用干净树对照验证）；
`check_architecture.py --base <PR-base>` 通过。

## Supersedes

None

## Revisit when

远端需要 SVN 支持、merge 冲突解决或行级 diff 统计时；
或 exec 通道支持增量输出回流（进度条）时重做 push/pull 的反馈。