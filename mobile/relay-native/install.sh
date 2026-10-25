#!/bin/sh
set -eu

fail() {
    printf 'Installation stopped: %s\n' "$1" >&2
    exit 1
}

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    printf 'Usage: sh install.sh SERVER_IP [PORT]\n' >&2
    exit 2
fi
address=$1
port=${2:-443}
case "$port" in ''|*[!0-9]*) fail 'PORT must be an integer from 1 to 65535' ;; esac
[ "${#port}" -le 5 ] && [ "$port" -ge 1 ] && [ "$port" -le 65535 ] ||
    fail 'PORT must be an integer from 1 to 65535'

printf '1/4 Check Linux, root access and CPU architecture\n'
[ "$(uname -s)" = Linux ] || fail 'Linux is required'
[ "$(id -u)" = 0 ] || fail 'Run this script as root'
case "$(uname -m)" in
    x86_64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) fail 'Supported architectures: x86_64 and aarch64' ;;
esac
command -v sha256sum >/dev/null 2>&1 || fail 'sha256sum is required'
directory=$(CDPATH= cd -P -- "$(dirname -- "$0")" && pwd)
binary=$directory/$arch/pebrel-relay
checksums=$directory/$arch/SHA256SUMS
[ -f "$binary" ] && [ ! -L "$binary" ] || fail 'The bundled relay binary is missing or is a symlink'
[ -f "$checksums" ] && [ ! -L "$checksums" ] || fail 'The bundled checksum is missing or is a symlink'

printf '2/4 Verify the bundled relay before executing it\n'
IFS=' ' read -r expected filename < "$checksums" || fail 'Invalid checksum file'
[ "$filename" = pebrel-relay ] && [ "${#expected}" -eq 64 ] || fail 'Invalid checksum entry'
case "$expected" in *[!0-9a-f]*) fail 'Invalid SHA256 digest' ;; esac
(cd -- "$directory/$arch" && sha256sum -c SHA256SUMS) || fail 'Relay checksum mismatch'
chmod 700 "$binary"

printf '3/4 Install, start and verify the relay service\n'
# 服务管理、权限、配置保留和就绪检查以二进制为唯一实现，脚本不复制安装逻辑。
"$binary" service-install --source "$binary" --sha256 "$expected" --address "$address" --port "$port"

printf '4/4 Show the installed service status\n'
"$binary" service-status
printf 'Next: select this SSH server in Pebrel desktop Settings > Phone connection > Relay.\n'
