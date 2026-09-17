# SSH deployment from Android

The Android computer connection form can deploy a relay using a saved SSH host
and its encrypted saved password, or a manually entered SSH endpoint. An address
written as `user@host` has one shared parser with ordinary SSH connections; the
embedded username overrides the separate default username.

The server must run Linux and already provide Docker, Compose, `tar`, `base64`,
`head`, `find`, and either `curl` or `wget`. The SSH account must be allowed to run
Docker. Deployment never runs an unattended package installer or changes the
server firewall. Point the chosen public domain at this server; public HTTP port
80 must reach the configured HTTP challenge port, and the chosen HTTPS port must
be reachable. The defaults are 80 and 443.

`mobile/tools/package_relay.py` creates the source archive bundled in the APK.
Only explicitly listed relay sources, locked npm manifests, the shared RPC policy
and the project license enter the archive. No saved credentials or node_modules
are included. The same sources can be unpacked on the PC as its pairing helper.

The Android adapter performs actual SSH host-key verification and checks server
prerequisites before uploading the archive. The native transport owns one exec
channel per connection, so deployment opens a fresh authenticated connection for
each command and retains the fingerprint accepted at the start. Password arrays
are retained only for this operation; each native connection receives its own
copy. Output, source archive size and operation time are bounded. Progress uses
semantic stages, and neither progress nor errors expose generated role keys.

The default installation path is `~/.pebrel-relay`. A nonempty unrelated directory
is refused; a deployment-owned marker allows retry in the same directory. The
Compose project name includes the account ID and directory identity. Container
images are downloaded as needed; the relay is built from the uploaded source,
with Caddy providing TLS and restart policies keeping the service running. The
configurations use separate mobile and desktop credentials. The result is shown
only after the HTTPS `/healthz` endpoint responds successfully.

Cancel closes the current SSH operation. Files already uploaded and containers
already started remain on the user's server; cancellation does not claim to roll
back remote changes. Retry reuses the same most-recent pairing when its computer
name and endpoint match. Deleting the app does not uninstall server containers.

After deployment, export both configuration files to the PC and use the startup
command shown in the app from the helper's `relay` directory. Node.js 22 and an
already running Pebrel desktop are required. The helper discovers the installed
Runtime API locally and never forwards its local token to the phone or relay.
The optional `--allow-input` switch remains an explicit PC-side choice. Relay TLS
terminates at the user-owned server; this preview does not claim end-to-end
payload encryption or native desktop terminal grid streaming.

## 中文

入口为“连接电脑 → 服务器中转 → 通过 SSH 部署服务”。可以选择已有主机并复用已保存
密码，或手动填写 SSH 地址、端口、用户名和密码。支持直接填写 `user@host`。

服务器需事先安装 Docker 和 Compose，并允许该 SSH 账户使用 Docker。填写公网域名、
HTTPS 端口、HTTP 验证端口、安装目录及电脑名称。域名需要解析到服务器，公网 80 端口
需通向 HTTP 验证端口，并开放所选 HTTPS 端口。应用不会自动安装系统软件或更改防火墙。

应用会真实上传源码、生成独立凭据、构建并启动容器，再检查 HTTPS 服务。取消会关闭
当前 SSH 操作，已上传文件和已启动服务仍保留。无关的非空目录不会被覆盖。

成功后将“电脑配置”和“手机邀请”两份文件保存到电脑连接助手的 `relay` 目录，执行
页面提供的命令。电脑需要 Node.js 22，并保持 Pebrel 桌面程序和连接助手运行；服务器
部署成功本身不会自动接入电脑会话。
