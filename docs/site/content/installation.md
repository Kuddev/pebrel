## 选择安装包

打开 [Pebrel Releases](https://github.com/Kuddev/pebrel/releases)。本指南对应 **1.9.1**，安装包名称包含版本、系统和处理器类型。

| 电脑 | 选择的包 | 系统要求 |
| --- | --- | --- |
| Windows，Intel / AMD 64 位处理器 | `windows-x64-setup.exe` 或 `windows-x64.zip` | Windows 10 1809+ / Windows 11 |
| Windows，ARM 处理器 | `windows-arm64.zip` | 使用 ARM64 对应包 |
| Mac，Apple 芯片 | `macos-arm64-preview.dmg` | 构建目标为 macOS 14+ |
| Mac，Intel 处理器 | `macos-x64-preview.dmg` | 构建目标为 macOS 14+ |
| Linux，Intel / AMD 64 位处理器 | `linux-x64-preview.deb`、`.AppImage` 或 `.tar.gz` | glibc 2.35+ |

在 Mac 的 **苹果菜单 → 关于本机** 中可以查看芯片类型。`x64` 指 64 位 Intel / AMD，不是 32 位 x86。macOS 和 Linux 当前提供 Preview 包；macOS 构建目标为 14+，发布 CI 在 macOS 15 上验证。

## Windows

### 使用安装程序

1. 下载名称以 `windows-x64-setup.exe` 结尾的文件。
2. 运行安装程序，按页面提示选择安装位置并完成安装。
3. 从开始菜单或安装完成后的入口启动 Pebrel。
4. 看到终端提示符后，输入 `echo Hello, Pebrel` 检查运行情况。

### 使用便携版

1. 下载对应架构的 ZIP 文件。
2. **完整解压**到一个可写目录。
3. 进入解压后的目录，运行 `pebrel.exe`。

保留与可执行文件一起提供的目录和资源；不要在压缩包预览窗口里直接启动，也不要只复制一个 EXE。需要随身携带配置时，参阅[配置文件](configuration.md)和[备份与迁移](migration.md)。

## macOS

1. 下载 Apple Silicon（arm64）或 Intel（x64）对应的 DMG。
2. 打开 DMG，将 **Pebrel** 拖入 **Applications／应用程序**。
3. 弹出 DMG，然后从“应用程序”启动 Pebrel。

如果系统拦截首次启动，先确认安装包来自项目官方 Release，再打开 **系统设置 → 隐私与安全性 → 仍要打开**，按系统提示确认。无需为此全局关闭 Gatekeeper。

<figure><img src="@ROOT@assets/screenshots/pebrel-1.9.1-macos-arm64-ci.png" alt="Pebrel 1.9.1 在 macOS Apple Silicon 发布 CI 中启动后的真实窗口" loading="lazy"><figcaption>1.9.1 发布 CI 中安装并启动的 macOS 版本。</figcaption></figure>

## Linux

### Debian / Ubuntu 安装包

在下载目录打开终端，执行：

```sh
sudo apt install ./Pebrel-v1.9.1-linux-x64-preview.deb
```

安装完成后，从应用菜单打开 Pebrel。使用 `apt install` 安装本地 DEB，可同时处理发行版提供的依赖。

### AppImage

```sh
chmod +x Pebrel-v1.9.1-linux-x64-preview.AppImage
./Pebrel-v1.9.1-linux-x64-preview.AppImage
```

如果提示缺少 FUSE，请按照你的发行版说明安装兼容组件，或改用 DEB / TAR.GZ 包。

### TAR.GZ 便携包

将下载包解压，进入其中的目录后运行 `./AppRun`。保留解压后的完整目录，启动器需要读取随包资源。

## 平台功能差异

| 功能 | Windows | macOS | Linux |
| --- | --- | --- | --- |
| 本地终端、标签页、分屏、SSH、SFTP | 支持 | 支持 | 支持 |
| 系统通知 | 支持 | 支持 | 支持，取决于桌面环境 |
| 关窗保留后台会话、托盘、全局快速终端 | 支持 | 暂无 | 暂无 |
| 本地 AI Hook 自动设置 | 支持 | 暂无 | 暂无 |
| 应用内更新安装 | Windows 安装版 | 已有更新安装入口，参见更新页 | 手动更新 |

Linux 保存密码依赖可用且已解锁的系统凭据服务。macOS 使用系统钥匙串。有关认证方式，见[SSH 认证](ssh-auth.md)。

## 已经安装过 Nebula

Pebrel 是项目更名后的名称。首次运行会尝试迁移旧版配置，保留原文件且不覆盖已有 Pebrel 数据。先备份旧配置，再安装新版本；具体数据位置与迁移步骤见[备份与迁移](migration.md)。

无法出现窗口、资源报错或 Shell 启动失败时，见[故障排查](troubleshooting.md)。
