# ml-training.rego — OPA policy for an ML training vCluster
#
# This policy controls what operations are allowed on the "ml-training"
# vCluster. Load it into OPA alongside the base pact policies.
#
# Deploy to: /etc/pact/policies/ml-training.rego
# Test with: opa test /etc/pact/policies/ -v

package pact.vcluster.ml_training

import rego.v1

# ---------------------------------------------------------------
# Role mapping: who can do what on ml-training
# ---------------------------------------------------------------

# Ops engineers can read state, commit, rollback, exec, and manage services
allow if {
    input.role == "pact-ops-ml-training"
    input.action in {"status", "log", "diff", "cap", "watch", "commit", "rollback", "exec", "service", "apply", "extend"}
}

# Viewers can only read
allow if {
    input.role == "pact-viewer-ml-training"
    input.action in {"status", "log", "diff", "cap", "watch"}
}

# Platform admins can do everything
allow if {
    input.role == "pact-platform-admin"
}

# AI agents (MCP) can read but not write
allow if {
    input.role == "pact-service-ai"
    input.action in {"status", "log", "diff", "cap", "watch", "service_status", "query_fleet"}
}

# ---------------------------------------------------------------
# Command whitelist for exec/shell
# ---------------------------------------------------------------

# Commands allowed for ops engineers on ML training nodes
allowed_commands := {
    "nvidia-smi",
    "rocm-smi",
    "cat",
    "grep",
    "dmesg",
    "top",
    "htop",
    "free",
    "df",
    "mount",
    "lspci",
    "lsmod",
    "ip",
    "ss",
    "ps",
    "systemctl",
    "journalctl",
    "chronyc",
    "uenv",
}

exec_allowed if {
    input.action == "exec"
    input.command in allowed_commands
}

# ---------------------------------------------------------------
# Drift commit policy
# ---------------------------------------------------------------

# Auto-converge services and kernel params (no manual commit needed)
auto_converge_categories := {"services", "kernel"}

# Require manual acknowledgment for mounts, network, GPU changes
require_ack_categories := {"mounts", "network", "gpu"}

# Reject drift in these categories (must rollback, not commit)
deny_commit if {
    input.action == "commit"
    input.drift_category in {"security", "firmware"}
    not input.emergency_mode
}

# ---------------------------------------------------------------
# Emergency mode restrictions
# ---------------------------------------------------------------

# Emergency mode is allowed for ops and platform admins
emergency_allowed if {
    input.action == "emergency"
    input.role in {"pact-ops-ml-training", "pact-platform-admin"}
}

# Emergency mode is NOT allowed for viewers or AI agents
deny if {
    input.action == "emergency"
    input.role in {"pact-viewer-ml-training", "pact-service-ai"}
}

# ---------------------------------------------------------------
# Service management
# ---------------------------------------------------------------

# Only these services can be restarted without emergency mode
restartable_services := {
    "chronyd",
    "nvidia-persistenced",
    "lattice-node-agent",
    "metrics-exporter",
}

service_restart_allowed if {
    input.action == "service_restart"
    input.service_name in restartable_services
}

# Restarting unlisted services requires emergency mode
deny if {
    input.action == "service_restart"
    not input.service_name in restartable_services
    not input.emergency_mode
}
