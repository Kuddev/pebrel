#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
    echo "usage: bash scripts/install-ci-ubuntu-packages.sh [apt options] package..." >&2
    exit 2
fi

# Resolve Ubuntu packages directly; the hosted runner's mirror list can retry
# an unreachable regional mirror before it reaches the public archive.
. /etc/os-release
if [[ ${ID:-} != ubuntu || ! ${VERSION_CODENAME:-} =~ ^[a-z]+$ ]]; then
    echo "Ubuntu package installation requires a release codename" >&2
    exit 1
fi

arch="$(dpkg --print-architecture)"
case "$arch" in
    amd64)
        archive=https://archive.ubuntu.com/ubuntu
        security=https://security.ubuntu.com/ubuntu
        ;;
    arm64)
        archive=https://ports.ubuntu.com/ubuntu-ports
        security="$archive"
        ;;
    *)
        echo "Unsupported Ubuntu CI architecture: $arch" >&2
        exit 1
        ;;
esac

keyring=/usr/share/keyrings/ubuntu-archive-keyring.gpg
test -r "$keyring"
scratch="$(mktemp -d)"
chmod 0755 "$scratch"
sources="$scratch/ubuntu.list"
touch "$sources"
chmod 0644 "$sources"
cleanup() {
    if [[ -f "$scratch/original-hosts" ]]; then
        sudo tee /etc/hosts < "$scratch/original-hosts" >/dev/null
    fi
    rm -f "$sources" "$scratch/original-hosts"
    rmdir "$scratch"
}
trap cleanup EXIT

resolve_archive_host() {
    local host="${1#https://}"
    host="${host%%/*}"
    if timeout 6s getent ahostsv4 "$host" >/dev/null; then
        return
    fi
    # On disposable CI runners, recover only the official archive hostnames.
    # APT still verifies the HTTPS hostname and the Ubuntu archive signatures.
    if [[ ${GITHUB_ACTIONS:-} != true ]]; then
        echo "Could not resolve Ubuntu archive host $host" >&2
        return 1
    fi
    local address
    address="$(python3 - "$host" <<'PY'
import ipaddress
import json
import sys
from urllib.parse import urlencode
from urllib.request import Request, urlopen

host = sys.argv[1]
query = urlencode({"name": host, "type": "A"})
for resolver in ("https://dns.google/resolve", "https://cloudflare-dns.com/dns-query"):
    try:
        request = Request(f"{resolver}?{query}", headers={"Accept": "application/dns-json"})
        with urlopen(request, timeout=8) as response:
            answer = json.load(response)
        for record in answer.get("Answer", []):
            if record.get("type") == 1:
                address = ipaddress.IPv4Address(record["data"])
                if address.is_global:
                    print(address)
                    raise SystemExit(0)
    except (OSError, ValueError, KeyError):
        continue
raise SystemExit(f"Could not resolve Ubuntu archive host {host} through public DNS")
PY
)"
    if [[ ! -f "$scratch/original-hosts" ]]; then
        cp /etc/hosts "$scratch/original-hosts"
    fi
    printf '%s %s\n' "$address" "$host" | sudo tee -a /etc/hosts >/dev/null
}

resolve_archive_host "$archive"
if [[ "$security" != "$archive" ]]; then
    resolve_archive_host "$security"
fi

for suite in "$VERSION_CODENAME" "$VERSION_CODENAME-updates" "$VERSION_CODENAME-security"; do
    mirror="$archive"
    if [[ "$suite" == *-security ]]; then
        mirror="$security"
    fi
    printf 'deb [arch=%s signed-by=%s] %s %s main restricted universe multiverse\n' \
        "$arch" "$keyring" "$mirror" "$suite"
done > "$sources"

apt_options=(
    -o "Dir::Etc::sourcelist=$sources"
    -o Dir::Etc::sourceparts=-
    -o APT::Get::List-Cleanup=false
    -o Acquire::Retries=2
    -o Acquire::https::Timeout=20
)
sudo apt-get "${apt_options[@]}" -o APT::Update::Error-Mode=any update
sudo apt-get "${apt_options[@]}" install -y "$@"
