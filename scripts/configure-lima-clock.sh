#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Run inside a Lima Linux test VM. NTP owns the clock; the agent keeps its other services.
set -eu
mode=${1:---check}
case "$mode" in --check|--apply) ;; *) echo "usage: $0 [--check|--apply]" >&2; exit 2 ;; esac
[ "$#" -le 1 ] || exit 2
[ "$(uname -s)" = Linux ]
systemctl is-active --quiet lima-guestagent.service
systemctl is-active --quiet systemd-timesyncd.service
[ "$(timedatectl show --property=NTPSynchronized --value)" = yes ] || {
    echo 'An active, synchronized systemd-timesyncd is required before changing the Lima clock policy.' >&2
    exit 1
}

has_time_capability() {
    pid=$(systemctl show lima-guestagent.service --property=MainPID --value) || exit 2
    [ "$pid" -gt 0 ] || exit 2
    for field in CapEff CapPrm CapBnd; do
        value=$(sed -n "s/^$field:[[:space:]]*//p" "/proc/$pid/status")
        [ -n "$value" ] || exit 2
        if [ "$((0x$value & 0x2000000))" -ne 0 ]; then return 0; fi
    done
    return 1
}

if [ "$mode" = --apply ]; then
    [ "$(id -u)" -eq 0 ] || { echo '--apply requires root' >&2; exit 2; }
    dropin=/etc/systemd/system/lima-guestagent.service.d/90-trames-clock.conf
    temporary=$(mktemp)
    trap 'rm -f "$temporary"' EXIT HUP INT TERM
    cat > "$temporary" <<'EOF'
# NTP owns the CI VM clock. Lima 2.2.x SyncTime can otherwise step it backwards.
[Service]
CapabilityBoundingSet=~CAP_SYS_TIME
EOF
    changed=false
    if ! cmp -s "$temporary" "$dropin"; then
        install -d -m 0755 "$(dirname "$dropin")"
        install -m 0644 "$temporary" "$dropin"
        changed=true
    fi
    systemctl daemon-reload
    if [ "$changed" = true ] || has_time_capability; then
        systemctl restart lima-guestagent.service
    fi
fi

if has_time_capability; then
    echo 'Lima can still step the clock; run this script with sudo and --apply.' >&2
    exit 1
fi
systemctl is-active --quiet lima-guestagent.service
systemctl is-active --quiet systemd-timesyncd.service
echo 'Clock policy: synchronized NTP; active Lima agent without CAP_SYS_TIME.'
systemctl show lima-guestagent.service --property=MainPID --property=DropInPaths
timedatectl show --property=NTP --property=NTPSynchronized
