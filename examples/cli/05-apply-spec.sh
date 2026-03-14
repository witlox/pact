#!/bin/bash
# 05-apply-spec.sh — Apply a declarative TOML spec.
#
# pact supports applying configuration as declarative TOML specs.
# This is the preferred way to make repeatable, reviewable changes
# to vCluster overlays.
#
# Workflow:
# 1. Write or edit a TOML spec file
# 2. Review the spec
# 3. Apply it
# 4. Verify the changes took effect
#
# Prerequisites:
#   - pact journal running
#   - PACT_VCLUSTER set
#
# Usage:
#   ./05-apply-spec.sh [spec-file]

set -euo pipefail

SPEC="${1:-}"

if [ -z "$SPEC" ]; then
    # Create an example spec in a temp file
    SPEC=$(mktemp /tmp/pact-spec-XXXXXX.toml)
    cat > "$SPEC" << 'EOF'
# Example: add hugepages and an NFS mount to ml-training vCluster

[vcluster.ml-training.sysctl]
"vm.nr_hugepages" = "1024"
"net.core.rmem_max" = "16777216"

[vcluster.ml-training.mounts]
"/datasets" = { type = "nfs", source = "storage02:/shared-datasets", read_only = true }
EOF
    echo "Created example spec at $SPEC"
    echo ""
fi

echo "=== Spec Contents ==="
cat "$SPEC"

echo ""
echo "=== Applying Spec ==="
pact apply "$SPEC"

echo ""
echo "=== Verify: Check Log ==="
pact log -n 3

echo ""
echo "=== Verify: Check Diff ==="
pact diff
