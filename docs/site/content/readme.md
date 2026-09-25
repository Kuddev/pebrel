<div class="readme-intro">
<strong>GPU 加速终端、SSH 工作区，以及 AI CLI 会话空间。</strong>
<p>Pebrel 把本地 Shell、远程主机、文件和 AI 命令行工具集中在同一个原生桌面工作区。这个页面把仓库 README 中最常用的信息重新组织成可以快速浏览的图文说明。</p>
</div>

<div class="readme-badges"><span>Rust 2024</span><span>GPUI</span><span>Windows 10 / 11</span><span>macOS 14+ Preview</span><span>Linux glibc 2.35+ Preview</span><span>GPL-3.0</span></div>

## 一个工作区里完成什么

<div class="readme-visual-grid">
<div class="readme-visual-card"><small>TERMINAL</small><strong>终端与会话</strong><p>用标签页和分屏组织多个 Shell；每个窗格保留自己的工作目录，并可保存常用工作区布局。</p></div>
<div class="readme-visual-card"><small>REMOTE</small><strong>SSH 与文件</strong><p>保存远端主机，通过 SSH 连接服务器，并在终端旁使用 SFTP 上传、下载和整理文件。</p></div>
<div class="readme-visual-card"><small>AI CLI</small><strong>AI 命令行工作流</strong><p>在独立标签或分屏中运行受支持的 AI CLI，通过活动状态、通知和历史记录跟踪任务。</p></div>
<div class="readme-visual-card"><small>READER</small><strong>原生阅读与配置</strong><p>阅读 Markdown、公式和本地图片，使用主题、背景、快捷键与 Lua 配置调整工作环境。</p></div>
</div>

<figure><img src="@ROOT@assets/screenshots/nebula-top-tabs.png" alt="Pebrel 顶部标签和终端工作区" width="1040" loading="lazy"><figcaption>用标签页组织多个终端，并在同一个窗口里保持工作上下文。</figcaption></figure>

<figure><img src="@ROOT@assets/screenshots/split-ai-workflows.png" alt="Pebrel 中的多 AI CLI 分屏工作流" width="1040" loading="lazy"><figcaption>多个命令行任务可以并排运行；每个窗格保持独立输入和输出。</figcaption></figure>

## 平台与下载

从[最新发行版](https://github.com/Kuddev/pebrel/releases/latest)选择与你的系统和架构对应的安装包。

| 平台 | 当前支持 | 安装包 |
| --- | --- | --- |
| Windows 10 1809+ / 11 | 正式版，x64 | .exe 安装器或 .zip 便携包 |
| macOS 14+ | Preview，Apple Silicon / Intel | .dmg |
| Linux，glibc 2.35+ | Preview，x64 | .AppImage、.deb 或 .tar.gz |

Windows 还提供系统托盘驻留、全局快速终端热键、本地 AI hook 自动配置和自动更新安装。macOS 与 Linux 的可用能力以[安装与平台支持](installation.md)为准。

## 终端与会话

- 侧栏或顶部标签管理终端，并可拖拽调整分屏。
- 每个窗格可以拥有独立工作目录；复制部分远端或子系统标签时会保留已知目录。
- 支持历史和路径补全、快捷键及集成 Shell 提示。
- Windows 可选择关窗后继续保留后台会话；应用真正退出后的对话恢复属于另一套机制。

要实际配置这些行为，参见[标签页与分屏](workspace.md)、[保存与恢复会话](sessions.md)和[快捷键速查](keyboard.md)。

## SSH 与文件

Pebrel 可以保存主机、读取 SSH 配置别名，并使用代理或单级跳板。认证可使用密码、私钥及服务器支持的交互流程，首次连接时应核对主机密钥。

SFTP 面板用于浏览远端目录、上传与下载文件或文件夹、查看传输进度并取消任务。本地文件浏览、Git 操作和远端文件编辑可以与终端同时进行。

<figure><img src="@ROOT@assets/screenshots/hero.png" alt="Pebrel 工作区示例" width="1040" loading="lazy"><figcaption>终端、文件与远端连接围绕同一工作区组织。</figcaption></figure>

## AI CLI 工作流

Pebrel 会识别多种 AI CLI，并在受支持时显示活动状态与更精确的任务提示。通知与来源窗格关联，点击后可以回到对应终端。

已捕获的回答可以在阅读器中查看 Markdown、公式、原文和本地图片。向终端粘贴图片时，Pebrel 会把图片保存为文件并把路径交给当前会话；终端内联显示是否可用取决于命令行工具输出的协议和内容。

<figure><img src="@ROOT@assets/screenshots/nebula-claude-session.png" alt="Pebrel 中的 AI CLI 会话" width="1040" loading="lazy"><figcaption>AI CLI 与普通 Shell 一样运行在终端中，同时可使用会话相关的辅助能力。</figcaption></figure>

## 外观、文档与配置

Pebrel 提供浅色与深色主题、背景、不透明度、图标和界面语言设置。Markdown 文档可以直接在应用内阅读，数学公式由原生界面排版。

<figure><img src="@ROOT@assets/screenshots/native-math-rendering.png" alt="Pebrel 原生文档与公式排版" width="1040" loading="lazy"><figcaption>文档阅读器可以显示 Markdown 内容和数学公式。</figcaption></figure>

使用命令行初始化并检查 Lua 配置：

~~~sh
pebrel config init --language zh-CN
pebrel config check
~~~

配置重载失败时会继续使用上一份有效配置。完整字段和示例参见[使用 Lua 配置](configuration.md)与[常用配置示例](config-examples.md)。

## 通知

在 **设置 → 终端 → 提醒** 中可以选择通知显示时长：保留默认时长、5 / 10 / 30 / 90 秒，或保持常驻。隐藏通知卡片只会关闭显示，不会自动执行确认、拒绝或其他任务操作。

## 构建 Pebrel

安装仓库固定的 Rust 工具链和平台依赖后，README 给出的正式产品构建命令为：

~~~sh
cargo build --release --locked -p nebula --bin pebrel --features gpui-shell
~~~

准备参与开发时，请继续阅读[贡献与开发](contributing.md)，按照仓库规定运行相应检查。

## 赞助、社区与联系

- **赞助**：README 当前感谢 Fluxion AI 对项目的支持。
- **社区**：可通过项目 README 中列出的社区入口参与讨论。
- **Discord**：[discord.gg/VFn4rcxmhn](https://discord.gg/VFn4rcxmhn)
- **邮箱**：[fickleheartedkeys@163.com](mailto:fickleheartedkeys@163.com)
- **许可证**：GPL-3.0；第三方版权和许可证声明以仓库内对应文件为准。

> [!NOTE]
> 本页是 README 的文档化导览，不替代仓库 README。版本、下载文件和社区入口发生变化时，以仓库当前内容和最新发行版为准。
