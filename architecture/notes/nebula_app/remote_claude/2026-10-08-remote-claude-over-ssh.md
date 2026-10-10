# 远程 Claude Code 的回环通道与提示词缓存

## Status

Accepted (2026-10-08). 实现：`nebula_app/src/remote_claude/`。需求阶段文档是
本地文件（未随仓库发布）；本 note 是随代码入库的设计与理由记录，通道呈现层的
后续决策见 [`2026-10-09-remote-claude-chrome.md`](2026-10-09-remote-claude-chrome.md)。

## Context

Claude Code 本体必须跑在远程 Linux 服务器上，而项目文件、编译与测试留在用户
的 Windows 目录里。两者之间只有 SSH：远程侧要能主动"回连"本机执行命令，而
本机不能要求用户装常驻系统服务、开防火墙端口、改已有 SSH 配置或拿管理员权限。

同时，Claude Code 的内置文件工具作用在它所在的 Linux 上。让它改用回连通道，
只能靠每次启动追加的系统提示词；而提示词一旦逐字变化，模型的提示词缓存就会
整体失效，重连后的会话成本与延迟同时上升。

## Evidence

- 本机 `C:\Windows\System32\OpenSSH` 是 9.5p1 单体 sshd，且"OpenSSH 服务器"
  在 Windows 上是可选功能；随包固定版本（Win32-OpenSSH 10.0.0.0p2-Preview）
  的 `sshd.exe` 已拆成 `sshd-session.exe` / `sshd-auth.exe`，只带主程序起不来
  （`scripts/prepare-windows-openssh.ps1` 的文件清单）。
- `pebrel ssh` 已经在用 `AttachConsole(ATTACH_PARENT_PROCESS)` + 前台继承控制台
  跑交互式 ssh（`ssh.rs::run`），这是本机 GUI 进程唯一能正常交互的方式。
- `platform::process::ProcessGroup` 已经是 `CREATE_SUSPENDED` 起来、挂上
  `KILL_ON_JOB_CLOSE` Job Object 后再恢复的正确写法；临时 sshd 复用它即可在
  父进程被强杀时不留残余。
- `WindowsApps` 下的 `pwsh.exe` 执行别名在 SSH 会话里"拒绝访问"（需求阶段的
  实测记录），因此回连侧不使用商店别名。
- 远端端口占用检测只能读 `/proc/net/tcp`（Linux 限定），预检脚本已按此实现，
  并且必须配 `-o ExitOnForwardFailure=yes` 才能把"检测后到绑定前"的竞态变成
  快速失败。
- 2026-10-09 实测：把 `pebrel.exe`（GUI 子系统）直接当成 pane 的 PTY 子进程时，
  它派生的 `ssh.exe` 拿不到 ConPTY 控制台，Windows 会给 ssh 另开一个可见的
  控制台窗口，终端尺寸也不再跟随 pane；在 pane 的 shell 里执行同一命令行
  则一切正常。
- 2026-10-09 实测（同一轮的两个反例）：
  1. **PowerShell 不等 GUI 子系统进程**：`& '<pebrel.exe>' claude …` 立刻返回、
     `$LASTEXITCODE` 为空，包装脚本会误判失败并提前收尾；用
     `Start-Process -NoNewWindow -Wait -PassThru` 才真的等待并拿到退出码。
  2. **在本地 shell 里输命令行会吃掉方向键**：命令运行期间 PowerShell 会重绘
     提示符，宿主据此把补全状态误判成"又回到提示符"，`Up/Down` 被补全接管
     消费（远端 Claude 的信任面板因此选不动 Yes），屏幕还会出现提示符重影。

## Decision

1. **一次性回环 sshd**：每次连接现生成主机密钥与登录密钥（`getrandom` 种子 +
   `ssh_key` 的 `Ed25519Keypair`），只监听 `127.0.0.1`，`PasswordAuthentication
   no`，目录只授予当前用户，进程挂进 Job Object；会话结束即删目录，父进程被
   强杀时由 Job 回收整棵进程树。**登录私钥只留在内存里**（本机 sshd 只认已写入
   的 `authorized_keys` 公钥），落盘的只有本次的主机密钥与配置。运行库取自随包
   `runtime/openssh`，开发期用 `PEBREL_REMOTE_OPENSSH` 覆盖；不回落系统 OpenSSH。
2. **两阶段协议**：`ssh <host> exec /bin/sh -s` 走预检/下发（可用性、登录态、
   镜像目录、临时私钥、远端空闲端口），随后一条 `ssh -tt -o
   ExitOnForwardFailure=yes -R 127.0.0.1:<port>:127.0.0.1:<local>` 同时建立
   反向通道并前台运行 claude。四阶段进度分别由本机（①）和远端脚本（②③④）输出。
3. **提示词只含稳定量**：项目路径（`canonicalize` 后去掉 `\\?\` 前缀）、远端
   镜像目录、回连 shell 类型、PowerShell 绝对路径，以及每项目稳定的
   `~/.pebrel-remote/projects/<key>/ssh_config` 路径。端口、run id、连接 id
   一律不进提示词；每次启动重算提示词，因此 `--resume` 不会复用旧快照。
4. **远端临时目录只有一个清理权威**：远端脚本的 `EXIT/HUP/INT/TERM` trap 负责
   删除本次 run 目录，并带一个"回连连续失败约两分钟即收尾"的看门狗；下一次
   预检再扫描一次残留。本机侧不再补删，避免与仍在运行的远端会话抢文件。
5. **本机临时目录按属主进程回收**：会话目录写一个 `sshd.pid`，下一次连接先扫
   一遍 `sessions/`：属主进程已消失的目录立即删除（强杀后 `Drop` 没跑的情况），
   属主还活着的保留；没有 pid 记录的旧目录仍按 24 小时兜底。
6. **Windows 无窗口与 shell 事实**：sshd 进程带 `SSH_TEST_ENVIRONMENT=1`，
   否则远端每执行一条命令都会弹一个控制台窗口；`CommandShell` 从
   `HKLM\SOFTWARE\OpenSSH\DefaultShell` 读出（未设置即 cmd），回连自检与提示词
   都按它选 cmd 或 PowerShell 形式，PowerShell 只用真实安装路径。
7. **入口接在提示符上**：注入的 PowerShell 提示符在目录段后面画一枚 "ssh"
   标签（OSC 8 + `pebrel-ssh://<base64url(目录)>`，见
   `gpui_shell::terminal::osc_links::REMOTE_CLAUDE_SCHEME`）；左键单击（不需要
   Ctrl）由 `TerminalView` 发出 `RemoteClaudeRequest`，宿主用**已有的启动器
   主机列表**弹出选择器，选中后在**原 pane 内原地**把 PTY 子进程换成
   "控制台原生 PowerShell + `-EncodedCommand` + `Start-Process -Wait`" 的包装
   会话（`workspace::remote_claude_wrapper`），不新开 tab。三个坑（弹出独立
   控制台窗口、PowerShell 不等 GUI 进程、补全吞方向键）都由这一层绕开，
   理由见 Evidence 最后两条。

## Rejected alternatives

- **按需下载或依赖系统 OpenSSH**：用户的网络与"是否装了可选功能"不该决定功能
  能不能用；发布包要求固定版本与固定哈希。
- **常驻系统 sshd 服务 / 固定端口**：需要管理员权限、改防火墙、动用户已有
  配置，且无法做到"会话结束即销毁"。
- **交互式 `ssh -L` 或 `ControlMaster` 通道**：多一条需要单独管理的连接，
  失败面更大；`-R` 在会话 ssh 上一次完成。
- **本机侧兜底删除远端目录**：断网时本机 ssh 先退出而远端脚本仍活着，重复
  删除会把仍在用的 `ssh_config` 抽走。改为单一权威（见 Decision 4）。
- **把 run 目录路径写进提示词**：每次连接都变，等于每次让提示词缓存失效。

## Consequences

- 发布 ZIP 增加 `runtime/openssh/`（14 个文件、约 9 MB 未压缩），
  `package-release.ps1` 在打包前用固定 SHA256 + PE 架构校验补齐。
- 会话期间本机该用户对服务器开放：这是需求已确认的安全边界，提示词里明确
  "不要把私钥读进对话"，密钥仅存活到会话结束。
- 未接通的部分（有意留待后续）：GUI 入口与 `pebrel ctl agent-start --ssh`
  宿主工具命令、字面 `claude --ssh` 的可选包装、Inno 安装包的随包归属、
  sshfs 式本地文件挂载（需求 §5 非目标）。提示符标签只出现在 Pebrel 托管的
  PowerShell 里（用户自带提示符时不画 Nebula 的 powerline，因此也没有这枚标签）。

## Validation

- `cargo test -p nebula --bin pebrel --features gpui-shell remote_claude::`
  25 项覆盖镜像映射、提示词稳定性、报告解析、printf 模板、sshd 配置、
  临时目录 ACL 与残留回收、进程存活判定、PowerShell/cmd 自检命令形态与密钥生成；
  `cargo test -p nebula --test i18n_contract` 校验新增文案的词典合同。
- `scripts/tests/openssh-runtime.tests.ps1` 固定三件事：已校验文件不重复替换、
  被改坏的已安装文件按固定哈希修复、换包（其它来源或架构）在解包前失败。
- 2026-10-08 真机实测（解压后的交付包，目标 `ipxair-cc`，参数 `-- --version`）：
  四阶段全部出现、远端 Claude Code 正常启动并返回版本；`~/.pebrel-remote/run`
  会话结束后为空，镜像目录与稳定 `projects/<key>` 目录按预期建立。
- 同日强杀实测：对会话进程 `TerminateProcess` 后，本机 sshd 立刻消失（Job
  Object），下一次连接回收了上次留下的会话目录。
- 仍未由真实会话覆盖的部分：`/resume` 的历史列表、多目录并发会话的交互体验，
  以及断网时的看门狗时序（其逻辑与阈值见 `script.rs`）。

## Supersedes

None. 需求文档（本地保留、未入库）里的状态描述以本 note 与代码为准；呈现层的
补充决策见 [`2026-10-09-remote-claude-chrome.md`](2026-10-09-remote-claude-chrome.md)。

## Revisit when

- OpenSSH for Windows 不再拆分 sshd，或随包版本需要升级时（同时更新
  `scripts/prepare-windows-openssh.ps1` 的哈希与文件清单）。
- 需要把同一套编排接到 GUI/`ctl agent-start` 或安装包时。
- 本机 sshd 的能力边界（例如需要 TCP 转发或 sftp）变化时。
