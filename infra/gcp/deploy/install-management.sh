#!/usr/bin/env bash
# install-management.sh — Install pact-journal (+ optionally lattice-server) on management nodes.
#
# Reusable on-prem. No GCP-specific logic.
#
# Usage: ./install-management.sh <node-id> <listen-addr> <raft-peers> [--with-lattice]
#   node-id:     Raft node ID (1, 2, or 3)
#   listen-addr: This node's IP or hostname
#   raft-peers:  Comma-separated peer list (e.g., "mgmt-1:9444,mgmt-2:9444,mgmt-3:9444")
#   --with-lattice: Also install lattice-server
#
# Expects binaries in /opt/pact/bin/ (placed by Packer or manual copy).

set -euo pipefail

NODE_ID="${1:?Usage: install-management.sh <node-id> <listen-addr> <raft-peers> [--with-lattice]}"
LISTEN_ADDR="${2:?}"
RAFT_PEERS="${3:?}"
WITH_LATTICE=false
[ "${4:-}" = "--with-lattice" ] && WITH_LATTICE=true

BIN_DIR="/opt/pact/bin"
CONF_DIR="/etc/pact"
DATA_DIR="/var/lib/pact/journal"
CERT_DIR="/etc/pact/certs"

echo "=== Installing pact-journal (node $NODE_ID) ==="

# Create directories
mkdir -p "$CONF_DIR" "$DATA_DIR" "$CERT_DIR"

# Create pact user if it doesn't exist
id -u pact &>/dev/null || useradd --system --no-create-home --shell /usr/sbin/nologin pact

# Generate certificates
"$(dirname "$0")/setup-ca.sh" "$CERT_DIR" "journal-$NODE_ID"

# Write journal config
cat > "$CONF_DIR/journal.toml" <<EOF
[journal]
node_id = $NODE_ID
listen = "0.0.0.0:9443"
data_dir = "$DATA_DIR"
snapshot_interval = 10000
max_concurrent_boot_streams = 15000

[journal.raft]
listen = "0.0.0.0:9444"
$(echo "$RAFT_PEERS" | awk -F',' '{for(i=1;i<=NF;i++) printf "peers = [\"%s\"]\n", $i}' | head -1)
# Full peer list:
$(IFS=',' read -ra PEERS <<< "$RAFT_PEERS"; printf 'peers = ['; for i in "${!PEERS[@]}"; do [ $i -gt 0 ] && printf ', '; printf '"%s"' "${PEERS[$i]}"; done; printf ']\n')

[journal.tls]
enabled = true
cert = "$CERT_DIR/node.crt"
key = "$CERT_DIR/node.key"
ca = "$CERT_DIR/ca.crt"

[telemetry]
log_format = "json"
log_level = "info"
prometheus_port = 9091
EOF

# Install systemd unit
cp /opt/pact/systemd/pact-journal.service /etc/systemd/system/
cat > /etc/pact/journal.env <<EOF
PACT_JOURNAL_NODE_ID=$NODE_ID
PACT_JOURNAL_LISTEN=0.0.0.0:9443
PACT_JOURNAL_DATA_DIR=$DATA_DIR
RUST_LOG=info
EOF

# Set ownership
chown -R pact:pact "$DATA_DIR" "$CERT_DIR"

# Enable and start
systemctl daemon-reload
systemctl enable pact-journal
systemctl start pact-journal

echo "pact-journal started (node $NODE_ID, listen $LISTEN_ADDR:9443)"

# Optionally install lattice-server
if [ "$WITH_LATTICE" = true ] && [ -f "$BIN_DIR/lattice-server" ]; then
    echo "=== Installing lattice-server ==="

    LATTICE_DATA="/var/lib/lattice/raft"
    LATTICE_CONF="/etc/lattice"
    mkdir -p "$LATTICE_DATA" "$LATTICE_CONF"

    # Generate lattice certs (reuse CA)
    "$(dirname "$0")/setup-ca.sh" "$LATTICE_CONF/certs" "lattice-$NODE_ID" "$CERT_DIR/../ca"

    cat > "$LATTICE_CONF/server.yaml" <<YAML
node_id: $NODE_ID
listen:
  grpc: "0.0.0.0:50051"
  rest: "0.0.0.0:8080"
  raft: "0.0.0.0:9000"
data_dir: "$LATTICE_DATA"
YAML

    if [ -f /opt/pact/systemd/lattice-server.service ]; then
        cp /opt/pact/systemd/lattice-server.service /etc/systemd/system/
        systemctl daemon-reload
        systemctl enable lattice-server
        systemctl start lattice-server
        echo "lattice-server started (node $NODE_ID)"
    fi
fi

echo "=== Management node $NODE_ID ready ==="
