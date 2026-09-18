# Pebrel 0.4.10 中转安装测试 / Relay installation preview

## 中文

桌面设置页继续使用 0.4.9 的原生局域网 / 中转二维码、SSH 和备份布局。
本次重点是服务器安装；不是新的终端渲染或网络性能改版。

1. 在服务器执行独立 Pebrel Relay 发布页提供的脚本命令，输入服务器 IP 或域名。
   默认使用 443，被占用时改为 8443；两者均占用时用 `--port` 明确指定。
2. 服务本机检查通过后，确认云安全组和防火墙允许所选 TCP 端口。
3. 电脑设置 → 手机连接 → 中转服务器，选择同一 SSH 主机，使用此服务器后扫码。

支持 systemd 239+ 与 OpenRC。脚本不需要 Docker、Node.js，不结束现有 443 服务。
卸载使用 `sh pebrel-relay.sh uninstall`（保留配置）或 `purge`（清除配对凭据）。
已有不同版本不会被静默覆盖。旧服务能正常运行时，不要先删掉它测试安装。

配套 APK 版本 0.4.10；“上传完成”需等服务器校验通过，不再只依据手机写入进度。
这是分支测试包。定向自动化测试与用户实际服务器、移动网络的验收应分别记录。
不要提供密码、完整配置、二维码或配对密钥。

## English

Desktop retains the 0.4.9 native LAN/relay QR, SSH and backup settings layout.
This preview focuses on server installation, not terminal rendering or latency.

1. Run the script command from the dedicated Pebrel Relay release on the server
   and enter its IP/domain. Default 443 falls back to 8443 when occupied; use
   `--port` if both ports are busy.
2. After local readiness passes, allow the chosen TCP port in your cloud security
   group/firewall as necessary.
3. Select the same SSH host under Settings → Phone connection → Relay, use this
   relay, then scan the desktop QR.

Systemd 239+ and OpenRC are supported without Docker or Node.js. Existing 443
services are not stopped. `sh pebrel-relay.sh uninstall` retains credentials;
`purge` removes them. Other installed binary versions are not silently replaced.
Do not remove a working older relay just to test installation.

Android 0.4.10 waits for remote integrity verification before reporting upload
completion. This is a branch preview; automated checks and real-device/server
acceptance are separate evidence. Never share credentials or complete pairing data.
