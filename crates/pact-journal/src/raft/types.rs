//! Raft type configuration and command/response enums for pact-journal.

use std::fmt;
use std::io::Cursor;

use pact_common::types::{
    AdminOperation, BootOverlay, ConfigEntry, ConfigState, NodeId, VClusterId, VClusterPolicy,
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
    SetPolicy {
        vcluster_id: VClusterId,
        policy: VClusterPolicy,
    },
    /// Store a pre-computed boot overlay for a vCluster.
    SetOverlay {
        vcluster_id: VClusterId,
        overlay: BootOverlay,
    },
    /// Record an admin operation (exec log, shell session).
    RecordOperation(AdminOperation),
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
}

impl fmt::Display for JournalResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "Ok"),
            Self::EntryAppended { sequence } => write!(f, "EntryAppended(seq={sequence})"),
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
                max_drift_magnitude: 5.0,
                commit_window_seconds: 900,
                emergency_allowed: true,
                two_person_approval: false,
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
