#!/bin/sh
# pact-init-wrapper.sh — Minimal init wrapper for PID 1 nodes.
#
# Runs as kernel init (PID 1). Mounts essential filesystems,
# reads pact config from kernel command line parameters,
# patches agent config, then exec's pact-agent.
#
# Kernel params (set via GRUB or Terraform metadata):
#   pact.node_id=compute-1
#   pact.journal=host1:9443,host2:9443,host3:9443
#
# This script must be POSIX sh — no bash, no systemd, minimal deps.

# Mount essential filesystems
mount -t proc proc /proc 2>/dev/null
mount -t sysfs sysfs /sys 2>/dev/null
mount -t devtmpfs devtmpfs /dev 2>/dev/null
mount -t tmpfs tmpfs /run 2>/dev/null
mkdir -p /dev/pts /dev/shm
mount -t devpts devpts /dev/pts 2>/dev/null
mount -t tmpfs tmpfs /dev/shm 2>/dev/null
mount -t cgroup2 cgroup2 /sys/fs/cgroup 2>/dev/null

# Remount root read-write (kernel boots with ro)
mount -o remount,rw / 2>/dev/null

# Bring up loopback
ip link set lo up 2>/dev/null

# Pre-create resolv.conf for GCP internal DNS
# dhclient-script fails to write it without systemd, so we write it manually.
# 169.254.169.254 is GCP's metadata/DNS server.
# Remove dangling symlink first (Debian 12 may symlink to /run/systemd/resolve/)
rm -f /etc/resolv.conf 2>/dev/null
echo "nameserver 169.254.169.254" > /etc/resolv.conf

# Bring up primary interface via DHCP (needed for journal connectivity)
# dhclient -1 blocks until one lease is acquired (up to ~60s)
echo "pact-init: starting network setup" > /dev/console 2>/dev/null
mkdir -p /var/lib/dhcp /var/run 2>/dev/null
for iface in ens4 eth0; do
    if ip link show "$iface" >/dev/null 2>&1; then
        echo "pact-init: bringing up $iface" > /dev/console 2>/dev/null
        ip link set "$iface" up
        if command -v dhclient >/dev/null 2>&1; then
            echo "pact-init: running dhclient on $iface" > /dev/console 2>/dev/null
            dhclient -1 -v "$iface" 2>&1 | while read -r line; do echo "pact-init: dhclient: $line" > /dev/console 2>/dev/null; done
        else
            echo "pact-init: dhclient not found" > /dev/console 2>/dev/null
        fi
        break
    fi
done

# Show IP for debugging
ip addr show 2>/dev/null | grep "inet " > /dev/console 2>/dev/null

# Now try GCP metadata (network should be up after dhclient)
NODE_ID=""
JOURNAL_ENDPOINTS=""
if command -v curl >/dev/null 2>&1; then
    echo "pact-init: fetching metadata" > /dev/console 2>/dev/null
    NODE_ID=$(curl -sf -m 5 -H "Metadata-Flavor: Google" \
        "http://metadata.google.internal/computeMetadata/v1/instance/attributes/pact-node-id" 2>/dev/null || true)
    JOURNAL_ENDPOINTS=$(curl -sf -m 5 -H "Metadata-Flavor: Google" \
        "http://metadata.google.internal/computeMetadata/v1/instance/attributes/pact-journal-endpoints" 2>/dev/null || true)
    echo "pact-init: node_id=$NODE_ID journal=$JOURNAL_ENDPOINTS" > /dev/console 2>/dev/null
fi

# Fall back to kernel command line params if metadata fetch didn't work
CONFIG="/etc/pact/agent.toml"
CMDLINE=$(cat /proc/cmdline 2>/dev/null)

if [ -z "$NODE_ID" ]; then
    case "$CMDLINE" in
        *pact.node_id=*)
            NODE_ID=$(echo "$CMDLINE" | sed 's/.*pact.node_id=\([^ ]*\).*/\1/')
            ;;
    esac
fi

JOURNAL=""
if [ -z "$JOURNAL_ENDPOINTS" ]; then
    case "$CMDLINE" in
        *pact.journal=*)
            JOURNAL=$(echo "$CMDLINE" | sed 's/.*pact.journal=\([^ ]*\).*/\1/')
            ;;
    esac
fi

# Patch agent config
if [ -n "$NODE_ID" ] && [ -f "$CONFIG" ]; then
    sed -i "s|node_id = \"unset\"|node_id = \"$NODE_ID\"|" "$CONFIG"
fi

if [ -n "$JOURNAL_ENDPOINTS" ] && [ -f "$CONFIG" ]; then
    # Metadata has pre-formatted TOML values: "host1:9443","host2:9443"
    sed -i "s|endpoints = \[\"journal-1:9443\"\]|endpoints = [$JOURNAL_ENDPOINTS]|" "$CONFIG"
elif [ -n "$JOURNAL" ] && [ -f "$CONFIG" ]; then
    # Kernel param: comma-separated, convert to TOML array
    TOML_ENDPOINTS=$(echo "$JOURNAL" | sed 's/,/", "/g')
    sed -i "s|endpoints = \[\"journal-1:9443\"\]|endpoints = [\"$TOML_ENDPOINTS\"]|" "$CONFIG"
fi

# Exec pact-agent as PID 1 (replaces this script)
exec /usr/local/bin/pact-agent --config "$CONFIG"
