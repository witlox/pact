#!/bin/bash
# 10-promote-and-blacklist.sh — Promote node deltas and manage drift blacklist.
#
# Workflow: tune a single node → validate → promote to vCluster overlay
# so all nodes in the vCluster get the same config.
#
# Prerequisites:
#   - pact journal + agent running
#   - pact-ops or pact-platform-admin role
#
# Usage:
#   ./10-promote-and-blacklist.sh

set -euo pipefail

echo "=== Promote Node Deltas to vCluster Overlay ==="
echo ""
echo "After tuning a node and committing changes, promote them:"
echo "  pact diff --committed compute-042    # Review committed deltas"
echo "  pact promote compute-042 --dry-run   # Preview the generated TOML"
echo "  pact promote compute-042             # Apply to vCluster overlay"
echo ""
echo "All other nodes in the vCluster will receive the changes on next sync."

echo ""
echo "=== Drift Detection Blacklist ==="
echo ""
echo "Manage which paths are ignored during drift detection:"
echo "  pact blacklist list                  # Show current patterns"
echo "  pact blacklist add '/scratch/**'     # Ignore scratch directory"
echo "  pact blacklist remove '/scratch/**'  # Stop ignoring"
echo ""
echo "Default blacklist: /tmp/**, /var/log/**, /proc/**, /sys/**, /dev/**, /run/user/**"
