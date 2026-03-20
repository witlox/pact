#!/bin/bash
# 06-diagnostics.sh — Retrieve diagnostic logs from nodes.
#
# pact diag replaces SSH-based log collection with authenticated,
# audited, server-side filtered log retrieval.
#
# Features:
#   - Per-node or fleet-wide (--vcluster) queries
#   - Source filtering: system (dmesg + syslog), service, or all
#   - Server-side grep to reduce bandwidth
#   - Concurrent fan-out for fleet-wide queries
#
# Prerequisites:
#   - pact journal + agent running
#   - pact-ops or pact-platform-admin role
#
# Usage:
#   ./06-diagnostics.sh [node-id]

set -euo pipefail

NODE="${1:-dev-node-001}"

echo "=== System Logs (last 50 lines) ==="
pact diag "$NODE" --source system --lines 50

echo ""
echo "=== Service Logs Only ==="
pact diag "$NODE" --source service

echo ""
echo "=== GPU Errors (server-side grep) ==="
pact diag "$NODE" --grep "ECC error|Xid|NVRM"

echo ""
echo "=== Specific Service ==="
pact diag "$NODE" --service nvidia-persistenced --lines 200

echo ""
echo "=== Fleet-Wide: All Nodes in vCluster ==="
echo "  pact diag --vcluster ml-training --grep 'error' --lines 20"
echo ""
echo "Output is prefixed with [node-id] per line for easy parsing."
