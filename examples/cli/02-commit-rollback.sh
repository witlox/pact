#!/bin/bash
# 02-commit-rollback.sh — Commit drift and roll back changes.
#
# Demonstrates the commit-inspect-rollback cycle:
# 1. Commit current drift with a message
# 2. View the new entry in the log
# 3. Roll back to the previous state
#
# Prerequisites:
#   - pact journal running
#   - pact agent running
#   - PACT_VCLUSTER set or --vcluster provided
#
# Usage:
#   PACT_VCLUSTER=dev-sandbox ./02-commit-rollback.sh

set -euo pipefail

echo "=== Current Drift ==="
pact diff

echo ""
echo "=== Committing Drift ==="
pact commit -m "example: tuned hugepages for training workload"

echo ""
echo "=== Log After Commit ==="
pact log -n 5

echo ""
echo "=== Rolling Back ==="
# Get the sequence number of the entry before our commit.
# In practice, you would inspect the log and choose the right seq.
echo "To roll back, run:"
echo "  pact rollback <seq>"
echo ""
echo "Example:"
echo "  pact log -n 5          # Find the seq number you want"
echo "  pact rollback 42       # Roll back to that seq"
