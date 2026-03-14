#!/bin/bash
# 04-approval-workflow.sh — Two-person approval on regulated vClusters.
#
# On vClusters with two_person_approval = true, state-changing operations
# require a second admin to approve before they take effect.
#
# This script shows both sides of the workflow:
#   - Admin A: submits a change
#   - Admin B: reviews and approves/denies
#
# Prerequisites:
#   - pact journal running with policy enabled
#   - A regulated vCluster configured
#   - Two separate admin identities
#
# Usage:
#   # As Admin A (submitter):
#   PACT_VCLUSTER=sensitive-compute PACT_TOKEN=$ADMIN_A_TOKEN \
#       ./04-approval-workflow.sh submit
#
#   # As Admin B (approver):
#   PACT_TOKEN=$ADMIN_B_TOKEN \
#       ./04-approval-workflow.sh approve

set -euo pipefail

ACTION="${1:-submit}"

case "$ACTION" in
    submit)
        echo "=== Admin A: Submitting Change ==="
        echo "Committing on a regulated vCluster (requires approval)..."
        pact commit -m "add audit-forwarder service to sensitive-compute"
        echo ""
        echo "The commit is now pending approval."
        echo "Ask another admin to run: ./04-approval-workflow.sh approve"
        ;;

    approve)
        echo "=== Admin B: Reviewing Pending Approvals ==="
        pact approve list
        echo ""
        echo "To approve a pending request:"
        echo "  pact approve accept <approval-id>"
        echo ""
        echo "To deny a pending request:"
        echo "  pact approve deny <approval-id> -m \"reason for denial\""
        ;;

    deny)
        APPROVAL_ID="${2:?Usage: $0 deny <approval-id>}"
        echo "=== Admin B: Denying Request ==="
        pact approve deny "$APPROVAL_ID" -m "change window not scheduled"
        ;;

    *)
        echo "Usage: $0 {submit|approve|deny <id>}"
        exit 1
        ;;
esac
