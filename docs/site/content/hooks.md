## 启用常用工具的集成

**自动本地 Hook 设置目前在 Windows 提供。** Hook 是工具在开始、结束或需要关注时发出的事件，Pebrel 用它更新对应窗格的活动提示。

1. 安装需要使用的 CLI，并启动一次完成初始配置。
2. 打开 **设置 → Agents**。
3. 刷新检测结果，找到正在使用的工具。
4. 启用对应集成；出现需要修复或缺少配置的提示时，按该项提示处理。
5. 退出并重新启动这个 CLI。
6. 提交一个短任务，检查状态和来源窗格是否对应。

只需开启自己使用的工具，不必把所有检测项都配置一遍。

## 支持的设置入口

Agent 设置提供 Claude、Codex、OpenCode、Cursor、Kimi、Pi、OhMyPi、Copilot 与 Grok 等入口。不同工具能提供的事件和历史能力不同；Claude Code 与 Codex 的恢复流程见[AI 会话历史](ai-history.md)。

<figure><img src="@ROOT@assets/screenshots/ai-sidebar.png" alt="Pebrel 侧栏显示 AI 终端的图标和状态" loading="lazy"><figcaption>从侧栏查看各终端的活动状态。</figcaption></figure>

## 工具没有被检测到

先在准备使用它的终端中运行工具命令。若命令也找不到，修复安装和 PATH；若命令可用，再刷新 Agent 设置。

检查安装的是目标 CLI。例如，Cursor 桌面程序的启动命令不一定就是 Cursor Agent CLI。工具刚升级过时，重新启动 Pebrel 和 CLI 可以清除旧的检测与运行状态。

## 收不到任务通知

先确认 CLI 已在启用集成后重启，再检查 **设置 → 终端 → 提醒**中的通知选项。任务状态能够更新但卡片没有显示时，通常应继续检查通知显示设置，而不是重新安装 CLI。

通知时间、点击跳回来源和系统通知见[通知](notifications.md)。

## 关闭集成

在 **设置 → Agents**关闭该工具的集成，再重启工具。关闭后仍可在终端正常运行 CLI；活动提示会取决于剩余可用的检测信息。

## macOS、Linux、WSL 与 SSH

macOS 和 Linux 可以正常运行已安装的 AI CLI，但不提供 Windows 的同一套本地自动 Hook 设置入口。

WSL 和 SSH 中的工具有各自的安装目录和配置。Windows 本地检测通过时，还需在实际运行工具的环境中确认它是否可用；不要在另一台主机的设置目录里寻找本机配置。
