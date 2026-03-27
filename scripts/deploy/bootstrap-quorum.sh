#!/usr/bin/env bash
# bootstrap-quorum.sh — Initialize and verify the Raft quorum.
#
# Reusable on-prem. No GCP-specific logic.
#
# Deployment order:
#   1. Run setup-ca.sh on first management node to create CA
#   2. Distribute /etc/pact/ca/ to ALL management + compute nodes
#   3. Run install-management.sh on node 1 with --bootstrap
#   4. Run install-management.sh on nodes 2, 3 (without --bootstrap)
#   5. Wait ~10 seconds for membership replication
#   6. Run this script to verify quorum health
#
# Usage: ./bootstrap-quorum.sh <leader-endpoint>
#   leader-endpoint: gRPC endpoint of the initial leader (e.g., mgmt-1:9443)

set -euo pipefail

LEADER="${1:?Usage: bootstrap-quorum.sh <leader-endpoint>}"

echo "=== Verifying Raft quorum via $LEADER ==="

# Wait for leader to be reachable
for i in $(seq 1 30); do
    if pact --endpoint "$LEADER" status &>/dev/null 2>&1; then
        echo "Leader reachable"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "ERROR: Could not reach leader at $LEADER after 60 seconds"
        echo "Check: systemctl status pact-journal on the leader node"
        echo "Check: journalctl -u pact-journal for errors"
        exit 1
    fi
    echo "Waiting for leader ($i/30)..."
    sleep 2
done

# Verify quorum status
echo ""
echo "Quorum status:"
pact --endpoint "$LEADER" status || {
    echo "ERROR: pact status failed"
    exit 1
}

echo ""
echo "=== Quorum verification complete ==="
echo "Next: deploy pact-agent to compute nodes using install-compute.sh"
