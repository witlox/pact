//! Node management backend abstraction.
//!
//! Trait + types for delegating BMC/power operations to CSM or OpenCHAMI.
//! One backend per deployment (NM-I1). Implementations live in pact-cli.
//!
//! See specs/architecture/interfaces/node-management.md

use serde::{Deserialize, Serialize};

/// Pluggable backend for BMC/power operations.
/// One implementation per deployment (NM-I1).
///
/// Uses RPITIT (NM-ADV-2) — no async_trait dependency needed.
pub trait NodeManagementBackend: Send + Sync {
    /// Power cycle a node. (NM-I2: caller must audit before calling)
    fn reboot(
        &self,
        node_id: &str,
    ) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send;

    /// Reimage a node (reboot with fresh boot from infrastructure).
    /// CSM: BOS session (operation: reboot). OpenCHAMI: Redfish PowerCycle.
    /// Semantics are normalized (NM-I5).
    fn reimage(
        &self,
        node_id: &str,
    ) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send;

    /// HSM path prefix for this backend.
    /// CSM: "/smd/hsm/v2". OpenCHAMI: "/hsm/v2".
    fn hsm_path_prefix(&self) -> &'static str;

    /// Backend display name for error messages.
    fn backend_name(&self) -> &'static str;
}

/// Backend type selection. One per deployment (NM-I1).
/// No default — must be explicitly configured (NM-ADV-4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeMgmtBackendType {
    /// HPE Cray System Management (CAPMC + BOS + HSM)
    Csm,
    /// OpenCHAMI (SMD Redfish + BSS + HSM)
    Ochami,
}

impl NodeMgmtBackendType {
    /// Display name for audit entries (NM-ADV-5: needed before backend is created).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Csm => "CSM",
            Self::Ochami => "OpenCHAMI",
        }
    }
}

impl std::fmt::Display for NodeMgmtBackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Errors from node management delegation (NM-I3).
#[derive(Debug, thiserror::Error)]
pub enum NodeMgmtError {
    #[error("node management backend not configured")]
    NotConfigured,

    #[error("backend unreachable: {0}")]
    Unreachable(String),

    #[error("backend returned error: HTTP {status} — {body}")]
    BackendError { status: u16, body: String },

    #[error("authentication failed: {0}")]
    AuthError(String),

    #[error("no boot template found for node {0}")]
    NoBootTemplate(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_type_display_names() {
        assert_eq!(NodeMgmtBackendType::Csm.display_name(), "CSM");
        assert_eq!(NodeMgmtBackendType::Ochami.display_name(), "OpenCHAMI");
    }

    #[test]
    fn backend_type_serde_roundtrip() {
        let csm: NodeMgmtBackendType = serde_json::from_str("\"csm\"").unwrap();
        assert_eq!(csm, NodeMgmtBackendType::Csm);
        let ochami: NodeMgmtBackendType = serde_json::from_str("\"ochami\"").unwrap();
        assert_eq!(ochami, NodeMgmtBackendType::Ochami);

        assert_eq!(serde_json::to_string(&NodeMgmtBackendType::Csm).unwrap(), "\"csm\"");
        assert_eq!(serde_json::to_string(&NodeMgmtBackendType::Ochami).unwrap(), "\"ochami\"");
    }

    #[test]
    fn error_messages() {
        let e = NodeMgmtError::NotConfigured;
        assert_eq!(e.to_string(), "node management backend not configured");

        let e = NodeMgmtError::BackendError { status: 500, body: "internal".into() };
        assert!(e.to_string().contains("500"));

        let e = NodeMgmtError::NoBootTemplate("x1000c0s0b0n0".into());
        assert!(e.to_string().contains("x1000c0s0b0n0"));
    }
}
