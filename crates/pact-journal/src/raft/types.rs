//! Raft type configuration and command/response enums for pact-journal.

use std::fmt;
use std::io::Cursor;

use pact_common::types::{
    AdminOperation, ApprovalStatus, BootOverlay, ConfigEntry, ConfigState, EnrollmentState,
    Identity, NodeEnrollment, NodeId, PendingApproval, VClusterId, VClusterPolicy,
};
use serde::{Deserialize, Serialize};

/// Commands that go through Raft consensus.
///
/// See `docs/architecture/journal-design.md` for what goes through Raft
/// vs. what is served directly from local state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalCommand {
    /// Append a new config entry (commit, rollback, policy update, etc.)
    AppendEntry(ConfigEntry),
    /// Update a node's config state (committed, drifted, emergency, etc.)
    UpdateNodeState { node_id: NodeId, state: ConfigState },
    /// Set or update a vCluster policy.
    SetPolicy { vcluster_id: VClusterId, policy: VClusterPolicy },
    /// Store a pre-computed boot overlay for a vCluster.
    SetOverlay { vcluster_id: VClusterId, overlay: BootOverlay },
    /// Record an admin operation (exec log, shell session).
    RecordOperation(AdminOperation),
    /// Assign a node to a vCluster.
    AssignNode { node_id: NodeId, vcluster_id: VClusterId },
    /// Create a pending two-person approval request.
    CreateApproval(PendingApproval),
    /// Decide on a pending approval (approve or reject).
    DecideApproval { approval_id: String, approver: Identity, decision: ApprovalStatus },
    /// Register a node for enrollment (admin pre-enrollment).
    RegisterNode { enrollment: NodeEnrollment },
    /// Activate a node (agent presented valid hardware identity + CSR).
    ActivateNode { node_id: NodeId, cert_serial: String, cert_expires_at: String },
    /// Deactivate a node (heartbeat timeout).
    DeactivateNode { node_id: NodeId },
    /// Revoke a node (decommission).
    RevokeNode { node_id: NodeId },
    /// Assign a node to a vCluster.
    AssignNodeToVCluster { node_id: NodeId, vcluster_id: VClusterId },
    /// Unassign a node from its vCluster.
    UnassignNode { node_id: NodeId },
    /// Move a node between vClusters.
    MoveNodeVCluster { node_id: NodeId, from_vcluster_id: VClusterId, to_vcluster_id: VClusterId },
    /// Update node's last-seen timestamp.
    UpdateNodeLastSeen { node_id: NodeId, timestamp: String },
    /// Update node's certificate after renewal (does not change enrollment state).
    UpdateNodeCert { node_id: NodeId, cert_serial: String, cert_expires_at: String },
}

impl fmt::Display for JournalCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppendEntry(e) => write!(f, "AppendEntry(seq={})", e.sequence),
            Self::UpdateNodeState { node_id, state } => {
                write!(f, "UpdateNodeState({node_id}, {state:?})")
            }
            Self::SetPolicy { vcluster_id, .. } => write!(f, "SetPolicy({vcluster_id})"),
            Self::SetOverlay { vcluster_id, .. } => write!(f, "SetOverlay({vcluster_id})"),
            Self::RecordOperation(op) => write!(f, "RecordOperation({})", op.operation_id),
            Self::AssignNode { node_id, vcluster_id } => {
                write!(f, "AssignNode({node_id} → {vcluster_id})")
            }
            Self::CreateApproval(a) => write!(f, "CreateApproval({})", a.approval_id),
            Self::DecideApproval { approval_id, decision, .. } => {
                write!(f, "DecideApproval({approval_id}, {decision:?})")
            }
            Self::RegisterNode { enrollment } => {
                write!(f, "RegisterNode({})", enrollment.node_id)
            }
            Self::ActivateNode { node_id, .. } => write!(f, "ActivateNode({node_id})"),
            Self::DeactivateNode { node_id } => write!(f, "DeactivateNode({node_id})"),
            Self::RevokeNode { node_id } => write!(f, "RevokeNode({node_id})"),
            Self::AssignNodeToVCluster { node_id, vcluster_id } => {
                write!(f, "AssignNodeToVCluster({node_id} → {vcluster_id})")
            }
            Self::UnassignNode { node_id } => write!(f, "UnassignNode({node_id})"),
            Self::MoveNodeVCluster { node_id, to_vcluster_id, .. } => {
                write!(f, "MoveNodeVCluster({node_id} → {to_vcluster_id})")
            }
            Self::UpdateNodeLastSeen { node_id, .. } => {
                write!(f, "UpdateNodeLastSeen({node_id})")
            }
            Self::UpdateNodeCert { node_id, .. } => {
                write!(f, "UpdateNodeCert({node_id})")
            }
        }
    }
}

/// Response from applying a journal command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalResponse {
    /// Command applied successfully.
    Ok,
    /// Entry appended, returns the sequence number.
    EntryAppended { sequence: u64 },
    /// Validation failed — deterministic rejection (same on all replicas).
    ValidationError { reason: String },
    /// Enrollment-specific result with enrollment details.
    EnrollmentResult { node_id: NodeId, state: EnrollmentState, vcluster_id: Option<VClusterId> },
}

impl fmt::Display for JournalResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "Ok"),
            Self::EntryAppended { sequence } => write!(f, "EntryAppended(seq={sequence})"),
            Self::ValidationError { reason } => write!(f, "ValidationError({reason})"),
            Self::EnrollmentResult { node_id, state, .. } => {
                write!(f, "EnrollmentResult({node_id}, {state:?})")
            }
        }
    }
}

openraft::declare_raft_types!(
    pub JournalTypeConfig:
        D = JournalCommand,
        R = JournalResponse,
        NodeId = u64,
        Node = openraft::impls::BasicNode,
        SnapshotData = Cursor<Vec<u8>>,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_display() {
        let cmd = JournalCommand::UpdateNodeState {
            node_id: "node-1".into(),
            state: ConfigState::Committed,
        };
        let s = format!("{cmd}");
        assert!(s.contains("node-1"));
    }

    #[test]
    fn response_display() {
        let r = JournalResponse::EntryAppended { sequence: 42 };
        assert_eq!(format!("{r}"), "EntryAppended(seq=42)");
    }

    #[test]
    fn command_serde_roundtrip() {
        let cmd = JournalCommand::SetPolicy {
            vcluster_id: "ml-train".into(),
            policy: VClusterPolicy {
                vcluster_id: "ml-train".into(),
                drift_sensitivity: 5.0,
                base_commit_window_seconds: 900,
                emergency_allowed: true,
                two_person_approval: false,
                ..VClusterPolicy::default()
            },
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let decoded: JournalCommand = serde_json::from_str(&json).unwrap();
        if let JournalCommand::SetPolicy { vcluster_id, .. } = decoded {
            assert_eq!(vcluster_id, "ml-train");
        } else {
            panic!("wrong variant");
        }
    }
}
