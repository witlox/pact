//! Delegation commands — operations delegated to external systems.
//!
//! These commands call lattice (drain/cordon/uncordon) or OpenCHAMI
//! (reboot/reimage) APIs. pact acts as a unified admin interface.
//! Each delegation is audit-logged in the journal before attempting
//! the external call.

use pact_common::config::DelegationConfig;
use pact_common::proto::config::{
    scope::Scope as ProtoScope, ConfigEntry as ProtoConfigEntry, Identity as ProtoIdentity,
    Scope as ProtoScopeMsg,
};
use pact_common::proto::journal::{config_service_client::ConfigServiceClient, AppendEntryRequest};
use tonic::transport::Channel;

use super::openchami::OpenChamiClient;

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

/// Record a delegation request in the journal for audit trail.
async fn audit_delegation(
    client: &mut ConfigServiceClient<Channel>,
    command: &str,
    node_id: &str,
    target_system: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<u64> {
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 12, // SERVICE_LIFECYCLE — delegation operations
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::NodeId(node_id.to_string())) }),
        author: Some(ProtoIdentity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: format!("delegate:{target_system}:{command}:{node_id}"),
        ttl: None,
        emergency_reason: None,
    };

    let resp = client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
        .map_err(|e| anyhow::anyhow!("audit logging for {command} failed: {e}"))?;

    Ok(resp.into_inner().sequence)
}

// --- Lattice delegation ---

/// Drain a node via lattice scheduler API.
///
/// Removes all running workloads from the node and prevents new scheduling.
/// Records the delegation in the journal for audit trail.
pub async fn drain_node(
    client: &mut ConfigServiceClient<Channel>,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    let audit_seq = audit_delegation(client, "drain", node_id, "lattice", principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let Some(ref endpoint) = delegation_config.lattice_endpoint else {
        return DelegationResult {
            command: "drain".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("lattice endpoint not configured ({audit_msg})"),
        };
    };

    let config = lattice_client::ClientConfig {
        endpoint: endpoint.clone(),
        timeout_secs: delegation_config.timeout_secs,
        token: delegation_config.lattice_token.clone(),
    };

    match lattice_client::LatticeClient::connect(config).await {
        Ok(mut lc) => match lc.drain_node(node_id, "pact drain").await {
            Ok(resp) => DelegationResult {
                command: "drain".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: resp.success,
                message: format!(
                    "drained ({} active allocations, {audit_msg})",
                    resp.active_allocations
                ),
            },
            Err(e) => DelegationResult {
                command: "drain".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: false,
                message: format!("{e} ({audit_msg})"),
            },
        },
        Err(e) => DelegationResult {
            command: "drain".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("connection failed: {e} ({audit_msg})"),
        },
    }
}

/// Cordon a node via lattice scheduler API.
///
/// Removes node from scheduling but does not affect running workloads.
/// Records the delegation in the journal for audit trail.
pub async fn cordon_node(
    client: &mut ConfigServiceClient<Channel>,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    let audit_seq = audit_delegation(client, "cordon", node_id, "lattice", principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let Some(ref endpoint) = delegation_config.lattice_endpoint else {
        return DelegationResult {
            command: "cordon".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("lattice endpoint not configured ({audit_msg})"),
        };
    };

    let config = lattice_client::ClientConfig {
        endpoint: endpoint.clone(),
        timeout_secs: delegation_config.timeout_secs,
        token: delegation_config.lattice_token.clone(),
    };

    match lattice_client::LatticeClient::connect(config).await {
        Ok(mut lc) => match lc.disable_node(node_id, "pact cordon").await {
            Ok(resp) => DelegationResult {
                command: "cordon".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: resp.success,
                message: format!("cordoned ({audit_msg})"),
            },
            Err(e) => DelegationResult {
                command: "cordon".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: false,
                message: format!("{e} ({audit_msg})"),
            },
        },
        Err(e) => DelegationResult {
            command: "cordon".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("connection failed: {e} ({audit_msg})"),
        },
    }
}

/// Uncordon a node via lattice scheduler API.
///
/// Returns node to scheduling pool.
/// Records the delegation in the journal for audit trail.
pub async fn uncordon_node(
    client: &mut ConfigServiceClient<Channel>,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    let audit_seq = audit_delegation(client, "uncordon", node_id, "lattice", principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let Some(ref endpoint) = delegation_config.lattice_endpoint else {
        return DelegationResult {
            command: "uncordon".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("lattice endpoint not configured ({audit_msg})"),
        };
    };

    let config = lattice_client::ClientConfig {
        endpoint: endpoint.clone(),
        timeout_secs: delegation_config.timeout_secs,
        token: delegation_config.lattice_token.clone(),
    };

    match lattice_client::LatticeClient::connect(config).await {
        Ok(mut lc) => match lc.enable_node(node_id).await {
            Ok(resp) => DelegationResult {
                command: "uncordon".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: resp.success,
                message: format!("uncordoned ({audit_msg})"),
            },
            Err(e) => DelegationResult {
                command: "uncordon".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: false,
                message: format!("{e} ({audit_msg})"),
            },
        },
        Err(e) => DelegationResult {
            command: "uncordon".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("connection failed: {e} ({audit_msg})"),
        },
    }
}

/// Cancel a drain via lattice scheduler API.
///
/// Returns a draining node to the Ready state.
/// Records the delegation in the journal for audit trail.
pub async fn undrain_node(
    client: &mut ConfigServiceClient<Channel>,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    let audit_seq = audit_delegation(client, "undrain", node_id, "lattice", principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let Some(ref endpoint) = delegation_config.lattice_endpoint else {
        return DelegationResult {
            command: "undrain".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("lattice endpoint not configured ({audit_msg})"),
        };
    };

    let config = lattice_client::ClientConfig {
        endpoint: endpoint.clone(),
        timeout_secs: delegation_config.timeout_secs,
        token: delegation_config.lattice_token.clone(),
    };

    match lattice_client::LatticeClient::connect(config).await {
        Ok(mut lc) => match lc.undrain_node(node_id).await {
            Ok(resp) => DelegationResult {
                command: "undrain".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: resp.success,
                message: format!("undrained ({audit_msg})"),
            },
            Err(e) => DelegationResult {
                command: "undrain".into(),
                node_id: node_id.into(),
                target_system: "lattice".into(),
                success: false,
                message: format!("{e} ({audit_msg})"),
            },
        },
        Err(e) => DelegationResult {
            command: "undrain".into(),
            node_id: node_id.into(),
            target_system: "lattice".into(),
            success: false,
            message: format!("connection failed: {e} ({audit_msg})"),
        },
    }
}

// --- OpenCHAMI delegation ---

/// Reboot a node via OpenCHAMI Redfish API.
///
/// Triggers a BMC-level reboot through the management network.
/// Records the delegation in the journal for audit trail.
pub async fn reboot_node(
    client: &mut ConfigServiceClient<Channel>,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    let audit_seq = audit_delegation(client, "reboot", node_id, "OpenCHAMI", principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let Some(ref smd_url) = delegation_config.openchami_smd_url else {
        return DelegationResult {
            command: "reboot".into(),
            node_id: node_id.into(),
            target_system: "OpenCHAMI".into(),
            success: false,
            message: format!("OpenCHAMI SMD URL not configured ({audit_msg})"),
        };
    };

    let oc = OpenChamiClient::new(
        smd_url,
        delegation_config.openchami_token.as_deref(),
        delegation_config.timeout_secs,
    );

    match oc.reboot(node_id).await {
        Ok(msg) => DelegationResult {
            command: "reboot".into(),
            node_id: node_id.into(),
            target_system: "OpenCHAMI".into(),
            success: true,
            message: format!("{msg} ({audit_msg})"),
        },
        Err(e) => DelegationResult {
            command: "reboot".into(),
            node_id: node_id.into(),
            target_system: "OpenCHAMI".into(),
            success: false,
            message: format!("{e} ({audit_msg})"),
        },
    }
}

/// Reimage a node via OpenCHAMI Manta API.
///
/// Triggers a full re-image of the node's SquashFS root.
/// Records the delegation in the journal for audit trail.
pub async fn reimage_node(
    client: &mut ConfigServiceClient<Channel>,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    let audit_seq =
        audit_delegation(client, "reimage", node_id, "OpenCHAMI", principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let Some(ref smd_url) = delegation_config.openchami_smd_url else {
        return DelegationResult {
            command: "reimage".into(),
            node_id: node_id.into(),
            target_system: "OpenCHAMI".into(),
            success: false,
            message: format!("OpenCHAMI SMD URL not configured ({audit_msg})"),
        };
    };

    let oc = OpenChamiClient::new(
        smd_url,
        delegation_config.openchami_token.as_deref(),
        delegation_config.timeout_secs,
    );

    match oc.reimage(node_id).await {
        Ok(msg) => DelegationResult {
            command: "reimage".into(),
            node_id: node_id.into(),
            target_system: "OpenCHAMI".into(),
            success: true,
            message: format!("{msg} ({audit_msg})"),
        },
        Err(e) => DelegationResult {
            command: "reimage".into(),
            node_id: node_id.into(),
            target_system: "OpenCHAMI".into(),
            success: false,
            message: format!("{e} ({audit_msg})"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn format_with_audit_seq() {
        let result = DelegationResult {
            command: "drain".into(),
            node_id: "node-001".into(),
            target_system: "lattice".into(),
            success: false,
            message: "lattice endpoint not configured (audit seq:42)".into(),
        };
        let output = format_delegation_result(&result);
        assert!(output.contains("audit seq:42"));
        assert!(output.contains("FAILED"));
    }
}
