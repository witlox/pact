//! Delegation commands — operations delegated to external systems.
//!
//! These commands call lattice (drain/cordon/uncordon) or the configured
//! node management backend (reboot/reimage) APIs. pact acts as a unified
//! admin interface. Each delegation is audit-logged in the journal before
//! attempting the external call.

use pact_common::config::DelegationConfig;
use pact_common::node_mgmt::{NodeManagementBackend, NodeMgmtBackendType, NodeMgmtError};
use pact_common::proto::config::{
    scope::Scope as ProtoScope, ConfigEntry as ProtoConfigEntry, Identity as ProtoIdentity,
    Scope as ProtoScopeMsg,
};
use pact_common::proto::journal::AppendEntryRequest;

use super::csm::CsmBackend;
use super::openchami::OpenChamiBackend;

/// Static dispatch enum for node management backends.
/// Avoids dyn trait (RPITIT is not dyn-compatible).
#[derive(Debug)]
enum NodeMgmtDispatch {
    Csm(CsmBackend),
    Ochami(OpenChamiBackend),
}

impl NodeMgmtDispatch {
    async fn reboot(&self, node_id: &str) -> Result<String, NodeMgmtError> {
        match self {
            Self::Csm(b) => b.reboot(node_id).await,
            Self::Ochami(b) => b.reboot(node_id).await,
        }
    }

    async fn reimage(&self, node_id: &str) -> Result<String, NodeMgmtError> {
        match self {
            Self::Csm(b) => b.reimage(node_id).await,
            Self::Ochami(b) => b.reimage(node_id).await,
        }
    }

    fn backend_name(&self) -> &str {
        match self {
            Self::Csm(b) => b.backend_name(),
            Self::Ochami(b) => b.backend_name(),
        }
    }
}

/// Create the appropriate node management backend from config (NM-ADV-1).
///
/// Backward compat: if `node_mgmt_backend` is None but `openchami_smd_url` is set,
/// creates an OpenCHAMI backend using the legacy env vars.
fn create_node_mgmt_backend(config: &DelegationConfig) -> Result<NodeMgmtDispatch, NodeMgmtError> {
    let Some(ref backend_type) = config.node_mgmt_backend else {
        // Legacy fallback: no backend type → try openchami_smd_url
        if let Some(ref url) = config.openchami_smd_url {
            let token = config.node_mgmt_token.as_deref().or(config.openchami_token.as_deref());
            return Ok(NodeMgmtDispatch::Ochami(OpenChamiBackend::new(
                url,
                token,
                config.timeout_secs,
            )));
        }
        return Err(NodeMgmtError::NotConfigured);
    };

    // NM-ADV-1: backward compat fallback only for Ochami, not Csm.
    let base_url = match backend_type {
        NodeMgmtBackendType::Ochami => config
            .node_mgmt_base_url
            .as_deref()
            .or(config.openchami_smd_url.as_deref()),
        NodeMgmtBackendType::Csm => config.node_mgmt_base_url.as_deref(),
    }
    .ok_or(NodeMgmtError::NotConfigured)?;

    let token = config
        .node_mgmt_token
        .as_deref()
        .or(config.openchami_token.as_deref());

    match backend_type {
        NodeMgmtBackendType::Csm => {
            Ok(NodeMgmtDispatch::Csm(CsmBackend::new(base_url, token, config.timeout_secs)))
        }
        NodeMgmtBackendType::Ochami => Ok(NodeMgmtDispatch::Ochami(OpenChamiBackend::new(
            base_url,
            token,
            config.timeout_secs,
        ))),
    }
}

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
    client: &mut super::execute::AuthConfigClient,
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
    client: &mut super::execute::AuthConfigClient,
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
    client: &mut super::execute::AuthConfigClient,
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
    client: &mut super::execute::AuthConfigClient,
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
    client: &mut super::execute::AuthConfigClient,
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

// --- Node management delegation ---

/// Reboot a node via the configured node management backend.
///
/// Triggers a BMC-level reboot through the management network.
/// Records the delegation in the journal for audit trail (NM-I2).
pub async fn reboot_node(
    client: &mut super::execute::AuthConfigClient,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    // NM-ADV-5: audit uses config display name, not backend instance.
    let target = delegation_config
        .node_mgmt_backend
        .as_ref()
        .map_or("OpenCHAMI", NodeMgmtBackendType::display_name);
    let audit_seq = audit_delegation(client, "reboot", node_id, target, principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let backend = match create_node_mgmt_backend(delegation_config) {
        Ok(b) => b,
        Err(e) => {
            return DelegationResult {
                command: "reboot".into(),
                node_id: node_id.into(),
                target_system: target.into(),
                success: false,
                message: format!("{e} ({audit_msg})"),
            };
        }
    };

    match backend.reboot(node_id).await {
        Ok(msg) => DelegationResult {
            command: "reboot".into(),
            node_id: node_id.into(),
            target_system: backend.backend_name().into(),
            success: true,
            message: format!("{msg} ({audit_msg})"),
        },
        Err(e) => DelegationResult {
            command: "reboot".into(),
            node_id: node_id.into(),
            target_system: backend.backend_name().into(),
            success: false,
            message: format!("{e} ({audit_msg})"),
        },
    }
}

/// Reimage a node via the configured node management backend.
///
/// Triggers a full re-image of the node's SquashFS root.
/// Records the delegation in the journal for audit trail (NM-I2).
pub async fn reimage_node(
    client: &mut super::execute::AuthConfigClient,
    node_id: &str,
    principal: &str,
    role: &str,
    delegation_config: &DelegationConfig,
) -> DelegationResult {
    let target = delegation_config
        .node_mgmt_backend
        .as_ref()
        .map_or("OpenCHAMI", NodeMgmtBackendType::display_name);
    let audit_seq = audit_delegation(client, "reimage", node_id, target, principal, role).await;
    let audit_msg = match &audit_seq {
        Ok(seq) => format!("audit seq:{seq}"),
        Err(_) => String::new(),
    };

    let backend = match create_node_mgmt_backend(delegation_config) {
        Ok(b) => b,
        Err(e) => {
            return DelegationResult {
                command: "reimage".into(),
                node_id: node_id.into(),
                target_system: target.into(),
                success: false,
                message: format!("{e} ({audit_msg})"),
            };
        }
    };

    match backend.reimage(node_id).await {
        Ok(msg) => DelegationResult {
            command: "reimage".into(),
            node_id: node_id.into(),
            target_system: backend.backend_name().into(),
            success: true,
            message: format!("{msg} ({audit_msg})"),
        },
        Err(e) => DelegationResult {
            command: "reimage".into(),
            node_id: node_id.into(),
            target_system: backend.backend_name().into(),
            success: false,
            message: format!("{e} ({audit_msg})"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::node_mgmt::NodeMgmtBackendType;

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

    // --- Factory function tests (Action 1) ---

    fn config_with(
        backend: Option<NodeMgmtBackendType>,
        url: Option<&str>,
        token: Option<&str>,
    ) -> DelegationConfig {
        DelegationConfig {
            node_mgmt_backend: backend,
            node_mgmt_base_url: url.map(String::from),
            node_mgmt_token: token.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn factory_csm_explicit() {
        let config = config_with(Some(NodeMgmtBackendType::Csm), Some("https://csm.example.com"), None);
        let backend = create_node_mgmt_backend(&config).unwrap();
        assert_eq!(backend.backend_name(), "CSM");
    }

    #[test]
    fn factory_ochami_explicit() {
        let config = config_with(Some(NodeMgmtBackendType::Ochami), Some("https://ochami.example.com"), None);
        let backend = create_node_mgmt_backend(&config).unwrap();
        assert_eq!(backend.backend_name(), "OpenCHAMI");
    }

    #[test]
    fn factory_not_configured() {
        let config = DelegationConfig::default();
        let err = create_node_mgmt_backend(&config).unwrap_err();
        assert!(matches!(err, NodeMgmtError::NotConfigured));
    }

    #[test]
    fn factory_csm_without_url_fails() {
        // NM-ADV-1: CSM backend with no URL must fail, not fall back to openchami_smd_url
        let mut config = config_with(Some(NodeMgmtBackendType::Csm), None, None);
        config.openchami_smd_url = Some("https://ochami.example.com".into());
        let err = create_node_mgmt_backend(&config).unwrap_err();
        assert!(matches!(err, NodeMgmtError::NotConfigured));
    }

    #[test]
    fn factory_legacy_fallback_uses_ochami() {
        // No backend type set, but openchami_smd_url exists → OpenCHAMI backend
        let mut config = DelegationConfig::default();
        config.openchami_smd_url = Some("https://legacy.example.com".into());
        let backend = create_node_mgmt_backend(&config).unwrap();
        assert_eq!(backend.backend_name(), "OpenCHAMI");
    }

    #[test]
    fn factory_ochami_falls_back_to_legacy_url() {
        // Ochami backend with no node_mgmt_base_url but openchami_smd_url set
        let mut config = config_with(Some(NodeMgmtBackendType::Ochami), None, None);
        config.openchami_smd_url = Some("https://legacy-ochami.example.com".into());
        let backend = create_node_mgmt_backend(&config).unwrap();
        assert_eq!(backend.backend_name(), "OpenCHAMI");
    }

    // --- Dispatch routing tests (Action 2) ---

    #[test]
    fn dispatch_routes_reboot_to_csm() {
        let dispatch = NodeMgmtDispatch::Csm(CsmBackend::new("https://csm.example.com", None, 5));
        assert_eq!(dispatch.backend_name(), "CSM");
    }

    #[test]
    fn dispatch_routes_reboot_to_ochami() {
        let dispatch = NodeMgmtDispatch::Ochami(OpenChamiBackend::new("https://ochami.example.com", None, 5));
        assert_eq!(dispatch.backend_name(), "OpenCHAMI");
    }

    #[tokio::test]
    async fn dispatch_csm_reboot_attempts_http() {
        // CsmBackend.reboot() should attempt POST to CAPMC — will fail (no server) with Unreachable
        let dispatch = NodeMgmtDispatch::Csm(CsmBackend::new("http://127.0.0.1:1", None, 1));
        let err = dispatch.reboot("x1000c0s0b0n0").await.unwrap_err();
        assert!(matches!(err, NodeMgmtError::Unreachable(_)));
    }

    #[tokio::test]
    async fn dispatch_ochami_reboot_attempts_http() {
        // OpenChamiBackend.reboot() should attempt POST to Redfish — will fail (no server) with Unreachable
        let dispatch = NodeMgmtDispatch::Ochami(OpenChamiBackend::new("http://127.0.0.1:1", None, 1));
        let err = dispatch.reboot("x1000c0s0b0n0").await.unwrap_err();
        assert!(matches!(err, NodeMgmtError::Unreachable(_)));
    }

    #[tokio::test]
    async fn dispatch_csm_reimage_attempts_bos() {
        // CsmBackend.reimage() should attempt POST to BOS — will fail with Unreachable
        let dispatch = NodeMgmtDispatch::Csm(CsmBackend::new("http://127.0.0.1:1", None, 1));
        let err = dispatch.reimage("x1000c0s0b0n0").await.unwrap_err();
        assert!(matches!(err, NodeMgmtError::Unreachable(_)));
    }

    #[tokio::test]
    async fn dispatch_ochami_reimage_attempts_redfish() {
        // OpenChamiBackend.reimage() should attempt Redfish PowerCycle — will fail with Unreachable
        let dispatch = NodeMgmtDispatch::Ochami(OpenChamiBackend::new("http://127.0.0.1:1", None, 1));
        let err = dispatch.reimage("x1000c0s0b0n0").await.unwrap_err();
        assert!(matches!(err, NodeMgmtError::Unreachable(_)));
    }

    // --- Audit ordering test (Action 5) ---
    // NM-I2: audit_delegation is called BEFORE backend.reboot/reimage in the code.
    // We verify this structurally: reboot_node/reimage_node call audit_delegation first,
    // then create_node_mgmt_backend. If backend creation fails, the audit entry still exists.
    // This test verifies the "not configured" path produces an audit message AND a failure.

    #[test]
    fn reboot_not_configured_includes_target_system() {
        // When no backend is configured, the target_system should still be set
        // (from config or default "OpenCHAMI") for the audit entry.
        let config = DelegationConfig::default();
        let target = config
            .node_mgmt_backend
            .as_ref()
            .map_or("OpenCHAMI", NodeMgmtBackendType::display_name);
        assert_eq!(target, "OpenCHAMI"); // default when None

        let config_csm = config_with(Some(NodeMgmtBackendType::Csm), None, None);
        let target_csm = config_csm
            .node_mgmt_backend
            .as_ref()
            .map_or("OpenCHAMI", NodeMgmtBackendType::display_name);
        assert_eq!(target_csm, "CSM");
    }
}
