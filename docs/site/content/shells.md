## 为新终端选择 Shell

按 **Ctrl+K**（macOS 为 **⌘K**）打开 Shell 选择器，选择要启动的终端程序。列表取决于当前电脑已安装的 Shell，以及你配置的启动入口。

Windows 可以使用 PowerShell、命令提示符以及已安装的 WSL 发行版。macOS 和 Linux 使用系统中可用的 Shell。选择一个入口后，Pebrel 会为它建立终端会话。

## 更改默认 Shell

1. 打开 **设置 → 终端**。
2. 在 **默认 Shell**中选择常用的程序。
3. 新建终端，检查提示符是否来自所选 Shell。

已打开的 Shell 会继续运行。更改默认项不会中途替换它们。

找不到刚安装的 Shell 时，先确认该程序可以独立启动，再重新打开 Pebrel。便携终端可以通过设置中的终端目录选择入口添加；应选择包含可执行文件的目录，或其上一级安装目录。

## 设定启动目录

1. 打开 **设置 → 终端 → 启动**，找到 **启动目录**。
2. 点击当前路径或“继承当前目录”，在文件夹选择窗口中选取项目目录。
3. 新建终端，检查是否进入所选目录。

要取消固定启动目录，点击路径右侧的 **清除**。该项会重新显示“继承当前目录”，新会话可沿用已有终端的已知目录。

输入 `pwd` 查看实际目录；cmd 使用 `cd`。如果 Shell 自己的启动脚本也执行了 `cd`，最终目录会受该脚本影响。

## 配置带参数的启动入口

例如，你希望在选择器中直接启动一个带指定参数的外部程序，可以在 Lua 配置中添加 Profile：

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

config.profiles = {
  {
    name = 'SSH — 开发服务器',
    command = 'ssh',
    args = { 'dev@example.com' },
  },
}

return config
```

把示例地址换成自己的服务器。保存后执行 `pebrel config check` 检查配置，再从 Shell 选择器打开这个入口。

该例使用外部 `ssh` 命令。需要 Pebrel 的主机管理、连接测试和 SFTP 文件面板时，使用[内置 SSH 连接](ssh.md)。

## Shell 打不开时

如果创建标签后立即退出，先从系统终端运行同一程序，检查路径和启动参数。自定义 Profile 中，程序路径放在 `command`，每个参数分别放进 `args`，不要把整条命令和所有参数合成程序名。

WSL 发行版入口与 Windows 本地 Shell 的文件路径不同，具体见[使用 WSL](wsl.md)。
