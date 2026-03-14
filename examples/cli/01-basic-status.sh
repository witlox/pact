#!/bin/bash
# 01-basic-status.sh — Check node status, view logs, and show drift.
#
# Prerequisites:
#   - pact journal running (just run-journal)
#   - pact agent running (just run-agent)
#
# Usage:
#   ./01-basic-status.sh [node-id]

set -euo pipefail

NODE="${1:-dev-node-001}"

echo "=== Node Status ==="
pact status "$NODE"

echo ""
echo "=== Recent Configuration Log (last 10 entries) ==="
pact log -n 10

echo ""
echo "=== Drift on $NODE ==="
pact diff "$NODE"

echo ""
echo "=== Hardware Capabilities ==="
pact cap "$NODE"
