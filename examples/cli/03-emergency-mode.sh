#!/bin/bash
# 03-emergency-mode.sh — Enter and exit emergency mode.
#
# Emergency mode relaxes policy constraints while maintaining
# the full audit trail. Use for genuine emergencies only.
#
# What changes in emergency mode:
#   - Commit window extended to 4 hours
#   - Shell whitelist restrictions relaxed
#   - All actions still logged to immutable journal
#
# Prerequisites:
#   - pact journal running
#   - pact-platform-admin or pact-ops role
#
# Usage:
#   ./03-emergency-mode.sh

set -euo pipefail

NODE="${1:-dev-node-001}"

echo "=== Entering Emergency Mode ==="
pact emergency start -r "GPU ECC errors on $NODE, need unrestricted diagnostics"

echo ""
echo "=== Current Status (should show emergency mode) ==="
pact status "$NODE"

echo ""
echo "=== Run diagnostics (unrestricted in emergency mode) ==="
echo "  pact exec $NODE -- nvidia-smi -q -d ECC"
echo "  pact exec $NODE -- dmesg -T | grep -i error"
echo "  pact shell $NODE"

echo ""
echo "=== Exiting Emergency Mode ==="
pact emergency end

echo ""
echo "=== Verify: Check log for emergency entries ==="
pact log -n 5
