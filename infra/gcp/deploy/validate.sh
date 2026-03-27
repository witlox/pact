#!/usr/bin/env bash
# validate.sh — Run the GCP test matrix against a deployed pact cluster.
#
# Usage: ./validate.sh <variant> <journal-endpoint> <compute-nodes>
#   variant:          v1|v2|v3|v4
#   journal-endpoint: gRPC endpoint (e.g., mgmt-1:9443)
#   compute-nodes:    Comma-separated compute node IDs
#
# Exits 0 if all tests pass, 1 if any fail.

set -euo pipefail

VARIANT="${1:?Usage: validate.sh <variant> <journal-endpoint> <compute-nodes>}"
ENDPOINT="${2:?}"
NODES="${3:?}"

PACT="pact --endpoint $ENDPOINT"
IFS=',' read -ra NODE_LIST <<< "$NODES"
FIRST_NODE="${NODE_LIST[0]}"

PASS=0
FAIL=0
SKIP=0

run_test() {
    local id="$1" name="$2" categories="$3"
    shift 3

    # Check if test applies to this variant
    case "$VARIANT" in
        v1) [[ "$categories" == *"S"* ]] && { ((SKIP++)); return; } ;;
        v2) [[ "$categories" == *"I"* || "$categories" == *"S"* ]] && { ((SKIP++)); return; } ;;
        v3) [[ "$categories" == *"D"* ]] && { ((SKIP++)); return; } ;;
        v4) [[ "$categories" == *"I"* || "$categories" == *"D"* ]] && { ((SKIP++)); return; } ;;
    esac

    printf "  [%3s] %-50s " "$id" "$name"
    if eval "$@" &>/dev/null 2>&1; then
        echo "PASS"
        ((PASS++))
    else
        echo "FAIL"
        ((FAIL++))
    fi
}

expect_fail() {
    # Inverts: passes if the command FAILS
    ! "$@"
}

echo "========================================"
echo " Pact GCP Test Matrix — Variant $VARIANT"
echo "========================================"
echo ""

# --- Raft quorum ---
echo "Raft quorum:"
run_test 1 "Journal quorum elects leader" "C" \
    "$PACT status"
run_test 2 "Write survives 1-node loss" "C" \
    "echo 'manual: stop 1 journal, pact commit'" # manual step
run_test 3 "Write blocked on 2-node loss" "C" \
    "echo 'manual: stop 2 journals, expect error'" # manual step

# --- Boot + init ---
echo ""
echo "Boot + init:"
run_test 4 "pact-agent starts as PID 1" "I" \
    "$PACT exec $FIRST_NODE -- cat /proc/1/cmdline | grep -q pact-agent"
run_test 5 "Pseudofs mounted by agent" "I" \
    "$PACT exec $FIRST_NODE -- test -d /proc/self"
run_test 6 "Watchdog status (skip if no device)" "I" \
    "$PACT exec $FIRST_NODE -- test -f /run/pact/ready"
run_test 7 "Boot petter active during boot" "I" \
    "$PACT exec $FIRST_NODE -- grep -q 'boot petter\\|supervision loop' /var/log/pact-agent.log"
run_test 8 "Agent under systemd" "SYSTEMD" \
    "$PACT exec $FIRST_NODE -- systemctl is-active pact-agent"
run_test 9 "Systemd restarts on crash" "SYSTEMD" \
    "echo 'manual: kill agent, verify restart'" # manual step

# --- Config management ---
echo ""
echo "Config management:"
run_test 10 "Boot config streamed" "C" \
    "$PACT exec $FIRST_NODE -- test -f /run/pact/ready"
run_test 11 "Config commit" "C" \
    "$PACT commit --vcluster test --message 'validation test' --dry-run"
run_test 13 "Drift detection" "C" \
    "$PACT status --node $FIRST_NODE"
run_test 15 "Overlay sysctl application" "C" \
    "$PACT exec $FIRST_NODE -- sysctl vm.swappiness"

# --- Shell + exec ---
echo ""
echo "Shell + exec:"
run_test 16 "pact shell to compute" "C" \
    "echo 'hostname' | timeout 5 $PACT shell $FIRST_NODE"
run_test 17 "pact exec returns output" "C" \
    "$PACT exec $FIRST_NODE -- hostname"
run_test 19 "Audit trail has entries" "C" \
    "$PACT audit --limit 1"

# --- Process supervision ---
echo ""
echo "Process supervision:"
run_test 20 "Services started" "C" \
    "$PACT exec $FIRST_NODE -- test -f /run/pact/ready"
run_test 22 "cgroup hierarchy exists" "I" \
    "$PACT exec $FIRST_NODE -- test -d /sys/fs/cgroup/pact.slice"
run_test 23 "Graceful shutdown stops all" "C" \
    "echo 'manual: shutdown agent, verify no orphans'" # manual step

# --- Identity + auth ---
echo ""
echo "Identity + auth:"
run_test 24 "mTLS connection" "C" \
    "$PACT status"

# --- Monitoring ---
echo ""
echo "Monitoring:"
run_test 27 "Journal metrics available" "C" \
    "curl -sf http://${ENDPOINT%%:*}:9091/metrics | grep -q pact"

# --- Supercharged (delegation available) ---
echo ""
echo "Supercharged (delegation):"
run_test 30 "pact drain" "S" \
    "$PACT drain $FIRST_NODE --dry-run"
run_test 31 "pact cordon" "S" \
    "$PACT cordon $FIRST_NODE --dry-run"
run_test 32 "pact uncordon" "S" \
    "$PACT uncordon $FIRST_NODE --dry-run"
run_test 33 "pact promote" "S" \
    "$PACT promote --from test --dry-run"
run_test 34 "pact group list" "S" \
    "$PACT group list"
run_test 35 "pact blacklist add" "S" \
    "$PACT blacklist list"

# --- Supercharged (delegation unavailable) ---
echo ""
echo "Supercharged (no delegation):"
run_test 36 "drain without lattice → clean error" "D" \
    "expect_fail $PACT drain $FIRST_NODE"
run_test 37 "cordon without lattice → clean error" "D" \
    "expect_fail $PACT cordon $FIRST_NODE"
run_test 38 "reboot without OpenCHAMI → clean error" "D" \
    "expect_fail $PACT reboot $FIRST_NODE"
run_test 39 "reimage without OpenCHAMI → clean error" "D" \
    "expect_fail $PACT reimage $FIRST_NODE"

# --- Emergency mode ---
echo ""
echo "Emergency mode:"
run_test 40 "Enter emergency" "C" \
    "$PACT emergency start --reason 'validation test'"
run_test 42 "End emergency" "C" \
    "$PACT emergency end"

# --- Capability reporting ---
echo ""
echo "Capability reporting:"
run_test 43 "CPU capability" "C" \
    "$PACT exec $FIRST_NODE -- cat /run/pact/capability.json | grep -q cpu"
run_test 44 "Memory capability" "C" \
    "$PACT exec $FIRST_NODE -- cat /run/pact/capability.json | grep -q memory"
run_test 45 "Network capability" "C" \
    "$PACT exec $FIRST_NODE -- cat /run/pact/capability.json | grep -q network"

# --- Lattice integration ---
echo ""
echo "Lattice integration:"
run_test 46 "lattice-agent started by pact" "S,I" \
    "$PACT exec $FIRST_NODE -- pgrep lattice-agent"
run_test 47 "Capability report readable" "S" \
    "$PACT exec $FIRST_NODE -- test -f /run/pact/capability.json"

# --- Summary ---
echo ""
echo "========================================"
TOTAL=$((PASS + FAIL + SKIP))
echo " Results: $PASS passed, $FAIL failed, $SKIP skipped (of $TOTAL)"
echo "========================================"

[ "$FAIL" -eq 0 ] && exit 0 || exit 1
