#!/bin/bash
# 08-service-management.sh — Manage supervised services on nodes.
#
# pact-agent supervises services directly (PactSupervisor) or via
# systemd (fallback). All lifecycle operations are authenticated,
# authorized, and logged to the immutable journal.
#
# Prerequisites:
#   - pact agent running on target node
#   - pact-ops or pact-platform-admin role
#
# Usage:
#   ./08-service-management.sh [node-id]

set -euo pipefail

NODE="${1:-dev-node-001}"

echo "=== Service Status ==="
pact service status "$NODE"

echo ""
echo "=== Restart a Service ==="
echo "  pact service restart $NODE nvidia-persistenced"
echo "  Records a ServiceLifecycle audit entry in the journal."

echo ""
echo "=== Stop a Service ==="
echo "  pact service stop $NODE metrics-exporter"
echo "  The supervisor's restart policy determines if it auto-restarts."

echo ""
echo "=== Start a Service ==="
echo "  pact service start $NODE metrics-exporter"
