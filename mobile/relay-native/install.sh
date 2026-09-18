#!/bin/sh
# Offline installer: same ownership-checked native service command as Android.
# No package downloads, account creation, firewall edits or recursive cleanup.
set -eu
step=1
trap 'code=$?; if [ "$code" != 0 ]; then printf "FAILED at step %s / 第 %s 步失败 (exit %s)\n" "$step" "$step" "$code" >&2; fi' 0
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    printf 'Usage / 用法: sh install.sh SERVER_IP_OR_HOST [PORT]\n' >&2
    exit 2
fi
address=$1
port=${2:-443}
case "$address" in ''|-*|*[!A-Za-z0-9.:-]*) printf 'Invalid server address / 服务器地址无效\n' >&2; exit 2;; esac
case "$port" in ''|*[!0-9]*) printf 'Invalid port / 端口无效\n' >&2; exit 2;; esac
[ "$port" -ge 1 ] && [ "$port" -le 65535 ] || exit 2
printf '1/4 Check Linux, root and running service manager / 检查系统、root 和服务管理器\n'
[ "$(uname -s)" = Linux ] || { printf 'Linux required\n' >&2; exit 1; }
[ "$(id -u)" = 0 ] || { printf 'Root login required / 请用 root 执行\n' >&2; exit 1; }
arch=$(uname -m)
case "$arch" in x86_64|aarch64) ;; *) printf 'Unsupported architecture / 不支持的架构: %s\n' "$arch" >&2; exit 1;; esac
if [ -d /run/systemd/system ]; then
    [ -x /usr/bin/systemctl ] || exit 1
elif [ -d /run/openrc ]; then
    for command in rc-service rc-update openrc-run supervise-daemon; do
        [ -x "/sbin/$command" ] || { printf 'Missing OpenRC component / 缺少组件: %s\n' "$command" >&2; exit 1; }
    done
else
    printf 'No running systemd/OpenRC / 未检测到运行中的 systemd 或 OpenRC；普通 SSH 容器不等于完整服务主机\n' >&2
    exit 1
fi
step=2
printf '2/4 Verify bundled executable / 校验内置服务文件（不联网下载）\n'
package_dir=$(CDPATH= cd -P "$(dirname "$0")" && pwd)
cd "$package_dir/$arch"
sha256sum -c SHA256SUMS
read -r expected_hash unused < SHA256SUMS
chmod 700 pebrel-relay
./pebrel-relay --version
step=3
printf '3/4 Install and start the owned service / 安装并启动服务\n'
./pebrel-relay service-install --source "$package_dir/$arch/pebrel-relay" --sha256 "$expected_hash" --address "$address" --port "$port"
step=4
printf '4/4 Verify local TLS listener / 验证本机 TLS 监听\n'
/opt/pebrel-relay/pebrel-relay probe --config /etc/pebrel-relay/relay.json
/opt/pebrel-relay/pebrel-relay service-status
printf 'DONE / 已完成。电脑：设置 → 手机连接 → 中转服务器 → 使用此中转服务器。\n'
printf 'Check external port reachability separately / 请另行确认电脑和手机能访问所选端口；未修改防火墙。\n'
