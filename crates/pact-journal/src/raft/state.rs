//! Journal state machine — the application state managed by Raft.

use std::collections::{BTreeMap, HashMap};

use pact_common::types::{
    AdminOperation, ApprovalStatus, BootOverlay, ConfigEntry, ConfigState, EnrollmentState,
    EntrySeq, NodeEnrollment, NodeId, PendingApproval, Scope, VClusterId, VClusterPolicy,
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
    /// Pending two-person approval requests.
    pub pending_approvals: HashMap<String, PendingApproval>,
    /// Node enrollment records indexed by node ID.
    pub enrollments: HashMap<NodeId, NodeEnrollment>,
    /// Hardware identity index: canonical hw key → node ID (for duplicate detection).
    pub hw_index: HashMap<String, NodeId>,
    /// Revoked certificate serials (Raft-replicated revocation registry).
    /// Populated on node decommission. Checked during mTLS handshake.
    #[serde(default)]
    pub revoked_serials: std::collections::HashSet<String>,
}

/// A conflict between a local entry and journal state on the same config key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictEntry {
    pub key: String,
    pub local_value: String,
    pub journal_value: String,
}

/// A node with per-node deltas that deviate from the vCluster overlay.
#[derive(Debug, Clone)]
pub struct HomogeneityWarning {
    pub node_id: NodeId,
    pub delta_keys: Vec<String>,
}

impl JournalState {
    /// Convenience method to apply a command without importing the Raft trait.
    pub fn apply_command(&mut self, cmd: JournalCommand) -> JournalResponse {
        use raft_hpc_core::StateMachineState;
        StateMachineState::<JournalTypeConfig>::apply(self, cmd)
    }

    /// Detect conflicts between local entries and current journal state (CR2).
    ///
    /// For each local entry, compare its state_delta keys against the current
    /// entries for the same vCluster. Returns conflicting keys with both values.
    pub fn detect_conflicts(
        &self,
        _node_id: &str,
        local_entries: &[ConfigEntry],
    ) -> Vec<ConflictEntry> {
        let mut conflicts = Vec::new();
        for local in local_entries {
            if let Some(ref delta) = local.state_delta {
                // Check kernel deltas against existing entries with overlapping keys
                for local_item in &delta.kernel {
                    // Find the most recent journal entry with same key
                    for existing in self.entries.values().rev() {
                        if let Some(ref existing_delta) = existing.state_delta {
                            for existing_item in &existing_delta.kernel {
                                if existing_item.key == local_item.key
                                    && existing_item.value != local_item.value
                                {
                                    conflicts.push(ConflictEntry {
                                        key: local_item.key.clone(),
                                        local_value: local_item.value.clone().unwrap_or_default(),
                                        journal_value: existing_item
                                            .value
                                            .clone()
                                            .unwrap_or_default(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        conflicts
    }

    /// Check vCluster homogeneity — find nodes with per-node deltas (ND3).
    ///
    /// Returns nodes that have committed entries scoped to them specifically
    /// (not the vCluster overlay), indicating divergence from homogeneity.
    pub fn check_homogeneity(&self, vcluster_id: &str) -> Vec<HomogeneityWarning> {
        // Find all nodes assigned to this vCluster
        let nodes: Vec<&str> = self
            .node_assignments
            .iter()
            .filter(|(_, vc)| vc.as_str() == vcluster_id)
            .map(|(n, _)| n.as_str())
            .collect();

        let mut warnings = Vec::new();
        for node_id in &nodes {
            // Find node-scoped entries (per-node deltas)
            let delta_keys: Vec<String> = self
                .entries
                .values()
                .filter(|e| matches!(&e.scope, Scope::Node(n) if n == node_id))
                .filter(|e| e.state_delta.is_some())
                .flat_map(|e| {
                    let delta = e.state_delta.as_ref().unwrap();
                    delta.kernel.iter().map(|d| d.key.clone()).collect::<Vec<_>>()
                })
                .collect();

            if !delta_keys.is_empty() {
                warnings.push(HomogeneityWarning { node_id: (*node_id).to_string(), delta_keys });
            }
        }
        warnings
    }
}

/// Compute a canonical key from hardware identity for duplicate detection (E2).
///
/// Includes both MAC address and BMC serial (when present) to ensure
/// hardware identity uniqueness within a domain.
pub fn hw_canonical_key(hw: &pact_common::types::HardwareIdentity) -> String {
    match &hw.bmc_serial {
        Some(serial) if !serial.is_empty() => {
            format!("mac:{}:bmc:{}", hw.mac_address.to_lowercase(), serial.to_lowercase())
        }
        _ => format!("mac:{}", hw.mac_address.to_lowercase()),
    }
}

/// Minimum TTL: 15 minutes (ND1).
const TTL_MIN_SECONDS: u32 = 900;
/// Maximum TTL: 10 days (ND2).
const TTL_MAX_SECONDS: u32 = 864_000;
/// Maximum overlay data size: 10 MB (F32 fix).
const OVERLAY_MAX_BYTES: usize = 10 * 1024 * 1024;

impl StateMachineState<JournalTypeConfig> for JournalState {
    #[allow(clippy::too_many_lines)]
    fn apply(&mut self, cmd: JournalCommand) -> JournalResponse {
        match cmd {
            JournalCommand::AppendEntry(mut entry) => {
                // J3: authenticated authorship — reject empty principal or role.
                if entry.author.principal.is_empty() {
                    return JournalResponse::ValidationError {
                        reason: "author principal required".into(),
                    };
                }
                if entry.author.role.is_empty() {
                    return JournalResponse::ValidationError {
                        reason: "author role required".into(),
                    };
                }

                // J4: acyclic parent chain — parent must precede this entry.
                if let Some(parent) = entry.parent {
                    if parent >= self.next_sequence {
                        return JournalResponse::ValidationError {
                            reason: "parent must precede entry".into(),
                        };
                    }
                }

                // ND1/ND2: TTL bounds — if set, must be within [15 min, 10 days].
                if let Some(ttl) = entry.ttl_seconds {
                    if ttl > 0 && ttl < TTL_MIN_SECONDS {
                        return JournalResponse::ValidationError {
                            reason: format!(
                                "TTL must be >= {TTL_MIN_SECONDS} seconds (15 minutes)"
                            ),
                        };
                    }
                    if ttl > TTL_MAX_SECONDS {
                        return JournalResponse::ValidationError {
                            reason: format!("TTL must be <= {TTL_MAX_SECONDS} seconds (10 days)"),
                        };
                    }
                }

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
                // F32 fix: reject oversized overlays.
                if overlay.data.len() > OVERLAY_MAX_BYTES {
                    return JournalResponse::ValidationError {
                        reason: format!(
                            "overlay data too large: {} bytes (max {})",
                            overlay.data.len(),
                            OVERLAY_MAX_BYTES
                        ),
                    };
                }
                // J5: validate overlay checksum matches hash of data.
                let computed = pact_common::types::compute_overlay_checksum(&overlay.data);
                if overlay.checksum != computed {
                    return JournalResponse::ValidationError {
                        reason: format!(
                            "overlay checksum mismatch: expected {}, got {computed}",
                            overlay.checksum
                        ),
                    };
                }
                self.overlays.insert(vcluster_id, overlay);
                JournalResponse::Ok
            }
            JournalCommand::RecordOperation(op) => {
                self.audit_log.push(op);
                // F22 fix: cap audit log to prevent unbounded growth.
                // Oldest entries are dropped. Production: archive to Loki first.
                const AUDIT_LOG_MAX: usize = 100_000;
                if self.audit_log.len() > AUDIT_LOG_MAX {
                    let drain_count = self.audit_log.len() - AUDIT_LOG_MAX;
                    self.audit_log.drain(..drain_count);
                }
                JournalResponse::Ok
            }
            JournalCommand::AssignNode { node_id, vcluster_id } => {
                self.node_assignments.insert(node_id, vcluster_id);
                JournalResponse::Ok
            }
            JournalCommand::CreateApproval(approval) => {
                let id = approval.approval_id.clone();
                self.pending_approvals.insert(id, approval);
                JournalResponse::Ok
            }
            JournalCommand::DecideApproval { approval_id, approver, decision } => {
                match self.pending_approvals.get_mut(&approval_id) {
                    Some(approval) => {
                        if approval.status != ApprovalStatus::Pending {
                            return JournalResponse::ValidationError {
                                reason: format!(
                                    "approval {} already {:?}",
                                    approval_id, approval.status
                                ),
                            };
                        }
                        // P4: Self-approval prevention at Raft layer (F24 fix)
                        if approval.requester.principal == approver.principal {
                            return JournalResponse::ValidationError {
                                reason: "SELF_APPROVAL: cannot approve your own request (P4)"
                                    .into(),
                            };
                        }
                        approval.status = decision;
                        approval.approver = Some(approver);
                        JournalResponse::Ok
                    }
                    None => JournalResponse::ValidationError {
                        reason: format!("approval {approval_id} not found"),
                    },
                }
            }
            // --- Enrollment commands ---
            JournalCommand::RegisterNode { enrollment } => {
                // E1: reject if node already enrolled
                if self.enrollments.contains_key(&enrollment.node_id) {
                    return JournalResponse::ValidationError {
                        reason: format!("NODE_ALREADY_ENROLLED: {}", enrollment.node_id),
                    };
                }
                // E2: reject if hardware identity already registered
                let hw_key = hw_canonical_key(&enrollment.hardware_identity);
                if let Some(existing) = self.hw_index.get(&hw_key) {
                    return JournalResponse::ValidationError {
                        reason: format!(
                            "HARDWARE_IDENTITY_CONFLICT: hardware already registered to {existing}"
                        ),
                    };
                }
                let node_id = enrollment.node_id.clone();
                self.hw_index.insert(hw_key, node_id.clone());
                self.enrollments.insert(node_id.clone(), enrollment);
                JournalResponse::EnrollmentResult {
                    node_id,
                    state: EnrollmentState::Registered,
                    vcluster_id: None,
                }
            }
            JournalCommand::ActivateNode { node_id, cert_serial, cert_expires_at } => {
                match self.enrollments.get_mut(&node_id) {
                    Some(enrollment) => {
                        // E7: reject revoked nodes
                        if enrollment.state == EnrollmentState::Revoked {
                            return JournalResponse::ValidationError {
                                reason: format!("NODE_REVOKED: {node_id}"),
                            };
                        }
                        // Reject if already active
                        if enrollment.state == EnrollmentState::Active {
                            return JournalResponse::ValidationError {
                                reason: format!("ALREADY_ACTIVE: {node_id}"),
                            };
                        }
                        enrollment.state = EnrollmentState::Active;
                        enrollment.cert_serial = Some(cert_serial);
                        enrollment.cert_expires_at =
                            chrono::DateTime::parse_from_rfc3339(&cert_expires_at)
                                .ok()
                                .map(|dt| dt.with_timezone(&chrono::Utc));
                        enrollment.last_seen = Some(chrono::Utc::now());
                        let vc = enrollment.vcluster_id.clone();
                        JournalResponse::EnrollmentResult {
                            node_id,
                            state: EnrollmentState::Active,
                            vcluster_id: vc,
                        }
                    }
                    None => JournalResponse::ValidationError {
                        reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                    },
                }
            }
            JournalCommand::DeactivateNode { node_id } => {
                match self.enrollments.get_mut(&node_id) {
                    Some(enrollment) => {
                        enrollment.state = EnrollmentState::Inactive;
                        JournalResponse::Ok
                    }
                    None => JournalResponse::ValidationError {
                        reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                    },
                }
            }
            JournalCommand::RevokeNode { node_id } => match self.enrollments.get_mut(&node_id) {
                Some(enrollment) => {
                    enrollment.state = EnrollmentState::Revoked;
                    JournalResponse::Ok
                }
                None => JournalResponse::ValidationError {
                    reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                },
            },
            JournalCommand::AssignNodeToVCluster { node_id, vcluster_id } => {
                match self.enrollments.get_mut(&node_id) {
                    Some(enrollment) => {
                        enrollment.vcluster_id = Some(vcluster_id.clone());
                        // Also update the legacy node_assignments map
                        self.node_assignments.insert(node_id.clone(), vcluster_id);
                        JournalResponse::Ok
                    }
                    None => JournalResponse::ValidationError {
                        reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                    },
                }
            }
            JournalCommand::UnassignNode { node_id } => match self.enrollments.get_mut(&node_id) {
                Some(enrollment) => {
                    enrollment.vcluster_id = None;
                    self.node_assignments.remove(&node_id);
                    JournalResponse::Ok
                }
                None => JournalResponse::ValidationError {
                    reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                },
            },
            JournalCommand::MoveNodeVCluster { node_id, from_vcluster_id: _, to_vcluster_id } => {
                match self.enrollments.get_mut(&node_id) {
                    Some(enrollment) => {
                        enrollment.vcluster_id = Some(to_vcluster_id.clone());
                        self.node_assignments.insert(node_id.clone(), to_vcluster_id);
                        JournalResponse::Ok
                    }
                    None => JournalResponse::ValidationError {
                        reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                    },
                }
            }
            JournalCommand::UpdateNodeLastSeen { node_id, timestamp } => {
                match self.enrollments.get_mut(&node_id) {
                    Some(enrollment) => {
                        enrollment.last_seen = chrono::DateTime::parse_from_rfc3339(&timestamp)
                            .ok()
                            .map(|dt| dt.with_timezone(&chrono::Utc));
                        JournalResponse::Ok
                    }
                    None => JournalResponse::ValidationError {
                        reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                    },
                }
            }
            JournalCommand::UpdateNodeCert { node_id, cert_serial, cert_expires_at } => {
                match self.enrollments.get_mut(&node_id) {
                    Some(enrollment) => {
                        enrollment.cert_serial = Some(cert_serial);
                        enrollment.cert_expires_at =
                            chrono::DateTime::parse_from_rfc3339(&cert_expires_at)
                                .ok()
                                .map(|dt| dt.with_timezone(&chrono::Utc));
                        enrollment.last_seen = Some(chrono::Utc::now());
                        JournalResponse::Ok
                    }
                    None => JournalResponse::ValidationError {
                        reason: format!("NODE_NOT_ENROLLED: {node_id}"),
                    },
                }
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
    use pact_common::types::{
        AdminOperationType, DeltaAction, DeltaItem, EntryType, Identity, PrincipalType, Scope,
        StateDelta,
    };

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
        let overlay = BootOverlay::new("dev", 1, vec![1, 2, 3]);
        state.apply(JournalCommand::SetOverlay { vcluster_id: "dev".into(), overlay });
        assert!(state.overlays.contains_key("dev"));
        assert_eq!(state.overlays["dev"].version, 1);
    }

    #[test]
    fn reject_overlay_with_bad_checksum() {
        let mut state = JournalState::default();
        let overlay = BootOverlay {
            vcluster_id: "dev".into(),
            version: 1,
            data: vec![1, 2, 3],
            checksum: "bad-checksum".into(),
        };
        let resp = state.apply(JournalCommand::SetOverlay { vcluster_id: "dev".into(), overlay });
        assert!(
            matches!(resp, JournalResponse::ValidationError { reason } if reason.contains("checksum mismatch"))
        );
        assert!(!state.overlays.contains_key("dev"));
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

    // --- J3: Author validation ---

    #[test]
    fn reject_empty_principal() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.author.principal = String::new();
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(
            matches!(resp, JournalResponse::ValidationError { reason } if reason == "author principal required")
        );
        assert_eq!(state.entries.len(), 0);
    }

    #[test]
    fn reject_empty_role() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.author.role = String::new();
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(
            matches!(resp, JournalResponse::ValidationError { reason } if reason == "author role required")
        );
        assert_eq!(state.entries.len(), 0);
    }

    // --- J4: Acyclic parent chain ---

    #[test]
    fn reject_future_parent() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.parent = Some(999); // no entry at seq 999
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(
            matches!(resp, JournalResponse::ValidationError { reason } if reason == "parent must precede entry")
        );
    }

    #[test]
    fn accept_valid_parent() {
        let mut state = JournalState::default();
        // First entry at seq 0
        state.apply(JournalCommand::AppendEntry(test_entry(EntryType::Commit)));
        // Second entry referencing seq 0 as parent
        let mut entry = test_entry(EntryType::Rollback);
        entry.parent = Some(0);
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(matches!(resp, JournalResponse::EntryAppended { sequence: 1 }));
    }

    // --- ND1/ND2: TTL bounds ---

    #[test]
    fn reject_ttl_below_minimum() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.ttl_seconds = Some(300); // 5 minutes — below 15 min minimum
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(
            matches!(resp, JournalResponse::ValidationError { reason } if reason.contains("15 minutes"))
        );
        assert_eq!(state.entries.len(), 0);
    }

    #[test]
    fn reject_ttl_above_maximum() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.ttl_seconds = Some(1_000_000); // ~11.6 days — above 10 day maximum
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(
            matches!(resp, JournalResponse::ValidationError { reason } if reason.contains("10 days"))
        );
        assert_eq!(state.entries.len(), 0);
    }

    #[test]
    fn accept_ttl_at_minimum() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.ttl_seconds = Some(900); // exactly 15 minutes
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(matches!(resp, JournalResponse::EntryAppended { sequence: 0 }));
    }

    #[test]
    fn accept_ttl_at_maximum() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.ttl_seconds = Some(864_000); // exactly 10 days
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(matches!(resp, JournalResponse::EntryAppended { sequence: 0 }));
    }

    #[test]
    fn accept_no_ttl() {
        let mut state = JournalState::default();
        let mut entry = test_entry(EntryType::Commit);
        entry.ttl_seconds = None; // no TTL — persists indefinitely
        let resp = state.apply(JournalCommand::AppendEntry(entry));
        assert!(matches!(resp, JournalResponse::EntryAppended { sequence: 0 }));
    }

    // --- Conflict detection (CR2) ---

    #[test]
    fn detect_kernel_conflict() {
        let mut state = JournalState::default();
        // Journal has an entry with kernel.shmmax = 64GB
        let mut journal_entry = test_entry(EntryType::Commit);
        journal_entry.state_delta = Some(StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "kernel.shmmax".into(),
                value: Some("68719476736".into()),
                previous: None,
            }],
            ..StateDelta::default()
        });
        state.apply(JournalCommand::AppendEntry(journal_entry));

        // Local entry has kernel.shmmax = 32GB (conflict)
        let local_entry = ConfigEntry {
            state_delta: Some(StateDelta {
                kernel: vec![DeltaItem {
                    action: DeltaAction::Modify,
                    key: "kernel.shmmax".into(),
                    value: Some("34359738368".into()),
                    previous: None,
                }],
                ..StateDelta::default()
            }),
            ..test_entry(EntryType::Commit)
        };

        let conflicts = state.detect_conflicts("node-001", &[local_entry]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].key, "kernel.shmmax");
        assert_eq!(conflicts[0].local_value, "34359738368");
        assert_eq!(conflicts[0].journal_value, "68719476736");
    }

    #[test]
    fn no_conflict_when_values_match() {
        let mut state = JournalState::default();
        let mut journal_entry = test_entry(EntryType::Commit);
        journal_entry.state_delta = Some(StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "kernel.shmmax".into(),
                value: Some("68719476736".into()),
                previous: None,
            }],
            ..StateDelta::default()
        });
        state.apply(JournalCommand::AppendEntry(journal_entry));

        // Local entry has same value — no conflict
        let local_entry = ConfigEntry {
            state_delta: Some(StateDelta {
                kernel: vec![DeltaItem {
                    action: DeltaAction::Modify,
                    key: "kernel.shmmax".into(),
                    value: Some("68719476736".into()),
                    previous: None,
                }],
                ..StateDelta::default()
            }),
            ..test_entry(EntryType::Commit)
        };

        let conflicts = state.detect_conflicts("node-001", &[local_entry]);
        assert!(conflicts.is_empty());
    }

    // --- Homogeneity check (ND3) ---

    #[test]
    fn detect_heterogeneous_nodes() {
        let mut state = JournalState::default();

        // Assign two nodes to same vCluster
        state.apply(JournalCommand::AssignNode {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
        });
        state.apply(JournalCommand::AssignNode {
            node_id: "node-002".into(),
            vcluster_id: "ml-training".into(),
        });

        // node-001 has a per-node delta
        let mut entry = test_entry(EntryType::Commit);
        entry.scope = Scope::Node("node-001".into());
        entry.state_delta = Some(StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.swappiness".into(),
                value: Some("10".into()),
                previous: Some("60".into()),
            }],
            ..StateDelta::default()
        });
        state.apply(JournalCommand::AppendEntry(entry));

        let warnings = state.check_homogeneity("ml-training");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].node_id, "node-001");
        assert_eq!(warnings[0].delta_keys, vec!["vm.swappiness"]);
    }

    #[test]
    fn no_warning_for_homogeneous_cluster() {
        let mut state = JournalState::default();
        state.apply(JournalCommand::AssignNode {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
        });
        state.apply(JournalCommand::AssignNode {
            node_id: "node-002".into(),
            vcluster_id: "ml-training".into(),
        });

        // Only vCluster-scoped entries, no per-node deltas
        let mut entry = test_entry(EntryType::Commit);
        entry.scope = Scope::VCluster("ml-training".into());
        state.apply(JournalCommand::AppendEntry(entry));

        let warnings = state.check_homogeneity("ml-training");
        assert!(warnings.is_empty());
    }

    // --- Approval persistence ---

    #[test]
    fn create_and_decide_approval() {
        use pact_common::types::{ApprovalStatus, PendingApproval};

        let mut state = JournalState::default();
        let approval = PendingApproval {
            approval_id: "apr-001".into(),
            original_request: "commit".into(),
            action: "commit".into(),
            scope: Scope::VCluster("ml-training".into()),
            requester: test_identity(),
            approver: None,
            status: ApprovalStatus::Pending,
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(24),
        };
        let resp = state.apply(JournalCommand::CreateApproval(approval));
        assert!(matches!(resp, JournalResponse::Ok));
        assert_eq!(state.pending_approvals.len(), 1);
        assert!(matches!(state.pending_approvals["apr-001"].status, ApprovalStatus::Pending));

        // Approve it
        let resp = state.apply(JournalCommand::DecideApproval {
            approval_id: "apr-001".into(),
            approver: Identity {
                principal: "approver@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-platform-admin".into(),
            },
            decision: ApprovalStatus::Approved,
        });
        assert!(matches!(resp, JournalResponse::Ok));
        assert!(matches!(state.pending_approvals["apr-001"].status, ApprovalStatus::Approved));
        assert_eq!(
            state.pending_approvals["apr-001"].approver.as_ref().unwrap().principal,
            "approver@example.com"
        );
    }

    #[test]
    fn reject_decide_on_already_decided_approval() {
        use pact_common::types::{ApprovalStatus, PendingApproval};

        let mut state = JournalState::default();
        let approval = PendingApproval {
            approval_id: "apr-002".into(),
            original_request: "exec".into(),
            action: "exec".into(),
            scope: Scope::Global,
            requester: test_identity(),
            approver: None,
            status: ApprovalStatus::Pending,
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(24),
        };
        state.apply(JournalCommand::CreateApproval(approval));

        // Reject it
        state.apply(JournalCommand::DecideApproval {
            approval_id: "apr-002".into(),
            approver: test_identity(),
            decision: ApprovalStatus::Rejected,
        });

        // Try to approve already-rejected — should fail
        let resp = state.apply(JournalCommand::DecideApproval {
            approval_id: "apr-002".into(),
            approver: test_identity(),
            decision: ApprovalStatus::Approved,
        });
        assert!(matches!(resp, JournalResponse::ValidationError { .. }));
    }

    #[test]
    fn reject_decide_on_nonexistent_approval() {
        let mut state = JournalState::default();
        let resp = state.apply(JournalCommand::DecideApproval {
            approval_id: "nonexistent".into(),
            approver: test_identity(),
            decision: ApprovalStatus::Approved,
        });
        assert!(matches!(resp, JournalResponse::ValidationError { .. }));
    }

    // --- Existing tests ---

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
