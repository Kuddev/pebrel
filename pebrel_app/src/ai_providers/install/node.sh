# npm 的用户级 prefix 避免全局目录 sudo；Node 从官方 LTS 校验列表定位。
export PATH="$HOME/.local/bin:$PATH"
if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1 ||
    ! node -e 'process.exit(Number(process.versions.node.split(".")[0]) >= 22 ? 0 : 1)'; then
    case "$(uname -s)" in Darwin) node_os=darwin ;; Linux) node_os=linux ;; *) exit 1 ;; esac
    case "$(uname -m)" in arm64|aarch64) node_arch=arm64 ;; x86_64) node_arch=x64 ;; *) exit 1 ;; esac
    node_stage=$(mktemp -d)
    trap 'rm -rf "$node_stage"' EXIT
    node_base=https://nodejs.org/dist/latest-v22.x
    curl --fail --location --proto '=https' "$node_base/SHASUMS256.txt" -o "$node_stage/sums"
    node_name=$(awk -v suffix="-$node_os-$node_arch.tar.gz" '$2 ~ /^node-v22\.[0-9]+\.[0-9]+-/ && substr($2,length($2)-length(suffix)+1)==suffix {print $2; exit}' "$node_stage/sums")
    test -n "$node_name"
    printf 'Downloading Node.js 22 LTS...\n'
    curl --fail --location --proto '=https' "$node_base/$node_name" -o "$node_stage/$node_name"
    node_expected=$(awk -v name="$node_name" '$2==name {print $1}' "$node_stage/sums")
    if command -v sha256sum >/dev/null 2>&1; then
        node_actual=$(sha256sum "$node_stage/$node_name" | awk '{print $1}')
    else
        node_actual=$(shasum -a 256 "$node_stage/$node_name" | awk '{print $1}')
    fi
    test "$node_actual" = "$node_expected"
    node_root="$HOME/.local/share/pebrel/cli-node"
    mkdir -p "$node_root" "$HOME/.local/bin"
    tar -xzf "$node_stage/$node_name" -C "$node_root"
    node_bin="$node_root/${node_name%.tar.gz}/bin"
    for tool in node npm npx; do
        ln -sf "$node_bin/$tool" "$HOME/.local/bin/$tool"
    done
fi
case "${SHELL:-}" in */zsh) node_rc="$HOME/.zshrc" ;; *) node_rc="$HOME/.profile" ;; esac
if ! grep -Fq 'export PATH="$HOME/.local/bin:$PATH"' "$node_rc" 2>/dev/null; then
    printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$node_rc"
fi
