## 安装并打开 Pebrel

从[安装与平台支持](installation.md)选择与你的系统和处理器匹配的安装包。首次打开后，Pebrel 会启动一个终端。看到提示符和闪烁的光标后，就可以输入命令。

在终端中输入下面这行内容，再按 **Enter**：

```sh
echo Hello, Pebrel
```

如果下一行显示 `Hello, Pebrel`，随后出现新的提示符，终端已经可以正常使用。

## 打开项目目录

输入 `cd`，后面跟项目的路径。路径包含空格时，用引号包起来。下面两行任选一行，并把路径改成自己的项目位置。

**Windows PowerShell：**

```powershell
cd "C:\Projects\my-project"
```

**macOS、Linux 或 WSL：**

```sh
cd ~/Projects/my-project
```

输入 `pwd` 可以查看当前目录。在 Windows 命令提示符（cmd）中，使用不带参数的 `cd` 查看目录。

需要切换终端程序时，按 **Ctrl+K**（macOS 为 **⌘K**）打开 Shell 选择器，再选择已安装的 Shell。默认 Shell 和启动目录可以在 **设置 → 终端** 中调整。

## 再开一个终端

运行开发服务时，原终端可能一直显示日志。另开一个标签页，就能继续输入其他命令。

| 操作 | Windows / Linux | macOS |
| --- | --- | --- |
| 新建标签页 | Ctrl+Shift+T | ⌘T |
| 向右分屏 | Ctrl+Shift+D | ⌘D |
| 向下分屏 | Ctrl+Shift+S | ⌘⇧D |
| 放大当前窗格，再按一次还原 | Ctrl+Shift+Enter | ⌘⇧Enter |

点击要使用的窗格后再输入。新建分屏会启动另一个 Shell，你可以在里面切换到其他目录或运行不同程序。

<figure><img src="@ROOT@assets/screenshots/nebula-top-tabs.png" alt="Pebrel 中的终端标签页" loading="lazy"><figcaption>通过标签页切换终端；需要同时查看时，可在一个标签页内分屏。</figcaption></figure>

## 连接服务器

打开 **设置 → SSH**，选择 **添加 SSH 主机**，填写地址、用户名和认证信息。保存后，从主机列表打开连接。

首次连接会要求确认服务器指纹。与服务器管理员提供的指纹核对一致后，再继续登录。完整示例见[连接第一台 SSH 主机](ssh.md)。

## 启动 AI 命令行工具

已经安装并登录 Claude Code 或 Codex 时，可以直接在项目终端中运行 `claude` 或 `codex`。先试一个范围明确的任务，例如“介绍这个项目的目录结构，先不要修改文件”。

Pebrel 会为识别出的工具显示图标和活动状态。如何查看回答、找回历史对话，见[使用 AI 命令行工具](ai-start.md)。

## 调整字体和外观

点击窗口中的设置按钮，打开 **外观**。文字太小时，先调整 **终端字号**；侧栏、菜单和按钮的文字则由 **界面字号** 控制。主题、字体和行高的详细设置见[外观](appearance.md)。

## 结束工作

运行中的命令通常可以按 **Ctrl+C** 停止；在 Windows / Linux 中有文字选中时，先点击终端空白处清除选区，再按 Ctrl+C。出现提示符后，输入 `exit` 退出 Shell。

希望下次打开时恢复标签页和目录，可以在 **设置 → 高级 → 会话生命周期** 开启 **启动时恢复上次标签**。需要在 Windows 关窗后继续运行任务，请另行开启[后台会话保留](sessions.md)。
