# Pebrel 0.4.9 手机连接测试 / Mobile connection preview

本轮设置页改动与测试重点见 `SETTINGS-PREVIEW-0.4.9.md`。
See `SETTINGS-PREVIEW-0.4.9.md` for this preview's settings changes and review checklist.

## 局域网测试

1. 将电脑 ZIP 解压到新的可写目录，双击 `Start-Pebrel-Preview.cmd`。
   此入口使用包内独立的 `preview-profile`，不替换已安装的 Pebrel 或旧测试目录。
2. 在 Pebrel **设置 → 手机连接** 选择 **局域网**。手机与电脑连接同一 Wi-Fi。
3. 确认地址是实际联网网卡的地址；必要时点击刷新或在下拉框切换。
   页面会自动准备二维码，直接用 Android Pebrel 的连接电脑入口扫码。
4. 如需在手机控制终端，先打开 **允许手机控制终端**，再生成二维码。
   默认只读；更改权限、地址或端口会重建当前连接，请重新扫码。
5. 如果 Windows 询问防火墙权限，只允许你信任的专用网络。
   不需要安装 Node.js、Docker、打开浏览器或手动运行配对命令。

关闭设置页不会断开已启动的连接；**停止连接**会断开手机，但不会结束电脑任务。
二维码包含连接凭据，请勿发送截图、公开二维码或分享系统凭据。
此次局域网继续兼容 Android 0.4.3；请覆盖安装配套 0.4.9 APK 测试流式画面与恢复连接。
PC 与 SSH 默认使用紧凑快捷栏，点击终端即可输入；气泡按钮切到编辑卡片，关闭按钮切回。
PC 必须已授权终端操作；只读连接会显示原因。电脑配色修复需要本轮电脑包与 APK 配套。
配套新端点主动推送物理网格与逐行增量，不再逐次请求画面；连续输入也不再逐键等回包。
这是有流量控制的网格推送，不是原始 PTY 字节流。旧电脑仍保留兼容的拉取方式。
已连成功的电脑会出现在“会话”中；切后台暂停画面，回前台检查连接并自动恢复。
草稿与原窗格保留，未确认输入不重发；电脑程序重启后需重新选择窗格，证书异常须用户处理。
实际 Wi-Fi / 中转延迟、系统后台限制仍需真机测试，不能承诺所有网络和系统永不掉线。

## 中转测试边界

设置内同时有中转服务器入口，可以导入原生 v2 服务导出的连接配置，生成原生二维码。
新 v2 链路使用端到端加密，建议配套 0.4.9 手机构建；旧 0.4.3 不支持。
二维码 5 分钟有效且只使用一次；配对后使用单独的设备凭据重新连接。
协议失败不自动降级。显式导入旧 v1 配置仍是旧的 TLS 分段加密，服务器可见内容。

手机“连接电脑 → 中转服务器 → 部署服务器”已接入原生服务管理卡片：
填写 SSH 登录信息，点击安装并启动，卡片显示服务端实际阶段。
底部保留 1–4 步状态、实际传输量和失败步骤；“手动安装命令”提供配套离线包的命令。
离线包 Pebrel-Relay-manual-0.4.6.tar.gz 沿用已核验服务程序，与 0.4.9 客户端兼容，
含安装、状态、启停和卸载说明。它不是对未知服务器环境保证成功的声明。
端口、连接地址覆盖位于高级设置，无需域名、Docker、Node.js 或 HTTP 验证端口。
当前支持 Linux x64/ARM64、systemd 247+ 或 Alpine OpenRC，需要 root SSH 登录；不会修改防火墙。
电脑、手机仍需能访问中转端口（默认 443），服务监听就绪不等于公网已可达。
安装后在电脑此设置页选择同一 SSH 服务器（或输入 root@地址），点击“使用此中转服务器”，
通过已验证的 SSH 读取配置，再生成二维码供手机扫描。复用电脑已有的 SSH 凭据；
如尚未连接过该主机，请先从 SSH 主机列表验证登录。文件导入保留在高级设置中。
可检查状态、启动、停止、卸载；卸载默认保留凭据，清除配置须额外确认。
已安装的原生服务无需为这轮流式/重连修改而重新安装，继续使用原有自定义端口。
取消操作不会自动回滚已安装的服务，应先检查状态；不支持自动升级或接管旧 Docker 安装。
电脑设置页负责选择已安装的服务器和二维码，不在电脑上执行 SSH 安装。没有服务器也可测试局域网。

## 建议反馈

- 设置内是否能看到二维码、手机是否能读到会话列表。
- 停止后重新生成是否成功，选错网卡时是否能切换恢复。
- 手机底部直接输入快捷栏与编辑框是否在同一位置互相切换。
- 失败连接是否不再进入已保存列表，电脑会话能否双指缩放。
- 连续输入时是否及时显示，局域网和中转两种方式都请验证。
- 切到其他 App 再回来，是否自动恢复原窗格和草稿；断网恢复后是否会重复输入。
- 首页“会话”与“全部会话”是否包含已连接电脑的窗格，并可直接打开。

可发不含凭据的状态文字或界面截图，不要发送二维码、访问密钥或服务器配置。
本包是分支预览，不是正式版本发布。已运行定向连接与输入区测试，未运行模拟器，
真实设备、网络、防火墙及视觉效果仍由此次人工测试验收。
准确源码与包内文件哈希见 `SOURCE_COMMIT` / `BUILD.json`。

## English

Extract into a new writable directory and run `Start-Pebrel-Preview.cmd`.
Open **Settings → Mobile connection → Local network**, select the computer’s
reachable Wi-Fi address, then scan the automatically prepared QR from Pebrel Mobile.
Control is off by default; enable it if needed and scan the regenerated QR.
Allow Pebrel only on a trusted private network when the firewall asks.
No Node.js, Docker, browser or pairing command is required on the computer.

The launcher uses an isolated `preview-profile`. Closing Settings preserves an
established link; Stop connection disconnects the phone without ending PC tasks.
Do not share the QR or credentials. LAN remains compatible with Android 0.4.3;
the paired 0.4.9 build unifies compact/direct input and the optional editor for PC
and SSH. Tap the terminal to type when control is authorized. Pair both new builds
to receive the computer's effective terminal palette.
Paired new endpoints push physical-grid snapshots and row deltas with bounded
credit instead of polling. Ordered input is pipelined rather than waiting one
network round trip per key. This is a grid stream, not raw PTY bytes; older PCs
retain the pull fallback. Previously connected PC panes appear in Sessions.
Background pauses screen observation; foreground probes/reconnects, preserving
the draft and selected pane without replaying uncertain input. A restarted PC
process requires pane reselection; certificate failures require user action.
Network latency and OS background restrictions still need physical-device testing.

The relay option imports native v2 service access settings and generates its QR
inside Pebrel. v2 requires the new phone build, uses end-to-end encryption and
one-use five-minute invitations, and never silently downgrades to v1. Explicitly
imported legacy v1 settings retain per-hop TLS: that relay can read the content.
The Android relay setup now manages the native service over verified SSH, with
four numbered progress steps, actual transferred bytes, the failing step,
status, start/stop and uninstall. Manual installation commands use the companion
Pebrel-Relay-manual-0.4.6.tar.gz offline kit, whose verified service executables
remain compatible with 0.4.9 clients. This does not guarantee installation on
unknown server environments. Linux x64/ARM64, systemd 247+ or
Alpine OpenRC, and root SSH login are required. Ports and address overrides are under Advanced;
no domain, Docker, Node.js or HTTP validation port is needed. No firewall rules
are changed. Both clients must still reach the relay port (443 by default).
Select the same SSH server on this desktop page (or enter root@host) and choose
Use this relay server to read its configuration over verified SSH, then scan
the generated QR. Existing desktop SSH credentials are reused; verify login from
the SSH host list first if needed. File import remains under Advanced.
Uninstall retains credentials unless purge is confirmed.
An existing native relay does not need reinstalling for this streaming/recovery
change; retain its configured custom port.
Cancellation is not rollback; check status before retrying. Automatic updates and
adoption of old Docker installs are not supported. The desktop page imports and
pairs; it does not administer SSH servers. LAN requires no server.

Focused tests were run; no emulator or physical-phone/visual acceptance is
claimed. This is a branch preview, not a stable release. Source and hashes are
recorded in `SOURCE_COMMIT` and `BUILD.json`.
