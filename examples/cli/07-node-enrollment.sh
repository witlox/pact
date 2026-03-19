#!/bin/bash
# 07-node-enrollment.sh — Register, assign, and manage compute nodes.
#
# Node lifecycle:
#   1. Register (admin pre-registers with hardware identity)
#   2. First boot (agent connects, CSR signed → Active)
#   3. Assign to vCluster (receives overlay config)
#   4. Decommission (certificate revoked)
#
# Prerequisites:
#   - pact-platform-admin role
#   - Journal quorum running
#
# Usage:
#   ./07-node-enrollment.sh

set -euo pipefail

echo "=== List Enrolled Nodes ==="
pact node list --vcluster ml-training

echo ""
echo "=== Inspect a Node ==="
echo "  pact node inspect compute-042"
echo "  Shows: hardware identity, vCluster, certificate serial/expiry, last seen"

echo ""
echo "=== Register a New Node ==="
echo "Pre-register before the node boots for the first time:"
echo '  pact node register \
    --node-id compute-099 \
    --mac aa:bb:cc:dd:ee:ff \
    --bmc-serial SN-2026-099'

echo ""
echo "=== Assign to vCluster ==="
echo "After registration, assign the node to receive its config overlay:"
echo "  pact node assign compute-099 --vcluster ml-training"

echo ""
echo "=== Move Between vClusters ==="
echo "  pact node move compute-099 --vcluster regulated-bio"
echo "  The node receives the new overlay on next config sync."

echo ""
echo "=== Decommission ==="
echo "Revokes the certificate and removes from scheduling:"
echo "  pact node decommission compute-099"
