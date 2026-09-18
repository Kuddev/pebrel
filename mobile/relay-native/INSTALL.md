# Manual native relay installation / 手动安装原生中转

This offline kit contains Linux x86_64 and aarch64 static executables extracted
from the verified preview APK, their hashes and exact source manifests. It does
not use Docker, Node.js, a domain name or an online installer. Check the archive
SHA256 against the separately supplied delivery checksum before uploading.

本离线包包含从已核验 APK 提取的 Linux x64 / ARM64 静态程序、哈希和源码清单。
不需要 Docker、Node.js、域名或在线下载脚本。先按交付的 SHA256 核对压缩包，
再上传到服务器。支持 Alpine OpenRC 或 systemd 247+，需要 root 登录。
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
