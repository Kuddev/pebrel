#!/bin/sh
# Real lifecycle acceptance only on a disposable CI host/container.
set -eu
[ "${PEBREL_DISPOSABLE_INSTALL_TEST:-}" = 1 ] || exit 1
[ "$(id -u)" = 0 ] || exit 1
[ ! -e /opt/pebrel-relay ] && [ ! -e /etc/pebrel-relay ] || exit 1
installer=$1
binary=$2
if [ "${3:-}" = 239 ]; then
    [ "$(systemctl --version | awk 'NR == 1 { print $2 }')" = 239 ]
    [ -L /etc/init.d ]
fi
fixture=$(mktemp -d /tmp/pebrel-online-test.XXXXXXXX)
decoy_pid=
cleanup() {
    if [ -f /opt/pebrel-relay/installation.json ]; then "$binary" service-uninstall --purge; fi
    if [ -n "$decoy_pid" ]; then kill "$decoy_pid" 2>/dev/null || :; wait "$decoy_pid" 2>/dev/null || :; fi
    rm -f "$fixture/relay.json" "$fixture/access.json" "$fixture/certificate.pem" \
        "$fixture/private-key.pem" "$fixture/corrupt" "$fixture/executed" "$fixture/download.log" "$fixture/corrupt.log"
    rmdir "$fixture"
}
trap cleanup EXIT
export PEBREL_FIXTURE_BINARY="$binary"
export PEBREL_FIXTURE_DIR="$fixture"

# An unrelated 443 listener must stay alive while Pebrel selects 8443.
"$binary" init --directory "$fixture" --address 127.0.0.1 --listen 0.0.0.0:443
"$binary" serve-unprivileged --config "$fixture/relay.json" &
decoy_pid=$!
attempt=0
until "$binary" probe --config "$fixture/relay.json"; do
    attempt=$((attempt + 1)); [ "$attempt" -lt 20 ]; sleep 0.2
done

# Full script execution with only the HTTPS boundary substituted. Truncated or
# poisoned bytes must never execute, even if the downloader reports success.
printf '#!/bin/sh\ntouch "%s/executed"\n' "$fixture" > "$fixture/corrupt"
if sh -c '
    installer=$1; shift
    curl() { for destination; do :; done; cp "$PEBREL_FIXTURE_DIR/corrupt" "$destination"; }
    . "$installer"
' fixture "$installer" --address 127.0.0.1 > "$fixture/corrupt.log" 2>&1; then
    printf 'Corrupt download was accepted\n' >&2; exit 1
fi
grep -q 'SHA256 mismatch' "$fixture/corrupt.log"
[ ! -e "$fixture/executed" ] && [ ! -e /opt/pebrel-relay ]

# Force curl failure and verify that wget's bytes are checked and installed.
sh -c '
    installer=$1; shift
    curl() { printf "curl\n" >> "$PEBREL_FIXTURE_DIR/download.log"; return 22; }
    wget() {
        printf "wget\n" >> "$PEBREL_FIXTURE_DIR/download.log"
        for destination; do :; done
        cp "$PEBREL_FIXTURE_BINARY" "$destination"
    }
    . "$installer"
' fixture "$installer" --address 127.0.0.1
[ "$(wc -l < "$fixture/download.log")" -eq 2 ]
grep -q '0.0.0.0:8443' /etc/pebrel-relay/relay.json
"$binary" probe --config "$fixture/relay.json"
pid=$(systemctl show -p MainPID --value pebrel-relay.service)
awk '/^Uid:/ { if ($2 == 0 || $3 == 0 || $4 == 0 || $5 == 0) exit 1; found=1 }
    END { if (!found) exit 1 }' "/proc/$pid/status"
if [ "${3:-}" = 239 ]; then
    grep -q 'serve-unprivileged' /etc/systemd/system/pebrel-relay.service
    grep -q '^CapEff:[[:space:]]*0000000000000000$' "/proc/$pid/status"
fi
before=$(sha256sum /etc/pebrel-relay/access.json)
sh "$installer" status # Verified installed binary works without network.
sh "$installer" stop --binary "$binary"
sh "$installer" start --binary "$binary"
sh "$installer" uninstall --binary "$binary"
sh "$installer" install --binary "$binary"
[ "$before" = "$(sha256sum /etc/pebrel-relay/access.json)" ]
grep -q '0.0.0.0:8443' /etc/pebrel-relay/relay.json
sh "$installer" purge --binary "$binary"
[ ! -e /etc/pebrel-relay/access.json ]
# Simulate unreachable GitHub. A proxy is only a transport for already pinned
# bytes; it never supplies a new expected checksum or an executable script.
sh -c '
    installer=$1; shift
    curl() {
        for arg; do case "$arg" in https://*) url=$arg;; esac; destination=$arg; done
        case "$url" in
            https://gh-proxy.com/*) cp "$PEBREL_FIXTURE_BINARY" "$destination";;
            *) return 22;;
        esac
    }
    wget() { return 4; }
    . "$installer"
' fixture "$installer" --address 127.0.0.1
sh "$installer" purge --binary "$binary"
"$binary" probe --config "$fixture/relay.json"
printf 'Online script: corrupt download rejection, curl/wget and proxy fallback, 443 preservation, 8443 selection, non-root listener, retained reinstall and purge passed\n'
