# Pebrel 0.4.4 手机连接测试 / Mobile connection preview

## 局域网测试

1. 将电脑 ZIP 解压到新的可写目录，双击 `Start-Pebrel-Preview.cmd`。
   此入口使用包内独立的 `preview-profile`，不替换已安装的 Pebrel 或旧测试目录。
2. 在 Pebrel **设置 → 手机连接** 选择 **局域网**。手机与电脑连接同一 Wi-Fi。
3. 确认地址是实际联网网卡的地址；必要时点击刷新或在下拉框切换。
   点 **生成二维码**，直接用 Android Pebrel 的连接电脑入口扫码。
4. 如需在手机控制终端，先打开 **允许手机控制终端**，再生成二维码。
   默认只读；更改权限、地址或端口会停止当前连接，需要重新生成。
5. 如果 Windows 询问防火墙权限，只允许你信任的专用网络。
   不需要安装 Node.js、Docker、打开浏览器或手动运行配对命令。

关闭设置页不会断开已启动的连接；**停止连接**会断开手机，但不会结束电脑任务。
二维码包含连接凭据，请勿发送截图、公开二维码或分享系统凭据。
此次局域网继续兼容 Android 0.4.3；建议覆盖安装配套 0.4.4 APK 测试输入区修复。

## 中转测试边界

设置内同时有中转服务器入口，可以导入原生 v2 服务导出的连接配置，生成原生二维码。
新 v2 链路使用端到端加密，需配套 0.4.4 手机构建；旧 0.4.3 不支持。
二维码 5 分钟有效且只使用一次；配对后使用单独的设备凭据重新连接。
协议失败不自动降级。显式导入旧 v1 配置仍是旧的 TLS 分段加密，服务器可见内容。

**一键 SSH 安装、更新和卸载的设置卡片尚未接入，本包不将其标记为可用。**
没有服务器也能完整测试局域网。请勿为了这次局域网测试去安装 Docker 或中转服务。

## 建议反馈

- 设置内是否能看到二维码、手机是否能读到会话列表。
- 停止后重新生成是否成功，选错网卡时是否能切换恢复。
- 手机底部直接输入快捷栏与编辑框是否在同一位置互相切换。
- 失败连接是否不再进入已保存列表，电脑会话能否双指缩放。

可发不含凭据的状态文字或界面截图，不要发送二维码、访问密钥或服务器配置。
本包是分支预览，不是正式版本发布。已运行定向连接与输入区测试，未运行模拟器，
真实设备、网络、防火墙及视觉效果仍由此次人工测试验收。
准确源码与包内文件哈希见 `SOURCE_COMMIT` / `BUILD.json`。

## English

Extract into a new writable directory and run `Start-Pebrel-Preview.cmd`.
Open **Settings → Mobile connection → Local network**, select the computer’s
reachable Wi-Fi address, then generate the QR and scan it from Pebrel Mobile.
Control is off by default; enable it before generating the QR if needed.
Allow Pebrel only on a trusted private network when the firewall asks.
No Node.js, Docker, browser or pairing command is required on the computer.

The launcher uses an isolated `preview-profile`. Closing Settings preserves an
established link; Stop connection disconnects the phone without ending PC tasks.
Do not share the QR or credentials. LAN remains compatible with Android 0.4.3;
the paired 0.4.4 build includes the input-mode and connection-state fixes.

The relay option imports native v2 service access settings and generates its QR
inside Pebrel. v2 requires the new phone build, uses end-to-end encryption and
one-use five-minute invitations, and never silently downgrades to v1. Explicitly
imported legacy v1 settings retain per-hop TLS: that relay can read the content.
The one-click SSH install/update/uninstall card is **not connected yet**.
You do not need any server to test LAN.

Focused tests were run; no emulator or physical-phone/visual acceptance is
claimed. This is a branch preview, not a stable release. Source and hashes are
recorded in `SOURCE_COMMIT` and `BUILD.json`.
