#!/bin/bash
# 09-supercharged-commands.sh — Unified pact + lattice admin commands.
#
# These commands combine pact (config management) and lattice (workload
# scheduling) data into single views. They delegate to the appropriate
# backend transparently.
#
# Prerequisites:
#   - pact journal + agent running
#   - lattice scheduler reachable (for lattice-delegated commands)
#   - OpenCHAMI reachable (for BMC commands)
#
# Usage:
#   ./09-supercharged-commands.sh

set -euo pipefail

echo "=== Combined Cluster Status ==="
echo "  pact cluster"
echo "  Shows pact config state + lattice scheduling state in one view."

echo ""
echo "=== System Health ==="
echo "  pact health"
echo "  Checks journal quorum, agent connectivity, and lattice scheduler."

echo ""
echo "=== Job Management (delegates to lattice) ==="
echo "  pact jobs list --vcluster ml-training"
echo "  pact jobs inspect <job-id>"

echo ""
echo "=== Queue Status ==="
echo "  pact queue --vcluster ml-training"
echo "  Shows pending/running/completed allocations."

echo ""
echo "=== Audit Log (combined pact + lattice) ==="
echo "  pact audit --source all -n 50"
echo "  pact audit --source pact -n 20"
echo "  pact audit --source lattice -n 20"

echo ""
echo "=== Resource Accounting ==="
echo "  pact accounting --vcluster ml-training"

echo ""
echo "=== Node Lifecycle (delegated) ==="
echo "Drain workloads before maintenance:"
echo "  pact drain compute-042"
echo ""
echo "Remove from scheduling:"
echo "  pact cordon compute-042"
echo ""
echo "Return to scheduling:"
echo "  pact uncordon compute-042"
echo ""
echo "Reboot via BMC (delegates to OpenCHAMI):"
echo "  pact reboot compute-042"
echo ""
echo "Re-image (delegates to OpenCHAMI):"
echo "  pact reimage compute-042"
