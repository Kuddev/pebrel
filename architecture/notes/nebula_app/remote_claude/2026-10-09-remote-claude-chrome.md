# 远程 Claude Code：连接卡片与提示符标签的收尾

## Status

Accepted (2026-10-09)。通道本体的设计与理由见
[`2026-10-08-remote-claude-over-ssh.md`](2026-10-08-remote-claude-over-ssh.md)；
本 note 只记这一轮呈现层的决策。

## Context

四阶段帧与统一连接卡片已经在位，但真机试用暴露四处"卡片说了话、屏幕不认账"的
细节：卡片撤得太早（撤早了会露出远端脚本的人读阶段行）、提示符标签的图标只是
字体里的近似字形、反向 ssh 拉起的面板在侧栏没有程序身份。三条都只改呈现，
不动通道协议。

## Evidence

- 实拍原始 ConPTY 流：`ready` 帧（远端脚本 `exec claude` 之前）到达时远端
  Claude 还没画首屏，卡片先撤、TUI 后到，中间那段空白看起来像会话断掉。
  Claude Code 2.1.x 的 OSC 0 标题 `✳ Claude Code` 也远早于面板铺开，所以
  "标题到了"不等于"画出来了"。
- 用户实测：卡片撤掉之后、TUI 铺开之前，pane 里留着四行人读阶段行（本机回环
  已就绪 / 回连通道已建立 / 回连自检通过 / 正在启动远程 Claude Code）——它们跟
  卡片说的是同一件事。
- 同一轮里卡片"停在第二个节点"的原因在帧序：`frame probe` 画在自检重试循环
  之后，重试期间卡片一直停在"回连通道"。
- 用户实测：反向 ssh 拉起的 Claude 面板在侧栏只剩金色权限盾牌，没有程序身份与
  品牌图；直连 ipxair-cc、并在其中跑到 `claude` 的面板两样都有。这条会话由包装
  器直接起，没有可嗅探的交互式命令行，远端 agent 的 hook 身份也没送到本机，
  于是 `running_program` 一直是 `None`。
- 提示符标签前那枚字形原来是 `U+F185`（太阳），与用户指认的图标（侧栏标签页
  标题前那张 `extra/logo/ai_claude.png`）不符。
- 实测：`[char]0xF0CE5`（Nerd Font 的 MDI 区码位，星平面）在 PowerShell 里直接
  报 `Value was either too large or too small for a character.`，赋值失败后提示符
  里那一格连字形带空格一起消失——`[char]` 是 UTF-16 单元，装不下星平面码位。

## Decision

1. **卡片撑到首屏真的画出来**：`ready` 之后不再按标题/目录上报撤卡片，改看
   "可见网格里出现了内容"（`TerminalView::clear_remote_claude_card_when_painted`）；
   包装器把失败原因写出来时同样命中，4 秒宽限仍是兜底。
2. **有卡片就不打第二份文字**：阶段帧的判据（`TERM_PROGRAM=pebrel`）同时决定
   人读阶段行打不打——本机 CLI 那一行、以及远端脚本的 `msg_tunnel`/`msg_probe`/
   `msg_session`（`Session.card` → 脚本里的 `note()`）。普通终端、管道、别的
   终端照旧拿到这些文字，那是它们的唯一反馈。
3. **提示符图标格由宿主画品牌图**：PS1 仍写一枚 BMP 回落字形
   （`nebula_terminal::tty::REMOTE_CLAUDE_CHIP_GLYPH` = `U+F069`，品牌橙），
   GPUI 壳在网格里认出这一格后不画字形（选区/光标底色因此原样保留），改在同一格
   居中画 `ai_claude.png` 的方图（`gpui_shell::terminal::claude_chip`）。位图进不了
   命令行，分工只能这样切；识别与虚线下划线共用同一遍网格遍历
   （`osc_links::link_decorations`），绘制帧不增加扫描。
4. **启动身份可以点名 agent**：`TerminalLaunch::Local` 的 `shell_name` 解析成
   AI agent 时（包装会话填的就是 `claude`），pane 出生即以此为 `running_program`：
   侧栏/标题栏显示品牌图，agent 输入语义（Shift+Enter 换行、不吃补全与 shell
   历史）与直连面板一致。真 hook 与 `NEBULA|` 标题之后照旧覆盖它；标题仍是目录名。
5. **自检帧先于重试**：`frame probe` 移到自检重试循环之前，卡片不再停在
   "回连通道"。

## Rejected alternatives

- **把品牌图或 `✳` 直接写进 PS1 文本**：位图进不了字符流；`U+2733` 不在内置
  字体里；星平面码位被 `[char]` 拒绝（见 Evidence）。
- **用 OSC 1337 内联图片画品牌图**：内联图片按绝对行锚定、绘制时钉在终端左边缘
  （`element.rs` 的图片事件不携带列号），画不进提示符中间；每个提示符还会重新
  解码一张图，把 16 张的图片缓存打穿。
- **继续用字体里的近似字形当图标**：用户明确要求品牌图本身。
- **`ready` 后立刻撤卡片，或按标题撤**：标题早于铺面板很久，实测正好露出那四行。
- **保留远端脚本的阶段文本、只把卡片留久一点**：两份文字说的是同一件事，而
  卡片本身就是它的呈现位。

## Consequences

- 提示符那一格在 Pebrel 里是位图、在别的终端与旧壳里是同一品牌的 BMP 字形，
  两边都不空白；品牌图只解码一次（进程级缓存）。
- `ready` 之后卡片至少留到有内容为止，最长 4 秒；远端 Claude 卡住时不会永久
  盖住终端。
- 侧栏那一行的图标是权限盾牌 + 品牌图（本地包装会话继承当前 token），右侧状态
  槽空闲时显示短标 `claude`，跑回合时换成转圈/圆点/手掌。

## Validation

- 新增 `link_underline::prompt_ssh_chip_exposes_the_icon_cell_for_the_brand_mark`
  （只有标签内的标记格进图标列表，同行里孤立的同码位字形不算）；
  `powershell_prompt_carries_a_clickable_ssh_chip_after_the_cwd` 钉住 PS1 与
  `REMOTE_CLAUDE_CHIP_GLYPH` 同码位。
- 新增 `startup_tests::a_launch_named_after_an_agent_starts_with_that_program`：
  `claude` 声明为身份，`pwsh` 与无身份保持 `None`。
- 新增 `script::tests::a_host_without_a_card_still_gets_the_stage_lines`（`card=0`
  仍打 `note`），并在 `session_command_embeds_arguments_and_stays_one_token` 里
  钉住 `card=1` 时不再出现裸 `printf '%s\r\n' "$msg_tunnel"`。
- 解压后的 release 包真机实测：提示符那格显示品牌图、没有虚线下划线；侧栏显示
  「权限盾牌 + 品牌图 + 目录名 + `claude`」；卡片一路停在「远程会话 / 正在启动
  远程 Claude Code…」直到 Claude TUI 铺开，pane 里不再出现四行阶段文本。

## Supersedes

None. 通道本体仍以
[`2026-10-08-remote-claude-over-ssh.md`](2026-10-08-remote-claude-over-ssh.md)
为准；本 note 只补充呈现层。

## Revisit when

- 远端 Claude 能把阶段写进窗口标题或其它宿主可读信号时（卡片可以只看那个信号）；
- 内置字体补上星平面或品牌字形，或提示符改成宿主渲染的装饰时（不再需要回落字形）。
