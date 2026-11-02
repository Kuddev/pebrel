# 嵌套无集成 shell 里陈旧 git 根的失效判定

## Status

Accepted

## Context

SSH 主机任务在远端终端里再进 fish（或任何没装集成的嵌套交互
shell）并 `cd` 后：文件抽屉跟随正常（OSC 7 照发），但左侧项目分组与右侧
Git 面板钉死在进 fish 前的项目（用户 10-15 报障；zsh 等其他 shell 理论同病）。

## Evidence

`res/shell/bashrc`/`zshrc` 的 `_nebula_precmd` 同一钩子里发三件事：
OSC 7（cwd）、`NEBULA|cwd|branch` 标题（cwd/分支）、`pebrel_git_root`
UserVar（根，**只在进出仓库的变化沿发送**）。进入 fish 后外层 shell 阻塞、
precmd 静默，三信道里只有 fish 自己的 OSC 7 还在随每次提示符到达
（view.rs 的 10-10「进 fish 一直 Running」兜底注释记录了同一事实）——
`view.cwd` 跟走、`view.git_root`/`view.branch` 停留旧值。而消费方
（`task_groups_ssh::child_key`、`remote_git::remote_git_root`）都**优先信任
git_root**，于是被陈旧根钉住。

## Decision

1. 消费侧一致性校验，共享谓词 `task_groups_ssh::git_root_covers(root, cwd)`：
   根为绝对路径，且 cwd 缺席（无从判陈旧，维持旧行为）或 cwd 仍在根下
   （等于根或以 `root/` 开头，路径组件边界 `/srv/work` 不覆盖
   `/srv/workflow`）才可信；否则根视为陈旧——分组回落 cwd 键，Git 面板
   根改用 cwd（`git -C <cwd> status` 自解析仓库，仓库外 128 走「不在仓库」
   空态）。
2. **不在 view 层清除 git_root**：变化沿协议靠 shell（`_nebula_git_root`）与
   pane 各存一份上次值对账；清了 view 侧，退出 fish 回到同一仓库子目录时
   外层判定「无变化」不重发，正确的根反而丢了。陈旧是「cwd 与根的相对
   关系」破坏的，判定就该留在读取这对关系的地方。
3. 分支 chip 与根同源同罪（`group_branch`）：根陈旧时旧仓库的分支不上
   cwd 组；空根（裸仓库单行输出形态）下分支保留展示。

## Rejected alternatives

- view 收到移出根的 OSC 7 时清空 `git_root`：见决策 2，破坏变化沿对账。
- 陈旧时远端 `rev-parse --show-toplevel` 探测真根：与 10-14 note「仓库根
  不探测」的立场冲突，且退化模式（无集成/fish）下每次 cd 多一次远端往返。
- 给 fish 推集成脚本：fish 与 POSIX rc 语法不兼容，属远端集成自身的迭代，
  不在本修复范围。

## Consequences

fish 内 cd 到目标仓库根时分组/面板即刻正确（cwd 即根）；
cd 到仓库**子目录**时分组名与面板根落在该子目录（无集成退化的既有语义，
非回归）；面板写操作以 cwd 为根锚定时，porcelain 的仓库相对路径只有在
cwd 恰为仓库根时才对齐（同上，无集成形态的既有边界）。退出 fish 回到
bash/zsh 后首个提示符即恢复上报根，无需任何本地状态修复。

## Validation

`task_groups_ssh::tests::stale_root_no_longer_pins_the_group_once_cwd_leaves_the_repo`
（移出/仍在根下/组件边界/cwd 缺席四态）、`branch_chip_follows_the_root_staleness_verdict`
（陈旧抑制、裸仓库保留）、`remote_git::tests::root_prefers_reported_git_root_then_cwd`
扩展（陈旧根让位 cwd、前缀边界、cwd 缺席保持可信）；workspace 模块套件
无回归（`recent_sessions::subdirectory_sessions_merge` 为分支既有失败，
干净树对照复现）。

## Supersedes

None

## Revisit when

远端集成支持 fish（或其他嵌套 shell）时，`git_root_covers`
的陈旧判定自然不再触发，可保留为防御；远端 Git 面板若引入根探测
（rev-parse），cwd 兜底锚定的子目录边界随之消失。