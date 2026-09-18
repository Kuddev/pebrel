# Native relay installation / 安装原生中转

## Online installer / 在线脚本（推荐）

Download `pebrel-relay.sh` from the dedicated **Pebrel Relay** GitHub prerelease,
then run `sh pebrel-relay.sh` as root. The exact public download command is included
in that release. Do not execute the unrendered source template: a published script
pins its release and both binary SHA256 values. No GitHub login, Docker, Node.js,
archive upload or extraction is needed on the server.

从独立的 **Pebrel Relay** GitHub 预发布下载 `pebrel-relay.sh`，以 root 执行
`sh pebrel-relay.sh`。对应发布页提供可复制的下载命令。不要执行源码模板；
发布脚本已固定版本与两种架构的 SHA256，不需要 GitHub 登录、Docker、Node.js
或手动上传、解压安装包。

- Enter the server IP or domain when prompted. Default TCP port is 443; a busy
  443 selects 8443. If both are busy, installation stops without killing services.
- `--address HOST --port PORT` explicitly chooses another address/port. A domain
  is optional. Cloud security groups, NAT and firewalls remain user-controlled.
- `sh pebrel-relay.sh status|start|stop|uninstall|purge` manages the same native
  service used by the app. `uninstall` retains pairing; `purge` deletes it.
- Repeated installation retains an existing identity/port. Different installed
  binary versions are not silently overwritten. The script requires outbound HTTPS
  access to GitHub and curl or wget. It fails closed on download/checksum errors.

安装仅询问服务器 IP 或域名；默认 443，被占用则使用 8443，两者都占用时停止并
提示指定端口，不结束其他服务。用 `--address 地址 --port 端口` 可明确指定。
重复安装保留原配对身份与端口，已有不同版本不会被静默覆盖。状态、启停与卸载
仍使用应用共用的原生服务实现，脚本不维护第二套服务规则。`uninstall` 保留配置，
`purge` 删除配对凭据。服务器须有 curl 或 wget 且能通过 HTTPS 访问 GitHub。

For restricted networks, a trusted HTTPS mirror can host the exact release files.
Pass `--download-base https://YOUR-MIRROR/RELEASE` or set `PEBREL_RELAY_MIRROR`.
The mirror is tried first, then GitHub; the embedded checksums never change with
the source. After GitHub fails, public third-party download proxies `gh-proxy.com`
and `ghfast.top` are tried. These are not Pebrel-owned services and have no uptime
guarantee. They receive only public asset requests, never server credentials.
`--github-only` (or `PEBREL_RELAY_GITHUB_ONLY=1`) disables public proxies.
If an installed executable already matches, no download is needed.
The bootstrap script itself also needs to be distributed through that mirror or
sent directly. GitHub/CDN reachability in mainland China is not guaranteed merely
by providing a fallback option; an actual hosted mirror and network checks are needed.

受限网络可用自有 HTTPS 镜像存放同一版本文件，通过 `--download-base` 或
`PEBREL_RELAY_MIRROR` 指定；先访问镜像，失败再访问 GitHub，校验值保持不变。
GitHub 失败后会尝试 `gh-proxy.com` 和 `ghfast.top` 公共第三方下载通道；
它们不是 Pebrel 自营服务，没有可用性保证，只接收公开资产请求，不传递服务器凭据。
可用 `--github-only` 或 `PEBREL_RELAY_GITHUB_ONLY=1` 禁用公共代理。
本机程序版本与校验值一致时无需重新下载。启动脚本本身也需由镜像分发或直接发送。
仅有备用源参数不代表已提供国内可用下载服务，须实际部署镜像并核实网络可达。

Systemd 239–246 starts the same bounded native relay with a read/bind-then-drop
privilege boundary, also used by OpenRC. Systemd 247+ retains credential passing
and DynamicUser. Runtime traffic never runs as root; only installation and the
old-manager single-threaded startup need root. A successful local TLS probe does
not prove public port reachability. No firewall or unrelated service is modified.

systemd 239–246 与 OpenRC 使用程序已有的启动降权路径：读取凭据并绑定端口后，
先永久降权，再创建线程和处理连接；247+ 保留凭据传递与动态用户方案。
本机加密连接检查成功不等于公网端口已放行，脚本不改防火墙或其他服务。

## Offline kit / 离线包

This offline kit contains Linux x86_64 and aarch64 static executables extracted
from the verified preview APK, their hashes and exact source manifests. It does
not use Docker, Node.js, a domain name or an online installer. Check the archive
SHA256 against the separately supplied delivery checksum before uploading.

本离线包包含从已核验 APK 提取的 Linux x64 / ARM64 静态程序、哈希和源码清单。
不需要 Docker、Node.js、域名或在线下载脚本。先按交付的 SHA256 核对压缩包，
再上传到服务器。新版支持 Alpine OpenRC 或 systemd 239+，需要 root 登录。
历史 0.4.6 离线包仍包含旧安装器，不具备本次兼容性修复。
没有正在运行的服务管理器的 SSH 容器不属于此安装器支持的主机，不要伪造
`/run/openrc` 或创建 `softlevel` 文件来绕过检查。

## Upload / 上传（电脑终端）

Replace `SERVER_IP` with the same reachable address used for SSH. The command
uses OpenSSH scp's legacy protocol for Alpine hosts without an SFTP subsystem.
For a nonstandard SSH port add `-P 2222` to scp and `-p 2222` to ssh.

把 `SERVER_IP` 换成你的 SSH 服务器地址。这里 `scp -O` 兼容未提供 SFTP 的
Alpine SSH 主机；非默认 SSH 端口给 scp 加 `-P 2222`，给 ssh 加 `-p 2222`。

```sh
scp -O Pebrel-Relay-manual-0.4.6.tar.gz root@SERVER_IP:/root/
ssh root@SERVER_IP
```

## Install / 安装（root 服务器终端）

Extract in a new directory so a previous kit is not overwritten. `SERVER_IP`
must be reachable by both devices; it is not automatically detected. Default
port 443 may be occupied by a web server; choose e.g. 8443 if necessary.
Pebrel never changes the host/cloud firewall or terminates another service.

解压到新目录，避免覆盖以前的手动包。下面第二条命令的 `SERVER_IP` 仍需换成
电脑和手机均可访问的服务器地址。443 若被网站占用，可改成 8443；自行确认
主机防火墙和云安全组允许该端口。安装器不修改防火墙，也不停止其他服务。

```sh
kit_dir=$(mktemp -d /root/pebrel-relay-kit.XXXXXXXX)
tar -xzf /root/Pebrel-Relay-manual-0.4.6.tar.gz -C "$kit_dir"
cd "$kit_dir/pebrel-relay-manual"
sh install.sh SERVER_IP 443
```

The script stops with the failing step. Success requires the TLS health probe,
not merely copying the file or starting a process. `ready:true` confirms the
local listener, **not** external network reachability. Keep this kit for recovery.

脚本会输出 1–4 步，失败停在对应步骤。完成必须通过实际 TLS 探测，不只复制文件
或启动进程。`ready:true` 证明本机监听就绪，**不代表公网端口一定可达**。
保留解压目录，供卸载后重新安装或清理保留配置使用。

## Status and management / 状态与管理

```sh
/opt/pebrel-relay/pebrel-relay service-status
/opt/pebrel-relay/pebrel-relay probe --config /etc/pebrel-relay/relay.json
/opt/pebrel-relay/pebrel-relay service-stop
/opt/pebrel-relay/pebrel-relay service-start
```

Alpine service-manager diagnostics / Alpine 服务管理器状态：

```sh
rc-service pebrel-relay status
rc-status default
```

On systemd / systemd 主机：

```sh
systemctl status pebrel-relay --no-pager
journalctl -u pebrel-relay -n 40 --no-pager
```

After success, on the PC select **Settings → Phone connection → Relay** and the
same SSH server, then **Use this relay server** and generate the pairing QR.
The desktop reads the private configuration over verified SSH. Do not paste
`access.json`, `relay.json`, keys or QR contents into public logs or messages.

成功后在电脑 **设置 → 手机连接 → 中转服务器** 选择同一 SSH 主机，点击
**使用此中转服务器**，生成二维码供手机扫描。电脑通过已验证 SSH 读取配置。
不要发送 `access.json`、`relay.json`、密钥或二维码内容。

## Uninstall / 卸载

This disconnects devices and removes only ownership-verified Pebrel service
files. Keep credentials for reinstall (default), or explicitly delete them too:

这会断开设备，只删除所有权校验通过的 Pebrel 服务文件。两条命令**任选一种**：

```sh
# Keep pairing credentials / 保留配对凭据
/opt/pebrel-relay/pebrel-relay service-uninstall
```

```sh
# Delete service AND pairing credentials / 同时删除服务与配对凭据
/opt/pebrel-relay/pebrel-relay service-uninstall --purge
```

After a keep-credentials uninstall, the installed executable is gone. To purge
later, run `./x86_64/pebrel-relay service-uninstall --purge` from this extracted kit
(use `aarch64` on ARM64). Do not delete broad directories by hand. Reinstall with
the same kit preserves paired identity. Changed owned files or another installed
binary produce a conflict rather than an automatic overwrite/update.

保留凭据卸载后，`/opt` 下的程序已移除。之后要清除配置，请在本包目录执行
`./x86_64/pebrel-relay service-uninstall --purge`（ARM64 换成 `aarch64`）。
不要手工递归删除目录。同版本重装保留配对身份；文件被改动或版本不同会报冲突，
不会自动覆盖或升级。

If installing fails, report only the last numbered step and the fixed JSON error
code. Relevant causes include root access, no running init manager, a `noexec`
upload directory, an occupied port, missing `nobody`, and ownership conflicts.
The same native installer passed isolated Alpine/OpenRC lifecycle checks in CI;
this does not prove compatibility with every server image or cloud firewall.

失败时只需提供最后的步骤编号和 JSON 错误码。常见原因包括非 root、服务管理器
未运行、上传目录 `noexec`、端口占用、缺少 `nobody` 和安装所有权冲突。
原生安装器已经过 CI 隔离 Alpine/OpenRC 生命周期验证；不冒称所有服务器镜像和
云防火墙环境都已验证。
