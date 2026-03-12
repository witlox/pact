//! Journal state machine — the application state managed by Raft.

use std::collections::{BTreeMap, HashMap};

use pact_common::types::{
    AdminOperation, BootOverlay, ConfigEntry, ConfigState, EntrySeq, NodeId, VClusterId,
    VClusterPolicy,
};
use raft_hpc_core::StateMachineState;
use serde::{Deserialize, Serialize};

use super::types::{JournalCommand, JournalResponse, JournalTypeConfig};

/// The journal's Raft state machine state.
///
/// Single source of truth for declared configuration state. All mutations
/// go through Raft consensus via `JournalCommand`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JournalState {
    /// All config entries, indexed by sequence number (`BTreeMap` for ordered iteration).
    pub entries: BTreeMap<EntrySeq, ConfigEntry>,
    /// Next sequence number to assign.
    pub next_sequence: EntrySeq,
    /// Per-node current config state.
    pub node_states: HashMap<NodeId, ConfigState>,
    /// Per-vCluster active policy.
    pub policies: HashMap<VClusterId, VClusterPolicy>,
    /// Pre-computed boot overlays per vCluster.
    pub overlays: HashMap<VClusterId, BootOverlay>,
    /// Admin operation audit log.
    pub audit_log: Vec<AdminOperation>,
    /// Node-to-vCluster assignment mapping.
    pub node_assignments: HashMap<NodeId, VClusterId>,
}

impl StateMachineState<JournalTypeConfig> for JournalState {
    fn apply(&mut self, cmd: JournalCommand) -> JournalResponse {
        match cmd {
            JournalCommand::AppendEntry(mut entry) => {
                let seq = self.next_sequence;
                entry.sequence = seq;
                self.next_sequence += 1;
                self.entries.insert(seq, entry);
                JournalResponse::EntryAppended { sequence: seq }
            }
            JournalCommand::UpdateNodeState { node_id, state } => {
                self.node_states.insert(node_id, state);
                JournalResponse::Ok
            }
            JournalCommand::SetPolicy { vcluster_id, policy } => {
                self.policies.insert(vcluster_id, policy);
                JournalResponse::Ok
            }
            JournalCommand::SetOverlay { vcluster_id, overlay } => {
                self.overlays.insert(vcluster_id, overlay);
                JournalResponse::Ok
            }
            JournalCommand::RecordOperation(op) => {
                self.audit_log.push(op);
                JournalResponse::Ok
            }
            JournalCommand::AssignNode { node_id, vcluster_id } => {
                self.node_assignments.insert(node_id, vcluster_id);
                JournalResponse::Ok
            }
        }
    }

    fn blank_response() -> JournalResponse {
        JournalResponse::Ok
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use pact_common::types::{AdminOperationType, EntryType, Identity, PrincipalType, Scope};

    use super::*;

    fn test_identity() -> Identity {
        Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        }
    }

    fn test_entry(entry_type: EntryType) -> ConfigEntry {
        ConfigEntry {
            sequence: 0, // will be overwritten by apply
            timestamp: Utc::now(),
            entry_type,
            scope: Scope::Global,
            author: test_identity(),
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        }
    }

    #[test]
    fn append_entry_assigns_sequence() {
        let mut state = JournalState::default();

        let resp = state.apply(JournalCommand::AppendEntry(test_entry(EntryType::Commit)));
        assert!(matches!(resp, JournalResponse::EntryAppended { sequence: 0 }));

        let resp = state.apply(JournalCommand::AppendEntry(test_entry(EntryType::Commit)));
        assert!(matches!(resp, JournalResponse::EntryAppended { sequence: 1 }));

        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.next_sequence, 2);
    }

    #[test]
    fn update_node_state() {
        let mut state = JournalState::default();
        state.apply(JournalCommand::UpdateNodeState {
            node_id: "node-1".into(),
            state: ConfigState::Committed,
        });
        assert_eq!(state.node_states.get("node-1"), Some(&ConfigState::Committed));

        // Update again
        state.apply(JournalCommand::UpdateNodeState {
            node_id: "node-1".into(),
            state: ConfigState::Drifted,
        });
        assert_eq!(state.node_states.get("node-1"), Some(&ConfigState::Drifted));
    }

    #[test]
    fn set_policy() {
        let mut state = JournalState::default();
        let policy = VClusterPolicy {
            vcluster_id: "ml-train".into(),
            drift_sensitivity: 5.0,
            base_commit_window_seconds: 900,
            emergency_allowed: true,
            two_person_approval: false,
            ..VClusterPolicy::default()
        };
        state.apply(JournalCommand::SetPolicy { vcluster_id: "ml-train".into(), policy });
        assert!(state.policies.contains_key("ml-train"));
    }

    #[test]
    fn set_overlay() {
        let mut state = JournalState::default();
        let overlay = BootOverlay {
            vcluster_id: "dev".into(),
            version: 1,
            data: vec![1, 2, 3],
            checksum: "abc123".into(),
        };
        state.apply(JournalCommand::SetOverlay { vcluster_id: "dev".into(), overlay });
        assert!(state.overlays.contains_key("dev"));
        assert_eq!(state.overlays["dev"].version, 1);
    }

    #[test]
    fn record_operation() {
        let mut state = JournalState::default();
        let op = AdminOperation {
            operation_id: "op-1".into(),
            timestamp: Utc::now(),
            actor: test_identity(),
            operation_type: AdminOperationType::Exec,
            scope: Scope::Node("node-1".into()),
            detail: "uname -a".into(),
        };
        state.apply(JournalCommand::RecordOperation(op));
        assert_eq!(state.audit_log.len(), 1);
        assert_eq!(state.audit_log[0].operation_id, "op-1");
    }

    #[test]
    fn serde_roundtrip() {
        let mut state = JournalState::default();
        state.apply(JournalCommand::AppendEntry(test_entry(EntryType::Commit)));
        state.apply(JournalCommand::UpdateNodeState {
            node_id: "n1".into(),
            state: ConfigState::Committed,
        });

        let json = serde_json::to_string(&state).unwrap();
        let decoded: JournalState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.entries.len(), 1);
        assert_eq!(decoded.node_states.len(), 1);
        assert_eq!(decoded.next_sequence, 1);
    }

    #[test]
    fn entries_btreemap_ordered_iteration() {
        let mut state = JournalState::default();
        // Insert entries out of order by appending multiple
        for _ in 0..5 {
            state.apply(JournalCommand::AppendEntry(test_entry(EntryType::Commit)));
        }
        // BTreeMap should iterate in key order
        let seqs: Vec<u64> = state.entries.keys().copied().collect();
        assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn assign_node() {
        let mut state = JournalState::default();
        state.apply(JournalCommand::AssignNode {
            node_id: "node-1".into(),
            vcluster_id: "ml-train".into(),
        });
        assert_eq!(state.node_assignments.get("node-1"), Some(&"ml-train".to_string()));

        // Reassign
        state.apply(JournalCommand::AssignNode {
            node_id: "node-1".into(),
            vcluster_id: "dev-sandbox".into(),
        });
        assert_eq!(state.node_assignments.get("node-1"), Some(&"dev-sandbox".to_string()));
    }
}
