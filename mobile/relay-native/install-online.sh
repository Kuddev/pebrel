#!/bin/sh
# Release packaging replaces the three @...@ tokens with a fixed tag and hashes.
# Download/UX adapter only: the native executable owns all persistent lifecycle
# operations, permissions, certificates, ownership checks and uninstall rules.
set -eu
umask 077
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
release_base='https://github.com/Kuddev/pebrel/releases/download/@RELEASE_TAG@'
installed_binary=/opt/pebrel-relay/pebrel-relay
action=install
address=
port=
binary_source=
step=1
scratch=
locked=0

say() { printf '%s\n' "$*"; }
fail() { say "$*" >&2; exit 1; }
cleanup() {
    code=$?
    trap - 0
    if [ -n "$scratch" ]; then
        # Only files created in this invocation's mktemp directory.
        rm -f "$scratch/pebrel-relay" "$scratch/pebrel-relay.part"
        rmdir "$scratch" 2>/dev/null || :
    fi
    if [ "$locked" = 1 ]; then rmdir /run/pebrel-relay-installer.lock 2>/dev/null || :; fi
    if [ "$code" != 0 ]; then
        printf '\nFAILED at step %s / 第 %s 步失败 (exit %s)\n' "$step" "$step" "$code" >&2
        say 'No unrelated service was stopped. / 未停止其他服务。' >&2
    fi
    exit "$code"
}
trap cleanup 0
trap 'exit 130' INT
trap 'exit 143' TERM

usage() {
    say 'Pebrel relay / Pebrel 中转服务'
    say 'sh pebrel-relay.sh [install] [--address IP_OR_HOST] [--port PORT]'
    say 'sh pebrel-relay.sh status|start|stop|uninstall|purge'
    say 'Default: 443; if occupied, 8443. / 默认 443，占用时改为 8443。'
    say 'Install asks for a reachable IP/domain. No domain is required.'
    say '安装时只需输入电脑与手机均可访问的服务器 IP 或域名，不强制使用域名。'
    say 'uninstall keeps credentials; purge explicitly removes them too.'
    say 'uninstall 保留配置；purge 同时清除配对凭据（不可恢复）。'
    say 'Advanced/offline: --binary PATH (the same pinned SHA256 is required).'
}
case "${1:-}" in
    install|status|start|stop|uninstall|purge) action=$1; shift;;
    --help|-h) usage; exit 0;;
esac
while [ "$#" -gt 0 ]; do
    case "$1" in
        --address) [ "$#" -ge 2 ] || fail 'Missing address / 缺少地址'; address=$2; shift 2;;
        --port) [ "$#" -ge 2 ] || fail 'Missing port / 缺少端口'; port=$2; shift 2;;
        --binary) [ "$#" -ge 2 ] || fail 'Missing binary path / 缺少程序路径'; binary_source=$2; shift 2;;
        --help|-h) usage; exit 0;;
        *) usage; fail 'Unknown option / 无效参数';;
    esac
done

say '1/4 Check server / 检查服务器'
[ "$(uname -s)" = Linux ] || fail 'Linux required / 需要 Linux 服务器'
[ "$(id -u)" = 0 ] || fail 'Run as root: sudo sh pebrel-relay.sh / 请用 root 执行'
case "$(uname -m)" in
    x86_64) arch=x86_64; expected='@X86_64_SHA256@';;
    aarch64|arm64) arch=aarch64; expected='@AARCH64_SHA256@';;
    *) fail 'Supported: Linux x64 / ARM64 / 仅支持 Linux x64、ARM64';;
esac
case "$expected" in *@*) fail 'Use the published pebrel-relay.sh, not the source template. / 请使用发布的脚本，不要执行源码模板。';; esac
for tool in sha256sum mktemp chmod awk cp rm rmdir; do
    command -v "$tool" >/dev/null 2>&1 || fail "Missing tool / 缺少工具: $tool"
done
if [ -d /run/systemd/system ]; then
    [ -x /usr/bin/systemctl ] || fail 'systemctl missing / 缺少 systemctl'
    manager=systemd
elif [ -d /run/openrc ]; then
    for tool in openrc-run rc-service rc-update supervise-daemon; do
        [ -x "/sbin/$tool" ] || fail "Missing OpenRC component / 缺少 OpenRC 组件: $tool"
    done
    manager=openrc
else
    fail 'A running systemd 239+ or OpenRC is required. / 需要运行中的 systemd 239+ 或 OpenRC，普通无服务管理器容器暂不支持。'
fi
if [ "$action" != install ] && { [ -n "$address" ] || [ -n "$port" ]; }; then
    fail '--address/--port only apply to install / 地址和端口仅用于安装'
fi

# Linux exposes IPv4 and IPv6 listening sockets without ss/netstat dependencies.
# This is a preflight, not a reservation: a subsequent bind failure remains fatal.
port_busy() {
    hex_port=$(printf '%04X' "$1")
    [ -r /proc/net/tcp ] || fail 'Cannot check listening ports / 无法检查监听端口'
    if [ -r /proc/net/tcp6 ]; then
        awk -v p=":$hex_port" '$4 == "0A" && substr($2, length($2)-4) == p { found=1 } END { exit !found }' /proc/net/tcp /proc/net/tcp6
    else
        awk -v p=":$hex_port" '$4 == "0A" && substr($2, length($2)-4) == p { found=1 } END { exit !found }' /proc/net/tcp
    fi
}
retained=0
if [ -f /opt/pebrel-relay/installation.json ]; then retained=1; fi
if [ "$action" = install ]; then
    if [ "$retained" = 1 ]; then
        [ -z "$address" ] && [ -z "$port" ] || fail 'Existing configuration is retained. Rerun without --address/--port. / 已有配置将保留，请去掉地址与端口参数后重试。'
        address=localhost
        port=443
        say 'Existing identity and port will be retained. / 保留现有配对身份和端口。'
    else
        if [ -z "$address" ]; then
            printf 'Server IP or domain / 服务器公网 IP 或域名（不带端口）: '
            IFS= read -r address || fail 'Provide --address IP_OR_HOST / 请指定服务器地址'
        fi
        case "$address" in ''|-*|*[!A-Za-z0-9.:-]*) fail 'Use an IP or domain, without https://, spaces or a path. / 请输入纯 IP 或域名，不带协议、空格或路径。';; esac
        if [ -z "$port" ]; then
            port=443
            if port_busy 443; then
                port=8443
                say '443 occupied; using 8443. / 443 已被占用，改用 8443。'
            fi
        fi
        case "$port" in ''|*[!0-9]*) fail 'Invalid port / 端口无效';; esac
        # Strip leading zeros so shell arithmetic/printf cannot interpret octal.
        port=$(printf '%s\n' "$port" | awk '{printf "%.0f", $0+0}')
        [ "$port" -ge 1 ] && [ "$port" -le 65535 ] || fail 'Port must be 1–65535 / 端口范围为 1–65535'
        if port_busy "$port"; then fail "Port $port is occupied; rerun with --port PORT. / 端口 $port 已被占用，请用 --port 指定空闲端口。"; fi
    fi
fi
if [ "$action" != status ]; then
    mkdir /run/pebrel-relay-installer.lock 2>/dev/null || fail 'Another install/management command may be running. / 另一个安装或管理命令可能正在运行。'
    locked=1
fi

step=2
say '2/4 Download and verify executable / 下载并校验服务程序'
scratch=$(mktemp -d /tmp/pebrel-install.XXXXXXXX)
binary=$scratch/pebrel-relay
if [ -n "$binary_source" ]; then
    cp "$binary_source" "$binary.part"
else
    url=$release_base/pebrel-relay-linux-$arch
    downloaded=0
    if command -v curl >/dev/null 2>&1; then
        if curl --proto '=https' --proto-redir '=https' --fail --location --show-error --progress-bar \
            --connect-timeout 15 --max-time 180 --retry 2 "$url" -o "$binary.part"; then downloaded=1; fi
    fi
    if [ "$downloaded" = 0 ] && command -v wget >/dev/null 2>&1; then
        if wget -T 30 -t 3 "$url" -O "$binary.part"; then downloaded=1; fi
    fi
    [ "$downloaded" = 1 ] || fail 'Download failed; curl or wget and HTTPS access to GitHub are required. / 下载失败，需要 curl 或 wget，并能通过 HTTPS 访问 GitHub。'
fi
actual=$(sha256sum "$binary.part")
[ "${actual%% *}" = "$expected" ] || fail 'SHA256 mismatch. The download will NOT be executed. / SHA256 不符，拒绝执行下载的文件。'
mv "$binary.part" "$binary"
chmod 700 "$binary"
"$binary" --version
step=3
case "$action" in
    install)
        say '3/4 Install and start service / 安装并启动服务'
        "$binary" service-install --source "$binary" --sha256 "$expected" --address "$address" --port "$port"
        ;;
    status) say '3/4 Read service state / 读取服务状态';;
    start|stop)
        say "3/4 $action service / 执行服务操作"
        "$binary" "service-$action"
        ;;
    uninstall|purge)
        say '3/4 Stop and uninstall owned service / 停止并卸载本软件服务'
        if [ "$action" = purge ]; then "$binary" service-uninstall --purge
        else "$binary" service-uninstall; fi
        ;;
esac
step=4
say '4/4 Verify result / 检查结果'
if [ "$action" = install ] || [ "$action" = start ]; then
    "$binary" probe --config /etc/pebrel-relay/relay.json
fi
"$binary" service-status
case "$action" in
    install|start)
        say 'READY: local TLS check passed. / 服务已就绪，本机加密连接检查通过。'
        if [ "$action" = install ] && [ "$retained" = 0 ]; then
            printf 'Port / 端口: %s\n' "$port"
        fi
        say 'Allow the relay TCP port in the cloud security group/firewall if needed. This script changes neither.'
        say '如有云安全组或防火墙，请允许中转 TCP 端口；脚本未更改这些规则。'
        say 'PC: Settings → Phone connection → Relay → Use this relay server.'
        say '电脑：设置 → 手机连接 → 中转服务器 → 使用此中转服务器，再生成二维码。'
        ;;
    uninstall) say 'Service removed; pairing configuration retained. / 服务已移除，配对配置已保留。';;
    purge) say 'Service and pairing credentials removed. / 服务与配对凭据已删除，需重新配对。';;
esac
say 'Manage / 管理: sh pebrel-relay.sh status|start|stop|uninstall|purge'
