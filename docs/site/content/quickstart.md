## 安装并打开

1. 前往 [Pebrel 官方发布页](https://github.com/Kuddev/pebrel/releases/latest)，选择适合系统与处理器的安装包。
2. Windows 运行安装程序，或完整解压便携包；macOS 打开 DMG，将 Pebrel 拖入“应用程序”；Linux 使用 DEB、AppImage 或完整便携包。
3. 启动 Pebrel，先保留默认设置。在终端中输入下面的命令并回车。

```sh
echo Hello, Pebrel
```

看到 `Hello, Pebrel` 后，你的第一个终端已经可以使用。[安装说明](installation.md)列出了各个平台的要求与首次启动注意事项。

> [!TIP]
> Pebrel 是运行命令的窗口，不会自动安装你的开发环境。某个命令提示“找不到”时，先确认相应工具已安装，并且能在系统原有终端中运行。

## 建立顺手的工作区

### 新建标签页

Windows / Linux 按 <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>T</kbd>；macOS 按 <kbd>⌘</kbd> + <kbd>T</kbd>。新标签页适合另一个项目，或一项不希望打断当前任务的工作。

在终端里用 `cd` 进入你的项目目录。例如，先把下面的路径替换为真实存在的文件夹，再运行：

```sh
cd "你的项目目录"
```

### 并排打开窗格

Windows / Linux 按 <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>D</kbd>，macOS 按 <kbd>⌘</kbd> + <kbd>D</kbd>，向右分屏。左边运行开发工具，右边执行测试或查看文件。

拖动两个窗格之间的分隔线调整宽度。需要专心看一个窗格时，按 <kbd>Ctrl</kbd> / <kbd>⌘</kbd> + <kbd>Shift</kbd> + <kbd>Enter</kbd> 切换放大，再按一次回到分屏布局。

<figure><img src="@ROOT@assets/screenshots/split-ai-workflows.png" alt="多个命令行工具在 Pebrel 的独立分屏中运行" width="1040" loading="lazy"><figcaption>分屏让各个任务保持独立。项目公开截图；其中的命令行工具需另行安装。</figcaption></figure>

### 找不到某个操作

按 <kbd>Ctrl</kbd> / <kbd>⌘</kbd> + <kbd>Shift</kbd> + <kbd>P</kbd> 打开命令面板。要更换 Shell，使用 <kbd>Ctrl</kbd> / <kbd>⌘</kbd> + <kbd>K</kbd> 打开 Shell 选择器。

这里是 **Pebrel 应用内的快捷键**。文档网站中的同一组合键用于搜索文档，不会控制桌面应用。

## 连接远程主机

打开 Pebrel 设置，进入 **SSH**。创建一个主机，填写服务器地址、用户名与端口，按服务器要求选择密码或私钥认证，然后连接。

第一次连接前，把显示的主机密钥指纹与服务器管理员提供的指纹核对一致。不要仅为了跳过提示而接受不认识的指纹；已有主机的指纹突然改变时，先确认服务器是否重装或换钥。

连接后，你输入的命令在远程机器上执行。远程文件浏览与传输使用 SFTP；远端需允许相应服务。

## 使用 AI 命令行工具

先按照工具自身的说明安装并登录 Claude Code 或 Codex。在 Pebrel 中进入项目目录，再运行你已安装的工具，例如：

```sh
claude
```

或者：

```sh
codex
```

Pebrel 可以为识别到的 AI CLI 显示活动状态。更精确的状态与回答捕获需要对应的 hook 集成；自动本地 hook 设置目前是 Windows 平台能力。macOS 与 Linux 仍可以正常运行已安装的 CLI，但不要把“能运行”理解为“所有集成都已自动配置”。

AI 服务的登录、额度与费用由对应工具和服务提供方管理。关闭或隐藏通知不会代替你批准终端中的请求。

## 做两个舒适度调整

打开设置：Windows / Linux 使用 <kbd>Ctrl</kbd> + <kbd>,</kbd>，macOS 使用 <kbd>⌘</kbd> + <kbd>,</kbd>。

在 **外观** 中选择主题和字体。临时调整终端字号可以使用 <kbd>Ctrl</kbd> / <kbd>⌘</kbd> + <kbd>+</kbd> 或 <kbd>−</kbd>；按 <kbd>Ctrl</kbd> / <kbd>⌘</kbd> + <kbd>0</kbd> 恢复。

再按 <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>B</kbd>（macOS 为 <kbd>⌘</kbd> + <kbd>B</kbd>）收起或展开侧边栏，选择适合屏幕的工作空间。

## 结束这次工作

先保存正在编辑的文件，确认长时间运行的任务是否已完成，再关闭窗格或窗口。

> [!IMPORTANT]
> **恢复布局、恢复 AI 对话、保持进程运行是三件不同的事。** Windows 的可选后台驻留可以在关闭窗口后保留运行中的会话；完整退出应用后，普通进程不会因为开启布局恢复而继续运行。其他平台不要依赖关闭窗口后的进程保活。
