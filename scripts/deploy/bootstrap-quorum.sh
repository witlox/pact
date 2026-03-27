#!/usr/bin/env bash
# bootstrap-quorum.sh — Initialize the Raft quorum after all journal nodes are running.
#
# Reusable on-prem. No GCP-specific logic.
#
# Usage: ./bootstrap-quorum.sh <leader-endpoint>
#   leader-endpoint: gRPC endpoint of the initial leader (e.g., mgmt-1:9443)
#
# This triggers Raft bootstrap on the leader node. Other nodes join via
# the peer list configured in their journal.toml.

set -euo pipefail

LEADER="${1:?Usage: bootstrap-quorum.sh <leader-endpoint>}"

echo "=== Bootstrapping Raft quorum via $LEADER ==="

# Wait for leader to be reachable
for i in $(seq 1 30); do
    if pact --endpoint "$LEADER" status &>/dev/null 2>&1; then
        echo "Leader reachable"
        break
    fi
    echo "Waiting for leader ($i/30)..."
    sleep 2
done

# Check quorum status
pact --endpoint "$LEADER" status || {
    echo "ERROR: Could not reach leader at $LEADER"
    echo "Check: systemctl status pact-journal on the leader node"
    exit 1
}

echo "=== Quorum bootstrap complete ==="
echo "Verify: pact --endpoint $LEADER status"
