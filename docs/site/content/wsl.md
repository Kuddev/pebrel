## 打开已安装的发行版

**此页适用于 Windows 上的 WSL。** 先在 Windows 中安装并完成 WSL 发行版的初始化，再打开 Pebrel。

按 **Ctrl+K**打开 Shell 选择器，选择需要的发行版。进入后，运行：

```sh
pwd
uname -s
```

`uname -s` 应显示 `Linux`。需要检查 Windows 已安装哪些发行版时，可以在 PowerShell 中运行：

```powershell
wsl --list --verbose
```

刚安装的发行版没有出现在选择器中时，先用 Windows 的 WSL 入口启动一次，再重新打开 Pebrel。

## 进入 Linux 或 Windows 项目

Linux 用户目录通常通过 `~`访问。Windows C 盘在常见 WSL 配置下映射到 `/mnt/c`：

```sh
cd ~/projects/my-project
```

或者进入 Windows 上的项目：

```sh
cd /mnt/c/Projects/my-project
```

把路径换成实际目录。项目位于 Linux 文件系统时，优先使用 Linux 路径；需要复制路径给 WSL 命令时，可以使用文件树提供的 **复制 Linux 路径**。

## 将 WSL 作为默认终端

打开 **设置 → 终端 → 默认 Shell**，选择相应发行版，然后新建标签。需要沿用当前 Linux 目录时，在 **启动目录**右侧点击 **清除**，让该项显示“继承当前目录”。

复制已有 WSL 标签也会尽量使用已知工作目录。新标签打开后运行 `pwd`，可以确认目录是否已跟随。

## 安装开发工具和 AI CLI

WSL 有独立的软件环境。需要在 WSL 中运行 Node.js、Python 或 AI CLI 时，在该发行版内安装并登录。Windows 中能运行某个命令，并不代表 WSL 的 Linux PATH 中也有它。

把项目文件交给 AI 之前，先在同一个 WSL 窗格中进入项目目录，再启动工具。

## 复制图片与使用 Pebrel CLI

在 WSL 终端粘贴图片时，Pebrel 会插入适合该环境访问的路径。提交给 CLI 前，检查生成的路径是否完整。

从 Pebrel 启动的本地窗格会提供 CLI 环境信息，WSL 会转换相关路径。可以运行 `pebrel env --pretty` 检查；没有找到命令时，检查 `PEBREL_CLI` 指向的程序是否可从 WSL 执行。控制命令的使用见[终端自动化](runtime.md)。

## 常见问题

**打开后立刻退出：**先从 PowerShell 执行 `wsl -d 发行版名称`。如果也失败，先修复发行版本身的启动问题。

**命令可以运行，但字体或颜色不同：**WSL Shell 的提示符配置与 Windows Shell 分开，终端字体则仍在 Pebrel 的 **外观**中设置。

**文件树目录没有变化：**确认 Shell 能上报当前目录，再使用文件树的跟随和刷新入口。手动浏览过目录后，点击 **跟随当前终端并刷新**恢复跟随。
