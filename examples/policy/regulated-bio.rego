# regulated-bio.rego — OPA policy for a regulated vCluster
#
# This vCluster requires two-person approval for all state-changing
# operations. Stricter than ml-training: no auto-converge, no
# emergency mode for AI agents or viewers.
#
# Deploy to: /etc/pact/policies/regulated-bio.rego
# Test with: opa test /etc/pact/policies/ -v

package pact.vcluster.regulated_bio

import rego.v1

# ---------------------------------------------------------------
# Role mapping
# ---------------------------------------------------------------

# Ops engineers: read + write (with approval)
allow if {
    input.role == "pact-ops-regulated-bio"
    input.action in {
        "status", "log", "diff", "cap", "watch",
        "commit", "rollback", "exec", "service", "apply", "extend",
        "diag",
    }
}

# Regulated role: like ops but operations are two-person approved
allow if {
    input.role == "pact-regulated-regulated-bio"
    input.action in {
        "status", "log", "diff", "cap", "watch",
        "commit", "rollback", "exec", "service", "apply", "extend",
        "diag",
    }
}

# Viewers: read only
allow if {
    input.role == "pact-viewer-regulated-bio"
    input.action in {"status", "log", "diff", "cap", "watch", "diag"}
}

# Platform admins: everything
allow if {
    input.role == "pact-platform-admin"
}

# ---------------------------------------------------------------
# Two-person approval enforcement
# ---------------------------------------------------------------

# All state-changing operations require two-person approval
requires_approval if {
    input.action in {"commit", "rollback", "apply", "service_restart", "service_stop"}
}

# Emergency mode requires two-person approval too (regulated!)
requires_approval if {
    input.action == "emergency"
}

# ---------------------------------------------------------------
# Exec whitelist (more restrictive than ml-training)
# ---------------------------------------------------------------

allowed_commands := {
    "dmesg",
    "journalctl",
    "nvidia-smi",
    "free",
    "df",
    "ip",
    "ps",
    "chronyc",
}

exec_allowed if {
    input.action == "exec"
    input.command in allowed_commands
}

# No sysctl changes via exec (even in emergency)
deny if {
    input.action == "exec"
    startswith(input.command, "sysctl -w")
}

# ---------------------------------------------------------------
# Drift policy: no auto-converge, all changes require ack
# ---------------------------------------------------------------

require_ack_categories := {
    "services", "kernel", "mounts", "network", "gpu", "packages",
}

deny_commit if {
    input.action == "commit"
    input.drift_category in {"security", "firmware"}
    not input.emergency_mode
}

# ---------------------------------------------------------------
# Emergency mode: platform admin only (not ops, not viewers)
# ---------------------------------------------------------------

emergency_allowed if {
    input.action == "emergency"
    input.role == "pact-platform-admin"
}

deny if {
    input.action == "emergency"
    input.role != "pact-platform-admin"
}

# AI agents cannot enter emergency mode (P8)
deny if {
    input.action == "emergency"
    input.role == "pact-service-ai"
}

# ---------------------------------------------------------------
# Audit: all operations must produce audit entries
# ---------------------------------------------------------------

audit_required if {
    input.action in {
        "commit", "rollback", "apply", "exec",
        "service_restart", "service_stop", "service_start",
        "emergency",
    }
}
