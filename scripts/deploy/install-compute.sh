#!/usr/bin/env bash
# install-compute.sh — Install pact-agent on compute nodes.
#
# For systemd variant: installs as a systemd service.
# For PID 1 variant: this runs during Packer image build, not at runtime
# (agent starts as init, no installer needed at boot).
#
# Reusable on-prem. No GCP-specific logic.
#
# Usage: ./install-compute.sh <node-id> <vcluster> <journal-endpoints> [--with-lattice-agent]
#   node-id:           Unique node identifier
#   vcluster:          vCluster assignment
#   journal-endpoints: Comma-separated journal gRPC endpoints (e.g., "mgmt-1:9443,mgmt-2:9443")
#   --with-lattice-agent: Also install lattice-agent as supervised service
#
# Expects binaries in /opt/pact/bin/ (placed by Packer or manual copy).
# Expects CA at /etc/pact/ca/ (distributed from first management node).

set -euo pipefail

NODE_ID="${1:?Usage: install-compute.sh <node-id> <vcluster> <journal-endpoints> [--with-lattice-agent]}"
VCLUSTER="${2:?}"
JOURNAL_ENDPOINTS="${3:?}"
WITH_LATTICE_AGENT=false
[ "${4:-}" = "--with-lattice-agent" ] && WITH_LATTICE_AGENT=true

BIN_DIR="/opt/pact/bin"
CONF_DIR="/etc/pact"
CERT_DIR="/etc/pact/certs"
CA_DIR="/etc/pact/ca"
RUN_DIR="/run/pact"

echo "=== Installing pact-agent ($NODE_ID, vcluster=$VCLUSTER) ==="

mkdir -p "$CONF_DIR" "$CERT_DIR" "$RUN_DIR"

# Create pact user if needed (systemd mode only — PID 1 runs as root)
id -u pact &>/dev/null || useradd --system --no-create-home --shell /usr/sbin/nologin pact

# Symlink agent binary to /usr/local/bin
if [ -f "$BIN_DIR/pact-agent" ]; then
    ln -sf "$BIN_DIR/pact-agent" /usr/local/bin/pact-agent
fi

# Generate node certificate (CA must be distributed from management node)
if [ -f "$CA_DIR/ca.key" ]; then
    "$(dirname "$0")/setup-ca.sh" "$CERT_DIR" "$NODE_ID" "$CA_DIR"
elif [ -f "$CA_DIR/ca.crt" ]; then
    # CA cert present but no key — can't generate certs locally.
    # In production, the agent would use enrollment (ADR-008).
    echo "WARNING: CA cert present but no key — skipping cert generation"
    echo "Agent will attempt enrollment via journal on first connect"
else
    echo "WARNING: No CA found at $CA_DIR — skipping cert generation"
    echo "Copy CA from management node or run setup-ca.sh manually"
fi

# Build journal endpoint list for TOML
IFS=',' read -ra ENDPOINTS <<< "$JOURNAL_ENDPOINTS"
ENDPOINT_TOML=""
for ep in "${ENDPOINTS[@]}"; do
    ENDPOINT_TOML="${ENDPOINT_TOML}  \"${ep}\","
done
ENDPOINT_TOML="[${ENDPOINT_TOML%,}]"

# Determine TLS settings
TLS_ENABLED=false
TLS_SECTION=""
if [ -f "$CERT_DIR/node.crt" ] && [ -f "$CERT_DIR/node.key" ] && [ -f "$CERT_DIR/ca.crt" ]; then
    TLS_ENABLED=true
    TLS_SECTION="tls_cert = \"$CERT_DIR/node.crt\"
tls_key = \"$CERT_DIR/node.key\"
tls_ca = \"$CERT_DIR/ca.crt\""
fi

# Write agent config
cat > "$CONF_DIR/agent.toml" <<EOF
[agent]
node_id = "$NODE_ID"
vcluster = "$VCLUSTER"
enforcement_mode = "observe"

[agent.supervisor]
backend = "pact"

[agent.journal]
endpoints = $ENDPOINT_TOML
tls_enabled = $TLS_ENABLED
$TLS_SECTION

[agent.observer]
ebpf = false
inotify = true
netlink = true

[agent.shell]
listen = "0.0.0.0:9445"
whitelist_mode = "strict"

[agent.commit_window]
base_window_seconds = 900
drift_sensitivity = 2.0
emergency_window_seconds = 14400

[agent.blacklist]
patterns = ["/tmp/**", "/proc/**", "/sys/**", "/dev/**", "/run/user/**", "/run/pact/**"]
EOF

# Install systemd unit (for systemd variant — PID 1 doesn't use this)
if [ -f /opt/pact/systemd/pact-agent.service ]; then
    cp /opt/pact/systemd/pact-agent.service /etc/systemd/system/
    cat > /etc/pact/agent.env <<EOF
PACT_AGENT_CONFIG=/etc/pact/agent.toml
RUST_LOG=info
EOF
    systemctl daemon-reload
    systemctl enable pact-agent
    systemctl start pact-agent
    echo "pact-agent started under systemd"
fi

# Optionally install lattice-agent
if [ "$WITH_LATTICE_AGENT" = true ] && [ -f "$BIN_DIR/lattice-agent" ]; then
    echo "=== Installing lattice-agent ==="
    ln -sf "$BIN_DIR/lattice-agent" /usr/local/bin/lattice-agent
    # In PID 1 mode, lattice-agent is a declared service in agent.toml.
    # In systemd mode, it runs as its own systemd unit.
    if [ -f /opt/pact/systemd/lattice-agent.service ]; then
        cp /opt/pact/systemd/lattice-agent.service /etc/systemd/system/
        systemctl daemon-reload
        systemctl enable lattice-agent
        systemctl start lattice-agent
        echo "lattice-agent started under systemd"
    fi
fi

echo "=== Compute node $NODE_ID ready ==="
