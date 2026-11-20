#!/bin/sh
# Only inside the disposable Alpine CI container, never on a user's server.
set -eu
[ "${PEBREL_DISPOSABLE_OPENRC_TEST:-}" = 1 ] || exit 1
[ -f /.dockerenv ] || exit 1
[ ! -e /opt/pebrel-relay ] && [ ! -e /etc/pebrel-relay ] || exit 1
apk add --no-cache openrc
# OpenRC is not PID 1 in this isolated container. Initialize its runtime state;
# rc-service and supervise-daemon below are the real Alpine implementations.
mkdir -p /run/openrc
touch /run/openrc/softlevel
umask 077
binary=/fixture/pebrel-relay
sha=$(sha256sum "$binary")
sha=${sha%% *}
manual_dir=$(mktemp -d /tmp/pebrel-manual-test.XXXXXXXX)
arch=$(uname -m)
mkdir "$manual_dir/$arch"
cp "$binary" "$manual_dir/$arch/pebrel-relay"
printf '%s  pebrel-relay\n' "$sha" > "$manual_dir/$arch/SHA256SUMS"
cp /fixture/install.sh "$manual_dir/install.sh"
install_relay() {
    sh "$manual_dir/install.sh" 127.0.0.1 18443
}
cleanup() {
    if [ -f /opt/pebrel-relay/installation.json ]; then
        "$binary" service-uninstall --purge
    fi
}
trap cleanup EXIT
install_relay
"$binary" service-status | grep '"ready":true'
# The listener must have permanently dropped root, not just its supervising PID.
pid=$(pidof pebrel-relay)
test -n "$pid"
awk '/^Uid:/ { if ($2 == 0 || $3 == 0 || $4 == 0 || $5 == 0) exit 1; found=1 } END { if (!found) exit 1 }' "/proc/$pid/status"
before=$(sha256sum /etc/pebrel-relay/access.json)
"$binary" service-stop
"$binary" service-status | grep '"running":false'
"$binary" service-start
"$binary" service-status | grep '"ready":true'
"$binary" service-uninstall
"$binary" service-status | grep '"configuration_retained":true'
test ! -e /etc/init.d/pebrel-relay
install_relay
after=$(sha256sum /etc/pebrel-relay/access.json)
test "$before" = "$after"
"$binary" service-uninstall --purge
"$binary" service-status | grep '"configuration_retained":false'
printf 'OpenRC: install, unprivileged listener, readiness, stop/start, retained reinstall and purge passed\n'
