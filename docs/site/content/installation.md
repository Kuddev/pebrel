## 选择安装包

只从 [Pebrel 官方发布页](https://github.com/Kuddev/pebrel/releases/latest)下载你要安装的版本。文件名中的系统与架构必须和你的机器相符。

| 系统 | 架构 | 安装包 | 当前状态 |
| --- | --- | --- | --- |
| Windows 10 1809+ / Windows 11 | x64 | 安装程序 `.exe`、便携 `.zip` | 正式平台 |
| macOS，部署目标 14+ | Apple Silicon / arm64 | `.dmg` | Preview |
| macOS，部署目标 14+ | Intel / x64 | `.dmg` | Preview |
| Linux，glibc 2.35+ | x64 | `.deb`、`.AppImage`、便携 `.tar.gz` | Preview |

例如，Windows 安装程序的命名形式为 `Pebrel-v<版本>-windows-x64-setup.exe`。macOS 与 Linux 预览包的文件名含 `-preview`。

> [!NOTE]
> macOS 的部署目标不等于每个旧系统版本都已完成实机验证；项目的原生 macOS CI 在 macOS 15 上运行。遇到平台问题时，请一并提供系统版本与处理器架构。

## Windows

### 使用安装程序

1. 下载对应版本的 `-setup.exe`。
2. 运行安装程序，按提示完成安装。默认是当前用户安装，通常位于 `%LOCALAPPDATA%\Programs\Pebrel`。
3. 从开始菜单启动 Pebrel。

### 使用便携包

完整解压 ZIP 到可写的文件夹，再运行 `pebrel.exe`。不要只取出 EXE；打包目录中的附带文件需要保留。

便携版更新时先退出应用，再完整替换程序包。安装版与便携版的更新方式不同；没有安装器标记的便携目录不使用 Windows 安装器自动覆盖。

## macOS

1. 在“关于本机”中确认是 Apple 芯片还是 Intel 处理器。
2. 下载对应架构的 DMG，打开并把 Pebrel 拖到 **应用程序**。
3. 从“应用程序”启动，而不是一直从挂载的 DMG 中运行。

预览构建采用临时签名，首次启动可能被系统阻止。确认文件来自官方发布页后，前往 **系统设置 → 隐私与安全性**，使用系统提供的 **仍要打开**，然后再次确认。不要关闭系统整体安全检查来绕过来源核验。

如果仍无法启动，记录 macOS 版本、芯片类型、下载的完整文件名及错误提示，再提交问题。

## Linux

### Debian / Ubuntu 系列

在下载目录打开终端，将文件名替换为你实际下载的 DEB：

```sh
sudo apt install ./Pebrel-v版本-linux-x64-preview.deb
```

### AppImage

```sh
chmod +x Pebrel-v版本-linux-x64-preview.AppImage
./Pebrel-v版本-linux-x64-preview.AppImage
```

若系统缺少 AppImage 所需的运行支持，请按发行版说明配置，或改用 DEB / 便携包；不要把所有启动问题都当作 Pebrel 配置错误。

### 便携包

解压整个 `.tar.gz`，保留原有目录结构，通过包内的 **`AppRun`** 启动。不要只复制底层可执行文件，否则可能丢失启动器负责配置的运行路径。

### 保存 SSH 凭据

Linux 上保存凭据需要可用且已解锁的 Secret Service；还需要 `libsecret-tools`。无法访问桌面密钥环时，可以先不保存密码而进行连接，不必把密码写进共享配置文件。

## 各平台有什么不同

| 能力 | Windows | macOS Preview | Linux Preview |
| --- | --- | --- | --- |
| 终端、分屏、SSH、SFTP、文件与文档界面 | 支持 | 支持 | 支持 |
| 托盘与关闭窗口后后台驻留 | 支持 | 不提供 | 不提供 |
| 全局快速终端热键 | 支持 | 不提供 | 不提供 |
| 自动本地 AI hook 设置 | 支持 | 不提供 | 不提供 |
| 系统通知与系统凭据存储 | 支持 | 支持 | 支持；凭据依赖桌面服务 |
| 应用内安装更新 | 安装版支持 | 应用包更新路径已实现 | 手动安装新包 |

此表依据 1.9.1 的平台能力与更新安装代码，不把未来计划或未合并的改动算作已支持。

## 从旧名称 Nebula 迁移

Pebrel 保留对部分旧配置名称的兼容。在新位置没有相应文件时，会尝试迁移旧设置；**已有的新文件不会被覆盖**，旧文件会保留。

迁移前退出旧应用并备份设置与 SSH 主机信息。安装新版后，检查字体、主题、默认 Shell 与主机列表是否符合预期，再决定是否删除旧程序。不要在确认迁移成功之前清理旧数据。

## 下一步

按照[快速开始](quickstart.md)打开终端并创建第一个分屏。若只是想修改外观，先使用设置界面，不需要编写配置文件。
