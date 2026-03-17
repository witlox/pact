//! Delegation commands — operations delegated to external systems.
//!
//! These commands call lattice (drain/cordon/uncordon) or OpenCHAMI
//! (reboot/reimage) APIs. pact acts as a unified admin interface.

/// Result of a delegation command.
#[derive(Debug, Clone)]
pub struct DelegationResult {
    pub command: String,
    pub node_id: String,
    pub target_system: String,
    pub success: bool,
    pub message: String,
}

/// Format delegation result for display.
pub fn format_delegation_result(result: &DelegationResult) -> String {
    if result.success {
        format!(
            "{} on {} → {} (via {})",
            result.command, result.node_id, result.message, result.target_system,
        )
    } else {
        format!(
            "FAILED: {} on {} → {} (via {})",
            result.command, result.node_id, result.message, result.target_system,
        )
    }
}

// --- Lattice delegation ---

/// Drain a node via lattice scheduler API.
///
/// Removes all running workloads from the node and prevents new scheduling.
pub fn drain_node(node_id: &str) -> DelegationResult {
    // TODO: Call lattice drain API via lattice Rust client (A-Int1)
    DelegationResult {
        command: "drain".into(),
        node_id: node_id.into(),
        target_system: "lattice".into(),
        success: false,
        message: "lattice drain API not yet integrated".into(),
    }
}

/// Cordon a node via lattice scheduler API.
///
/// Removes node from scheduling but does not affect running workloads.
pub fn cordon_node(node_id: &str) -> DelegationResult {
    // TODO: Call lattice cordon API via lattice Rust client
    DelegationResult {
        command: "cordon".into(),
        node_id: node_id.into(),
        target_system: "lattice".into(),
        success: false,
        message: "lattice cordon API not yet integrated".into(),
    }
}

/// Uncordon a node via lattice scheduler API.
///
/// Returns node to scheduling pool.
pub fn uncordon_node(node_id: &str) -> DelegationResult {
    // TODO: Call lattice uncordon API via lattice Rust client
    DelegationResult {
        command: "uncordon".into(),
        node_id: node_id.into(),
        target_system: "lattice".into(),
        success: false,
        message: "lattice uncordon API not yet integrated".into(),
    }
}

// --- OpenCHAMI delegation ---

/// Reboot a node via OpenCHAMI Redfish API.
///
/// Triggers a BMC-level reboot through the management network.
pub fn reboot_node(node_id: &str) -> DelegationResult {
    // TODO: Call OpenCHAMI Redfish API (A-Int2: client status unknown)
    DelegationResult {
        command: "reboot".into(),
        node_id: node_id.into(),
        target_system: "OpenCHAMI".into(),
        success: false,
        message: "OpenCHAMI reboot API not yet integrated".into(),
    }
}

/// Reimage a node via OpenCHAMI Manta API.
///
/// Triggers a full re-image of the node's SquashFS root.
pub fn reimage_node(node_id: &str) -> DelegationResult {
    // TODO: Call OpenCHAMI Manta API (A-Int2: client status unknown)
    DelegationResult {
        command: "reimage".into(),
        node_id: node_id.into(),
        target_system: "OpenCHAMI".into(),
        success: false,
        message: "OpenCHAMI reimage API not yet integrated".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_returns_not_integrated() {
        let result = drain_node("node-001");
        assert!(!result.success);
        assert!(result.message.contains("not yet integrated"));
        assert_eq!(result.target_system, "lattice");
    }

    #[test]
    fn cordon_returns_not_integrated() {
        let result = cordon_node("node-001");
        assert!(!result.success);
        assert_eq!(result.command, "cordon");
    }

    #[test]
    fn uncordon_returns_not_integrated() {
        let result = uncordon_node("node-001");
        assert!(!result.success);
        assert_eq!(result.command, "uncordon");
    }

    #[test]
    fn reboot_returns_not_integrated() {
        let result = reboot_node("node-001");
        assert!(!result.success);
        assert_eq!(result.target_system, "OpenCHAMI");
    }

    #[test]
    fn reimage_returns_not_integrated() {
        let result = reimage_node("node-001");
        assert!(!result.success);
        assert_eq!(result.target_system, "OpenCHAMI");
    }

    #[test]
    fn format_success() {
        let result = DelegationResult {
            command: "drain".into(),
            node_id: "node-001".into(),
            target_system: "lattice".into(),
            success: true,
            message: "drained successfully".into(),
        };
        let output = format_delegation_result(&result);
        assert!(output.contains("drain on node-001"));
        assert!(!output.contains("FAILED"));
    }

    #[test]
    fn format_failure() {
        let result = DelegationResult {
            command: "reboot".into(),
            node_id: "node-002".into(),
            target_system: "OpenCHAMI".into(),
            success: false,
            message: "BMC unreachable".into(),
        };
        let output = format_delegation_result(&result);
        assert!(output.contains("FAILED"));
        assert!(output.contains("BMC unreachable"));
    }
}
