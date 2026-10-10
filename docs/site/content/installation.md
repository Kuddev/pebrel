## 选择安装包

打开 [Pebrel Releases](https://github.com/Kuddev/pebrel/releases)。本指南对应 **2.1.1**，安装包名称包含版本、系统和处理器类型。更新版本的文件名只有版本号不同。

| 电脑 | 选择的包 | 系统要求 |
| --- | --- | --- |
| Windows，Intel / AMD 64 位处理器 | `Pebrel-v2.1.1-windows-x64-setup.exe` 或 `Pebrel-v2.1.1-windows-x64.zip` | Windows 10 1809+ / Windows 11 |
| Windows，ARM 处理器 | `Pebrel-v2.1.1-windows-arm64-setup.exe` 或 `Pebrel-v2.1.1-windows-arm64.zip` | Windows 10 1809+ / Windows 11，ARM64 |
| Mac，Apple 芯片 | `Pebrel-v2.1.1-macos-arm64-preview.dmg` | 构建目标为 macOS 14+ |
| Mac，Intel 处理器 | `Pebrel-v2.1.1-macos-x64-preview.dmg` | 构建目标为 macOS 14+ |
| Linux，Intel / AMD 64 位处理器 | `Pebrel-v2.1.1-linux-x64-preview.deb`、`Pebrel-v2.1.1-linux-x64-preview.AppImage` 或 `Pebrel-v2.1.1-linux-x64-preview.tar.gz` | glibc 2.35+ |

在 Mac 的 **苹果菜单 → 关于本机** 中可以查看芯片类型。`x64` 指 64 位 Intel / AMD，不是 32 位 x86。macOS 和 Linux 当前提供 Preview 包；macOS 构建目标为 14+，发布 CI 在 macOS 15 上验证。

同一页面还有 `Pebrel-v2.1.1-android-universal-preview.apk`，它是手机端，不是桌面安装包，用法见[Android 连接电脑](mobile.md)。`SHA256SUMS` 用来核对下载文件。

## Windows

### 使用安装程序

1. 下载 `Pebrel-v2.1.1-windows-x64-setup.exe`。
2. 运行安装程序，按页面提示选择安装位置并完成安装。
3. 从开始菜单或安装完成后的入口启动 Pebrel。
4. 看到终端提示符后，输入 `echo Hello, Pebrel` 检查运行情况。

ARM 处理器的电脑同样使用安装程序，下载 `Pebrel-v2.1.1-windows-arm64-setup.exe`。

### 使用便携版

1. 下载 `Pebrel-v2.1.1-windows-x64.zip` 或 `Pebrel-v2.1.1-windows-arm64.zip`。
2. **完整解压**到一个可写目录。
3. 进入解压后的目录，运行 `pebrel.exe`。

保留与可执行文件一起提供的目录和资源；不要在压缩包预览窗口里直接启动，也不要只复制一个 EXE。需要随身携带配置时，参阅[配置文件](configuration.md)和[备份与迁移](migration.md)。

## macOS

1. 下载 Apple Silicon 的 `Pebrel-v2.1.1-macos-arm64-preview.dmg` 或 Intel 的 `Pebrel-v2.1.1-macos-x64-preview.dmg`。
2. 打开 DMG，将 **Pebrel** 拖入 **Applications／应用程序**。
3. 弹出 DMG，然后从“应用程序”启动 Pebrel。

如果系统拦截首次启动，先确认安装包来自项目官方 Release，再打开 **系统设置 → 隐私与安全性 → 仍要打开**，按系统提示确认。无需为此全局关闭 Gatekeeper。

<figure><img src="@ROOT@assets/screenshots/pebrel-1.9.1-macos-arm64-ci.png" alt="Pebrel 1.9.1 在 macOS Apple Silicon 发布 CI 中启动后的真实窗口" loading="lazy"><figcaption>来自 1.9.1 发布 CI 的 macOS 启动截图，仅用于说明首次启动的样子；2.1.1 的界面细节以实际安装后的窗口为准。</figcaption></figure>

## Linux

### Debian / Ubuntu 安装包

在下载目录打开终端，执行：

```sh
sudo apt install ./Pebrel-v2.1.1-linux-x64-preview.deb
```

安装完成后，从应用菜单打开 Pebrel。使用 `apt install` 安装本地 DEB，可同时处理发行版提供的依赖。

### AppImage

```sh
chmod +x Pebrel-v2.1.1-linux-x64-preview.AppImage
./Pebrel-v2.1.1-linux-x64-preview.AppImage
```

如果提示缺少 FUSE，请按照你的发行版说明安装兼容组件，或改用 DEB / TAR.GZ 包。

### TAR.GZ 便携包

解压 `Pebrel-v2.1.1-linux-x64-preview.tar.gz`，进入其中的目录后运行 `./AppRun`。保留解压后的完整目录，启动器需要读取随包资源。

## 平台功能差异

| 功能 | Windows | macOS | Linux |
| --- | --- | --- | --- |
| 本地终端、标签页、分屏、SSH、SFTP | 支持 | 支持 | 支持 |
| 系统通知 | 支持 | 支持 | 支持，取决于桌面环境 |
| 系统托盘、关窗保留后台会话、登录时启动 | 支持 | 支持 | 支持，托盘需要桌面提供状态通知器 |
| 全局快速终端热键 | 支持 | 支持 | 支持，Wayland 通过桌面门户注册，会按系统流程请求授权 |
| 本地 AI 集成自动设置 | 支持 | 支持 | 支持 |
| 应用内更新安装 | 安装版支持 | 支持，替换应用包 | 不提供，使用包管理器或下载新包 |

macOS 和 Linux 的这些集成从 2.1 版开始提供，对应的设置项见[设置索引](settings.md)。通过 Scoop 或 Microsoft Store 等渠道安装的 Windows 版本，由原渠道负责更新，见[更新](updates.md)。

Linux 保存密码依赖可用且已解锁的系统凭据服务。macOS 使用系统钥匙串。有关认证方式，见[SSH 认证](ssh-auth.md)。

## 已经安装过 Nebula

Pebrel 是项目更名后的名称。首次运行会尝试迁移旧版配置，保留原文件且不覆盖已有 Pebrel 数据。先备份旧配置，再安装新版本；具体数据位置与迁移步骤见[备份与迁移](migration.md)。

无法出现窗口、资源报错或 Shell 启动失败时，见[故障排查](troubleshooting.md)。
