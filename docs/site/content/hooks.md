> [!NOTE]
> 本页按 Pebrel 2.1.1 核对。设置页里的"Hook"是各工具对"事件入口"的称呼，本页统一叫"集成"。

## 先了解默认行为

集成让工具在开始、结束或需要你处理时主动告诉 Pebrel，Pebrel 据此更新标签和窗格上的活动提示，并发出通知。它在 Windows、macOS 和 Linux 上都可以使用。

- **Claude Code 和 Codex 默认开启。** 只要它们安装在这台电脑上，Pebrel 会自行在它们的配置里写入集成入口，通常不需要你操作。
- **其他工具默认关闭。** 需要在 **设置 → Agents** 里自己打开，包括 OpenCode、Cursor Agent、Kimi Code、Pi、Oh My Pi、GitHub Copilot 和 Grok。
- **集成只在 Pebrel 里起作用。** 在 Pebrel 之外的终端运行同一个工具，不会发生任何变化。

各工具具体支持哪些功能，见[支持的 AI 命令行工具](agents.md)。

## 在设置里查看和开关

打开 **设置 → Agents**，页面标题是"编程智能体"。每个工具占一行，从左到右依次是：

- 工具图标和名称；
- Pebrel 找到的程序路径，鼠标停在路径上可以看到完整内容；找不到时显示"未安装"；
- 当前状态；
- 一个开关。

1. 先在终端里启动一次要用的工具，让它完成初始配置。工具还没有创建过配置时，这一行会提示"请先运行一次此工具以创建配置，然后刷新"。
2. 点击右上角的 **刷新**，重新检测。
3. 打开这个工具的开关。点击整行也可以切换。
4. 看到"Hooks 已安装，请重启 agent 使其生效"后，退出并重新启动这个工具。
5. 提交一个简短任务，检查标签上的状态是否随任务变化。

只需要打开自己用的工具。开关灰着无法点击，通常是因为 Pebrel 还没有找到这个工具，或它还没有创建过配置。

### 状态的含义

| 状态 | 含义 | 怎么处理 |
| --- | --- | --- |
| Hook 已安装 | 集成已经就绪 | 重启工具后即可使用 |
| Hook 未安装 | 工具已找到，集成处于关闭状态 | 需要时打开开关 |
| Hook 已安装 · 需修复 | 入口还在，但内容与当前版本不一致，例如工具更新时改写过配置 | 关闭再打开一次 |
| 需要检查 | 读取或写入这个工具的配置时出错，原因会显示在这一行 | 看提示，检查配置文件的权限，再刷新 |
| 处理中… | 正在写入或移除 | 稍候 |

如果提示"缺少 hook 辅助程序，请修复 Pebrel 安装后再启用"，说明 Pebrel 自带的辅助程序没有找到，重新安装 Pebrel 即可。

## 开启后会改动什么

Pebrel 只写入属于自己的条目，不会改动你已有的其他配置：

| 工具 | 修改位置（均在用户目录下） |
| --- | --- |
| Claude Code | `.claude/settings.json`，设置了 `CLAUDE_CONFIG_DIR` 时用该目录 |
| Codex | `.codex/` 下的 `config.toml` 和 `hooks.json` |
| OpenCode | `.config/opencode/plugins/pebrel.js` |
| Pi | `.pi/agent/extensions/pebrel.ts` |
| Oh My Pi | `.omp/agent/extensions/pebrel.ts` |
| Kimi Code | `.kimi-code/config.toml` |
| Cursor Agent | `.cursor/hooks.json` |
| GitHub Copilot | `.copilot/hooks/pebrel.json` |
| Grok | `.grok/hooks/pebrel.json` |

这些文件属于对应工具自己的配置。开关处于打开状态时，如果工具升级或别的程序改动了它们，Pebrel 会检测到并把自己的条目补回；你主动关闭的工具不会被补回。

对于 Claude Code 和 Codex，Pebrel 还会放入一个名为 `pebrel-runtime` 的技能，让它们在对话里直接操作 Pebrel，例如分屏、打开文件或把任务交给另一个窗格里的工具，用法见[命令行控制与自动化](runtime.md)。你修改过的技能文件不会被覆盖。

## 关闭集成

在 **设置 → Agents** 关闭该工具的开关，再重启工具。出现"Hooks 已移除，请重启 agent 使其生效"，表示已经移除。关闭后仍可在终端里正常运行这个工具，活动提示会退回到[屏幕识别](agents.md)。

想一次移除所有工具的集成，在终端运行：

```sh
pebrel setup-ai --remove
```

它只移除 Pebrel 写入的条目，保留其他集成。不带 `--remove` 再运行一次，会重新为 Claude Code、Codex 以及你在设置里打开过的工具写入集成。

## WSL 与 SSH

**设置 → Agents** 只管理这台电脑上的工具。WSL 和服务器上的工具有各自的配置，由下面的方式处理。

- **通过 Pebrel 的 SSH 连接：** 连接时会自动检查服务器，为其中已安装的 Claude Code、Codex、OpenCode 和 Pi 配置集成。这一步失败不会影响你正常使用终端。需要手动处理时运行 `pebrel setup-ai --ssh 主机别名或user@host`；加上 `--remove` 会移除，并记住这台主机不再自动配置。
- **WSL：** 在 Windows 上打开 WSL 终端时，会为发行版里的 Claude Code 和 Codex 配置集成。手动运行 `pebrel setup-ai --wsl 发行版名称`，需要指定用户时再加 `--wsl-user 用户名`。Codex 可能会要求你在它的 `/hooks` 里确认这些入口。

SSH 和 WSL 里的其他工具仍可使用，只是没有集成，状态提示依赖屏幕识别。

## 工具没有被检测到

先在准备使用它的终端里运行工具命令。命令找不到时，修复安装和 PATH；命令可用，再点 **刷新**。

确认安装的是目标 CLI。例如，Cursor 桌面程序的 `cursor` 命令不等于 Cursor Agent CLI，后者的命令是 `agent`。工具刚升级过时，重新启动 Pebrel 和工具可以清除旧的检测与运行状态。

## 收不到任务通知

先确认工具已在启用集成后重启，再检查 **设置 → 终端 → 提醒** 里的通知选项。任务状态能更新但卡片没有出现时，通常应继续检查通知显示设置，而不是重新安装工具。

通知时长、点击跳回来源窗格和系统通知见[通知](notifications.md)。
