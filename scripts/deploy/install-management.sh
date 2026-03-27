#!/usr/bin/env bash
# install-management.sh — Install pact-journal (+ optionally lattice-server) on management nodes.
#
# Reusable on-prem. No GCP-specific logic.
#
# Usage: ./install-management.sh <node-id> <listen-addr> <raft-peers> [--bootstrap] [--with-lattice]
#   node-id:     Raft node ID (1, 2, or 3)
#   listen-addr: This node's IP or hostname
#   raft-peers:  Peers in id=addr format (e.g., "1=mgmt-1:9443,2=mgmt-2:9443,3=mgmt-3:9443")
#   --bootstrap: Initialize Raft cluster (only on node 1, only on first start)
#   --with-lattice: Also install lattice-server
#
# Expects binaries in /opt/pact/bin/ (placed by Packer or manual copy).
# Expects CA at /etc/pact/ca/ (created by setup-ca.sh on first management node,
# then distributed to all nodes before running this script).

set -euo pipefail

NODE_ID="${1:?Usage: install-management.sh <node-id> <listen-addr> <raft-peers> [--bootstrap] [--with-lattice]}"
LISTEN_ADDR="${2:?}"
RAFT_PEERS="${3:?}"
shift 3

BOOTSTRAP=false
WITH_LATTICE=false
for arg in "$@"; do
    case "$arg" in
        --bootstrap) BOOTSTRAP=true ;;
        --with-lattice) WITH_LATTICE=true ;;
    esac
done

BIN_DIR="/opt/pact/bin"
CONF_DIR="/etc/pact"
DATA_DIR="/var/lib/pact/journal"
CERT_DIR="/etc/pact/certs"
CA_DIR="/etc/pact/ca"

echo "=== Installing pact-journal (node $NODE_ID) ==="

# Create directories
mkdir -p "$CONF_DIR" "$DATA_DIR" "$CERT_DIR" "$CA_DIR"

# Create pact user if it doesn't exist
id -u pact &>/dev/null || useradd --system --no-create-home --shell /usr/sbin/nologin pact

# Symlink binaries to /usr/local/bin (systemd units expect them there)
for bin in pact-journal pact pact-mcp; do
    if [ -f "$BIN_DIR/$bin" ]; then
        ln -sf "$BIN_DIR/$bin" "/usr/local/bin/$bin"
    fi
done

# Generate node certificate (CA must already exist at $CA_DIR)
# On the first management node, setup-ca.sh creates the CA.
# On subsequent nodes, CA must be distributed before running this script.
if [ -f "$CA_DIR/ca.key" ]; then
    "$(dirname "$0")/setup-ca.sh" "$CERT_DIR" "journal-$NODE_ID" "$CA_DIR"
else
    echo "WARNING: No CA found at $CA_DIR"
    echo "Run setup-ca.sh on the first management node, then copy $CA_DIR/ to all nodes."
fi

# Write journal env file (journal reads these as CLI args via systemd)
cat > "$CONF_DIR/journal.env" <<EOF
PACT_JOURNAL_NODE_ID=$NODE_ID
PACT_JOURNAL_LISTEN=0.0.0.0:9443
PACT_JOURNAL_DATA_DIR=$DATA_DIR
PACT_JOURNAL_PEERS=$RAFT_PEERS
RUST_LOG=info
EOF

# Install systemd unit
if [ -f /opt/pact/systemd/pact-journal.service ]; then
    cp /opt/pact/systemd/pact-journal.service /etc/systemd/system/
fi

# Set ownership
chown -R pact:pact "$DATA_DIR"
[ -d "$CERT_DIR" ] && chown -R pact:pact "$CERT_DIR"

# Bootstrap Raft cluster if requested (only on node 1, first start)
if [ "$BOOTSTRAP" = true ]; then
    echo "Bootstrapping Raft cluster..."
    # Run journal briefly with --bootstrap to initialize membership,
    # then stop so systemd takes over without --bootstrap.
    sudo -u pact "$BIN_DIR/pact-journal" \
        --node-id "$NODE_ID" \
        --listen "0.0.0.0:9443" \
        --data-dir "$DATA_DIR" \
        --peers "$RAFT_PEERS" \
        --bootstrap &
    JOURNAL_PID=$!
    sleep 3

    if kill -0 "$JOURNAL_PID" 2>/dev/null; then
        echo "Bootstrap succeeded — stopping bootstrap instance"
        kill "$JOURNAL_PID"
        wait "$JOURNAL_PID" 2>/dev/null || true
    else
        echo "WARNING: Bootstrap process exited early — check logs"
    fi
fi

# Enable and start via systemd
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

    ln -sf "$BIN_DIR/lattice-server" /usr/local/bin/lattice-server

    # Generate lattice certs (reuse same CA)
    "$(dirname "$0")/setup-ca.sh" "$LATTICE_CONF/certs" "lattice-$NODE_ID" "$CA_DIR"

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
