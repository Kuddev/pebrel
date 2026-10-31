# 侧栏与抽屉统一从 pane cwd 派生项目身份（本地挂载折叠 + 远端探测）

## Status

Accepted

## Context

用户 10-15 报障两处 + 一个架构诉求：① 本地任务进 fish 后左侧分组
正常跟随，右侧抽屉（文件树 + Git 面板）不动；② SSH 任务进 fish 后分组
正常但组头分支 chip 消失（10-15 上午陈旧根判定的副作用：分支随标题协议
静默被抑制，却没有补位来源）；③「左右两侧的 cwd 项目/目录与 git 状态能
否统一用一个源」。用户随后给出目标规格并追加两处报障：**规格**——先拿
当前 cwd（无论默认终端还是嵌套 shell），再由 cwd 取 git 项目状态；有则
左侧项目分组+分支名、右侧 cwd 目录+git 状态，无则左侧目录分组、右侧
cwd 目录+「不在仓库」；**④** 本地 fish 里分组右侧的分支名停在旧 shell 的
遗物上（本地 chip 唯一来源是标题协议）；**⑤** SSH fish 里 chip 显示成
路径、面板误报「不在仓库」（探测输出形状假设在真实主机上不成立）。

## Evidence

三条上报信道（OSC 7 cwd、`NEBULA|cwd|branch` 标题、
`pebrel_git_root` UserVar）同源于集成 shell 的 precmd；fish 等无集成嵌套
shell 只剩 OSC 7（view.rs 10-10 兜底注释）。左侧分组 10-10 起接受 unix
风格 cwd 并折叠 `/mnt|/cygdrive`（`host_group_path`），右侧抽屉的
`TerminalView::local_cwd` 却直接 `is_dir`——fish 上报的挂载形态必为假，
两侧就此分叉。分支 chip 的唯一来源是标题协议，fish 里静默。

## Decision

1. **本地**：挂载折叠单点化为 `shell_detect::mounted_host_path`
   （`/mnt/<盘>`、`/cygdrive/<盘>`，及仅 Windows 的 `/<盘>/` MSYS `$PWD`
   形态——其他平台单字母根是真实目录）。左侧分组（`host_group_path`
   委托）与右侧抽屉（`local_cwd` 原样 `is_dir` 失败后折叠再验）共用同一
   变换；fish 里两侧读到同一份宿主目录。
2. **远端**：新增按 pane 的项目身份探测 `refresh_remote_project`（挂
   TitleChanged 事件沿）：集成上报的根失守（cwd 移出，即 10-15 上午的
   陈旧判定）才在**变化沿**上对远端跑一次 `git rev-parse --abbrev-ref
   HEAD --show-toplevel`——与集成同一条命令、同一输出契约、单叉只读，
   结果存 `RemoteGit::projects`（pane 关闭才清，不随抽屉聚焦切换）。
   在途按 pane 去重；预算 3s。
3. **三个消费方读同一份身份**：侧栏子组键（`group_root`：上报根 →
   探测根 → cwd）、组头分支 chip（`group_branch`：集成在线随标题、陈旧
   随探测、裸仓库保持）、Git 面板根（`remote_git_root` 同一优先序）。
   探测根顺带修掉 cwd 兜底锚在仓库**子目录**的错位——porcelain 路径相对
   仓库根，锚点必须是真根。
4. 集成在线时探测零流量：上报根覆盖 cwd 即短路，集成保持权威（它比
   探测更及时——precmd 在带内，探测要等事件沿）。
5. **本地分支也从 cwd 派生**（④，落实用户规格）：`project_group_cache`
   的值从根串扩为 [`LocalProject`]（根 + 分支）。根的向上 stat 链仍只在
   cwd 首见时跑一次；分支每个提示符重读一次 `.git/HEAD`
   （`git_head_branch`，不 spawn git；linked worktree 经 `gitdir:` 指针
   跳转，detached 给 "HEAD" 与 `--abbrev-ref` 同口径）。本地 chip 不再
   读标题协议的 `view.branch`——它在 fish 里是旧仓库遗物。
6. **探测命令自描述化**（⑤）：两条**独立** `rev-parse` 各自捕获，
   `printf` 生成单行 `PEBREL_PROJECT\t<分支>\t<根>`——输出顺序、字段数
   由命令钉死，解析只认标记行（免疫登录 shell 欢迎噪声与 git 对组合
   参数的输出次序差异）；`sh -c` 包裹（quote 双层转义）使 bash/zsh/fish
   登录 shell 同一语法；退出码恒 0，git 缺失/不在仓库只是字段为空。
   **该方案在用户主机上整条死掉**（⑤复测症状原样），由决策 7 取代：
   再聪明的单条命令也引入了 status 通道没有的 shell 语法假设。
7. **探测回归两条单目的 exec**（⑤复测）：分支、根各跑一次
   `git -C <cwd> rev-parse …`，复用与 status 完全相同的命令构造器
   （真实主机验证过的引号与参数形态），零新增 shell 语法；每条要么单行
   要么失败（exec 退出码非零即 `Err`，stderr 不进结果），不存在形状
   假设。
8. **分支槽路径防御**：分支字段长得像绝对路径（`/` 开头）一律不信——
   标题协议与探测的分支槽都套用（用户现场的「分支名变为路径」无论
   上游哪里塞错槽都被这条拦下）。
9. **本地 cwd 的进程树兜底**（「已知边界」挽救，用户 10-15 追加）：
   不自报 OSC 7 的嵌套 shell（PowerShell→bash/zsh）与 cmd pane，由
   `platform::pane_cwd` 读 pane 进程树**最深 shell 进程**的 PEB
   `CurrentDirectory`（Windows 无 `/proc`；只读、同用户、
   `ReadProcessMemory`；尾部分隔符归一成 OSC 7 形态）。触发挂在
   Wakeup 沿（cd 必然伴随提示符输出）+ 2s 节流；`cwd_reported_at`
   记录 shell 自报活性，4s 内自报过的 pane 直接退避——进程枚举只花在
   真正不自报的 shell 上。结果经 `adopt_process_cwd` 走与 OSC 7 完全
   同一入口（更新 `cwd` + 广播 TitleChanged），分组/抽屉/Git 面板一齐
   跟随，消费方无需知道 cwd 的来路。
   **首版事故**：空闲的 PowerShell pane 超过 4s 没有提示符输出也会被
   当成「不自报」，最大化/还原的重排输出恰好触发探测沿——而
   PowerShell 的 `Set-Location` 只动 `$PWD`，Win32 进程目录停在**启动
   路径**，两侧边栏被覆写回 `C:\Users\…`（用户 10-15 复现三次点击稳定
   触发，回车出提示符即被 OSC 7 纠正）。修正分两层：
   a. **目标可信按运行时判定**：cmd 原生跟 `cd`；cygwin 系 shell
      （bash/sh/dash/zsh/fish）按目标进程加载的 DLL——`cygwin1.dll`
      即时同步 Win32 进程目录（可信），`msys-2.0.dll` 为性能刻意不同步
      （实测 Git Bash `cd` 后 PEB 停在启动目录，误信即覆写事故同款），
      WSL 的 `System32\bash.exe` 转发器无 cygwin 运行时；PowerShell/
      pwsh/nu/wsl 一律拒绝。宁缺毋错。
   b. **自报退避窗 4s→1s**：退避只是成本闸门（省自报 pane 的进程枚举），
      误信防护已由 a 承担。大窗口的代价是外层 shell 的旧上报戳吞掉嵌套
      shell 进场后第一次 cd 的探测沿——用户 10-15 现场为
      PowerShell→cygwin64 bash「要再回车一次/最大化还原才跟随」。
10. **SSH cwd 的按钮级兜底**：远端「跟随当前终端」在信道静默（嵌套
    bash/zsh）时改走 `ssh_sftp::remote_cwd` 的远端进程树探测（一次性
    往返）；抽屉首开本来就走这条。**拒绝**周期性自动探测：每 pane 每
    数秒一条 exec 的流量换不来按钮/首开覆盖不到的场景。

## Rejected alternatives

- 每次 TitleChanged 无条件探测：集成在线时纯属浪费；变化沿判据（cwd
  移出已知根才发）与集成的 UserVar 边沿协议同构。
- 把探测结果写回 `view.git_root`：与 10-15 上午「不在 view 层清除/改写」
  同理——变化沿协议靠 shell 与 pane 各存一份上次值对账，旁路写会破坏
  退出 fish 回到同仓库时的对账。
- 本地为 fish 推集成：fish 非 POSIX rc，属远端/本地集成自身迭代。

## Consequences

fish 内 cd 到仓库子目录时，SSH 组名与面板根都是真根（优于
cwd 兜底）；探测说「不在仓库」时组回落 cwd、无分支 chip——与集成形态
一致。远端 exec 流量有界：每 pane 每次项目切换一次 rev-parse。本地
`/<盘>/` 折叠仅 Windows；WSL 来宾自有路径（`/home`）本就不在宿主文件
系统，仍走 WSL 专属通道。上一 note 的「分支随根陈旧同罪」由本 note 的
探测补位取代（判定本身保留）。

## Validation

`shell_detect::tests::mounted_cwds_fold_to_host_drive_paths`；
`gitignore::tests::head_file_yields_the_branch_for_grouping`（普通/嵌套
分支/detached/worktree 指针/非仓库）；
`task_groups_ssh::tests::probed_identity_takes_over_when_the_reported_root_is_stale`
与 `branch_chip_follows_the_root_staleness_verdict`（含路径形态防御）；
`remote_git::tests::root_prefers_reported_then_probed_then_cwd`（三级 +
子目录真根锚定 + 集成权威）与
`probe_fields_reject_path_shaped_branches_and_non_posix_roots`；
`platform::pane_cwd::tests::pane_tree_reports_the_deepest_shells_working_directory`
（真实子进程端到端：树遍历 + 最深 shell + PEB 读取）、
`powershell_never_leaks_its_stale_process_directory_into_the_pane` 与
`bash_sync_tests::msys_bash_cd_is_refused_by_the_runtime_check`
（负向验证：PowerShell 与 msys 运行时的进程目录谎报一律拒绝回填）；
workspace 套件
无回归（`recent_sessions`、`process_tree::activity_evidence` 两失败经
干净树对照为既有环境问题）。`Win32_System_Kernel` feature 仅为
`PROCESS_BASIC_INFORMATION` 结构引入，无新 crate。用户 10-15 在真实
SSH 主机复测确认修复（fish 内分支 chip 与 Git 面板恢复正确）；改动
不预先写 `CHANGELOG.md`，发布准备时统一产出（维护者裁定）。

## Supersedes

None

## Revisit when

远端集成支持 fish 时探测自然不再触发（保留为防御）；探测
若需要更多字段（upstream、dirty 位），扩 `RemoteProject` 而不是另起通道。