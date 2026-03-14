//! ConfigService gRPC implementation.
//!
//! Writes go through Raft consensus (`raft.client_write()`).
//! Reads are served from local state machine replica (no Raft round-trip).
//! See invariants J7 (writes through Raft) and J8 (reads from local state).

use std::sync::Arc;

use openraft::Raft;
use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use pact_common::proto::config::ConfigEntry as ProtoConfigEntry;
use pact_common::proto::journal::{
    config_service_server::ConfigService, AppendEntryRequest, AppendEntryResponse, GetEntryRequest,
    GetNodeStateRequest, GetOverlayRequest, ListEntriesRequest, NodeStateResponse, OverlayResponse,
};

use crate::boot_service::ConfigUpdateNotifier;
use crate::raft::types::{JournalCommand, JournalResponse, JournalTypeConfig};
use crate::JournalState;

/// gRPC ConfigService backed by Raft consensus for writes, local state for reads.
pub struct ConfigServiceImpl {
    raft: Raft<JournalTypeConfig>,
    state: Arc<RwLock<JournalState>>,
    notifier: ConfigUpdateNotifier,
}

impl ConfigServiceImpl {
    pub fn new(
        raft: Raft<JournalTypeConfig>,
        state: Arc<RwLock<JournalState>>,
        notifier: ConfigUpdateNotifier,
    ) -> Self {
        Self { raft, state, notifier }
    }
}

#[tonic::async_trait]
impl ConfigService for ConfigServiceImpl {
    /// Write a config entry through Raft consensus (J7).
    async fn append_entry(
        &self,
        request: Request<AppendEntryRequest>,
    ) -> Result<Response<AppendEntryResponse>, Status> {
        let req = request.into_inner();
        let proto_entry = req.entry.ok_or_else(|| Status::invalid_argument("entry required"))?;

        // Convert proto entry to domain type
        let entry = proto_to_config_entry(proto_entry)?;

        // Write through Raft
        let cmd = JournalCommand::AppendEntry(entry);
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::EntryAppended { sequence } => {
                // Notify subscribers of the new entry
                let state = self.state.read().await;
                if let Some(entry) = state.entries.get(&sequence) {
                    let serialized = serde_json::to_vec(entry).unwrap_or_default();
                    let update = pact_common::proto::stream::ConfigUpdate {
                        sequence,
                        timestamp: Some(prost_types::Timestamp {
                            seconds: entry.timestamp.timestamp(),
                            nanos: entry.timestamp.timestamp_subsec_nanos() as i32,
                        }),
                        update: Some(
                            pact_common::proto::stream::config_update::Update::VclusterChange(
                                serialized,
                            ),
                        ),
                    };
                    drop(state);
                    self.notifier.notify(update);
                }
                Ok(Response::new(AppendEntryResponse { sequence }))
            }
            JournalResponse::ValidationError { reason } => Err(Status::failed_precondition(reason)),
            JournalResponse::Ok => Err(Status::internal("unexpected Ok for AppendEntry")),
        }
    }

    /// Read a single entry from local state (J8).
    async fn get_entry(
        &self,
        request: Request<GetEntryRequest>,
    ) -> Result<Response<ProtoConfigEntry>, Status> {
        let seq = request.into_inner().sequence;
        let state = self.state.read().await;
        let entry = state
            .entries
            .get(&seq)
            .ok_or_else(|| Status::not_found(format!("entry {seq} not found")))?;
        Ok(Response::new(config_entry_to_proto(entry)))
    }

    /// Read a node's config state from local state (J8).
    async fn get_node_state(
        &self,
        request: Request<GetNodeStateRequest>,
    ) -> Result<Response<NodeStateResponse>, Status> {
        let node_id = request.into_inner().node_id;
        let state = self.state.read().await;
        let config_state = state
            .node_states
            .get(&node_id)
            .ok_or_else(|| Status::not_found(format!("node {node_id} not found")))?;
        Ok(Response::new(NodeStateResponse { node_id, config_state: format!("{config_state:?}") }))
    }

    type ListEntriesStream = ReceiverStream<Result<ProtoConfigEntry, Status>>;

    /// Stream entries filtered by scope and range from local state (J8).
    async fn list_entries(
        &self,
        request: Request<ListEntriesRequest>,
    ) -> Result<Response<Self::ListEntriesStream>, Status> {
        let req = request.into_inner();
        let from = req.from_sequence.unwrap_or(0);
        let to = req.to_sequence.unwrap_or(u64::MAX);
        let limit = req.limit.unwrap_or(u32::MAX) as usize;

        // Parse optional scope filter
        let scope_filter = req.scope.and_then(|s| s.scope).map(|s| match s {
            pact_common::proto::config::scope::Scope::NodeId(n) => {
                pact_common::types::Scope::Node(n)
            }
            pact_common::proto::config::scope::Scope::VclusterId(v) => {
                pact_common::types::Scope::VCluster(v)
            }
            pact_common::proto::config::scope::Scope::Global(_) => {
                pact_common::types::Scope::Global
            }
        });

        let state = self.state.read().await;

        // Collect matching entries from BTreeMap range, filtered by scope
        let entries: Vec<ProtoConfigEntry> = state
            .entries
            .range(from..=to)
            .filter(|(_, e)| scope_filter.as_ref().is_none_or(|filter| e.scope == *filter))
            .take(limit)
            .map(|(_, e)| config_entry_to_proto(e))
            .collect();

        let (tx, rx) = tokio::sync::mpsc::channel(entries.len().max(1));
        tokio::spawn(async move {
            for entry in entries {
                if tx.send(Ok(entry)).await.is_err() {
                    break; // client disconnected
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    /// Read a cached boot overlay from local state (J8).
    async fn get_overlay(
        &self,
        request: Request<GetOverlayRequest>,
    ) -> Result<Response<OverlayResponse>, Status> {
        let vcluster_id = request.into_inner().vcluster_id;
        let state = self.state.read().await;
        let overlay = state
            .overlays
            .get(&vcluster_id)
            .ok_or_else(|| Status::not_found(format!("overlay for {vcluster_id} not found")))?;
        Ok(Response::new(OverlayResponse {
            vcluster_id,
            version: overlay.version,
            data: overlay.data.clone(),
            checksum: overlay.checksum.clone(),
        }))
    }
}

/// Convert a proto ConfigEntry to the domain ConfigEntry type.
fn proto_to_config_entry(
    proto: ProtoConfigEntry,
) -> Result<pact_common::types::ConfigEntry, Status> {
    use pact_common::types::{EntryType, Identity, PrincipalType, Scope};

    let author = proto.author.ok_or_else(|| Status::invalid_argument("author required"))?;

    let scope = proto.scope.and_then(|s| s.scope).map_or(Scope::Global, |s| match s {
        pact_common::proto::config::scope::Scope::NodeId(n) => Scope::Node(n),
        pact_common::proto::config::scope::Scope::VclusterId(v) => Scope::VCluster(v),
        pact_common::proto::config::scope::Scope::Global(_) => Scope::Global,
    });

    let entry_type = match proto.entry_type {
        1 => EntryType::Commit,
        2 => EntryType::Rollback,
        3 => EntryType::AutoConverge,
        4 => EntryType::DriftDetected,
        5 => EntryType::CapabilityChange,
        6 => EntryType::PolicyUpdate,
        7 => EntryType::BootConfig,
        8 => EntryType::EmergencyStart,
        9 => EntryType::EmergencyEnd,
        10 => EntryType::ExecLog,
        11 => EntryType::ShellSession,
        12 => EntryType::ServiceLifecycle,
        13 => EntryType::PendingApproval,
        _ => return Err(Status::invalid_argument("unknown entry type")),
    };

    let ttl_seconds = proto.ttl.map(|d| d.seconds as u32);

    let principal_type = match author.principal_type.as_str() {
        "agent" | "Agent" => PrincipalType::Agent,
        "service" | "Service" => PrincipalType::Service,
        _ => PrincipalType::Human,
    };

    let state_delta = proto.state_delta.map(proto_to_state_delta);

    Ok(pact_common::types::ConfigEntry {
        sequence: proto.sequence,
        timestamp: chrono::Utc::now(), // Server assigns timestamp
        entry_type,
        scope,
        author: Identity { principal: author.principal, principal_type, role: author.role },
        parent: proto.parent,
        state_delta,
        policy_ref: if proto.policy_ref.is_empty() { None } else { Some(proto.policy_ref) },
        ttl_seconds,
        emergency_reason: proto.emergency_reason,
    })
}

/// Convert proto StateDelta to domain StateDelta.
fn proto_to_state_delta(
    proto: pact_common::proto::config::StateDelta,
) -> pact_common::types::StateDelta {
    use pact_common::types::{DeltaAction, DeltaItem, StateDelta};

    fn proto_action(a: i32) -> DeltaAction {
        match a {
            1 => DeltaAction::Add,
            2 => DeltaAction::Remove,
            3 => DeltaAction::Modify,
            _ => DeltaAction::Modify, // UNSPECIFIED → Modify as safe default
        }
    }

    StateDelta {
        mounts: proto
            .mounts
            .into_iter()
            .map(|m| DeltaItem {
                action: proto_action(m.action),
                key: m.path,
                value: m.declared.map(|s| format!("{}:{} ({})", s.fs_type, s.source, s.options)),
                previous: m.actual.map(|s| format!("{}:{} ({})", s.fs_type, s.source, s.options)),
            })
            .collect(),
        files: proto
            .files
            .into_iter()
            .map(|f| DeltaItem {
                action: proto_action(f.action),
                key: f.path,
                value: f.content_hash,
                previous: f.owner,
            })
            .collect(),
        network: proto
            .network
            .into_iter()
            .map(|n| DeltaItem {
                action: proto_action(n.action),
                key: n.interface,
                value: n.detail,
                previous: None,
            })
            .collect(),
        services: proto
            .services
            .into_iter()
            .map(|s| DeltaItem {
                action: proto_action(s.action),
                key: s.name,
                value: s.declared_state,
                previous: s.actual_state,
            })
            .collect(),
        kernel: proto
            .kernel
            .into_iter()
            .map(|k| DeltaItem {
                action: proto_action(k.action),
                key: k.key,
                value: k.declared_value,
                previous: k.actual_value,
            })
            .collect(),
        packages: proto
            .packages
            .into_iter()
            .map(|p| DeltaItem {
                action: proto_action(p.action),
                key: p.name,
                value: p.version,
                previous: None,
            })
            .collect(),
        gpu: proto
            .gpu
            .into_iter()
            .map(|g| DeltaItem {
                action: proto_action(g.action),
                key: g.gpu_index.to_string(),
                value: g.detail,
                previous: None,
            })
            .collect(),
    }
}

/// Convert a domain ConfigEntry to the proto ConfigEntry type.
pub fn config_entry_to_proto(entry: &pact_common::types::ConfigEntry) -> ProtoConfigEntry {
    use pact_common::proto::config::{Identity as ProtoIdentity, Scope as ProtoScope};

    let entry_type = match entry.entry_type {
        pact_common::types::EntryType::Commit => 1,
        pact_common::types::EntryType::Rollback => 2,
        pact_common::types::EntryType::AutoConverge => 3,
        pact_common::types::EntryType::DriftDetected => 4,
        pact_common::types::EntryType::CapabilityChange => 5,
        pact_common::types::EntryType::PolicyUpdate => 6,
        pact_common::types::EntryType::BootConfig => 7,
        pact_common::types::EntryType::EmergencyStart => 8,
        pact_common::types::EntryType::EmergencyEnd => 9,
        pact_common::types::EntryType::ExecLog => 10,
        pact_common::types::EntryType::ShellSession => 11,
        pact_common::types::EntryType::ServiceLifecycle => 12,
        pact_common::types::EntryType::PendingApproval => 13,
    };

    let scope = match &entry.scope {
        pact_common::types::Scope::Global => {
            Some(ProtoScope { scope: Some(pact_common::proto::config::scope::Scope::Global(true)) })
        }
        pact_common::types::Scope::VCluster(v) => Some(ProtoScope {
            scope: Some(pact_common::proto::config::scope::Scope::VclusterId(v.clone())),
        }),
        pact_common::types::Scope::Node(n) => Some(ProtoScope {
            scope: Some(pact_common::proto::config::scope::Scope::NodeId(n.clone())),
        }),
    };

    let ttl = entry.ttl_seconds.map(|s| prost_types::Duration { seconds: i64::from(s), nanos: 0 });

    let principal_type = match entry.author.principal_type {
        pact_common::types::PrincipalType::Human => "Human",
        pact_common::types::PrincipalType::Agent => "Agent",
        pact_common::types::PrincipalType::Service => "Service",
    };

    let state_delta = entry.state_delta.as_ref().map(state_delta_to_proto);

    ProtoConfigEntry {
        sequence: entry.sequence,
        timestamp: Some(prost_types::Timestamp {
            seconds: entry.timestamp.timestamp(),
            nanos: entry.timestamp.timestamp_subsec_nanos() as i32,
        }),
        entry_type,
        scope,
        author: Some(ProtoIdentity {
            principal: entry.author.principal.clone(),
            principal_type: principal_type.to_string(),
            role: entry.author.role.clone(),
        }),
        parent: entry.parent,
        state_delta,
        policy_ref: entry.policy_ref.clone().unwrap_or_default(),
        ttl,
        emergency_reason: entry.emergency_reason.clone(),
    }
}

/// Convert domain StateDelta to proto StateDelta.
pub fn state_delta_to_proto(
    delta: &pact_common::types::StateDelta,
) -> pact_common::proto::config::StateDelta {
    use pact_common::proto::config::{
        FileDelta, GpuDelta, KernelDelta, MountDelta, NetworkDelta, PackageDelta, ServiceDelta,
    };
    use pact_common::types::DeltaAction;

    fn domain_action(a: &DeltaAction) -> i32 {
        match a {
            DeltaAction::Add => 1,
            DeltaAction::Remove => 2,
            DeltaAction::Modify => 3,
        }
    }

    pact_common::proto::config::StateDelta {
        mounts: delta
            .mounts
            .iter()
            .map(|m| MountDelta {
                path: m.key.clone(),
                action: domain_action(&m.action),
                declared: None, // DeltaItem uses flat key/value; mount specs not round-trippable
                actual: None,
            })
            .collect(),
        files: delta
            .files
            .iter()
            .map(|f| FileDelta {
                path: f.key.clone(),
                action: domain_action(&f.action),
                content_hash: f.value.clone(),
                mode: None,
                owner: f.previous.clone(),
            })
            .collect(),
        network: delta
            .network
            .iter()
            .map(|n| NetworkDelta {
                interface: n.key.clone(),
                action: domain_action(&n.action),
                detail: n.value.clone(),
            })
            .collect(),
        services: delta
            .services
            .iter()
            .map(|s| ServiceDelta {
                name: s.key.clone(),
                action: domain_action(&s.action),
                declared_state: s.value.clone(),
                actual_state: s.previous.clone(),
            })
            .collect(),
        kernel: delta
            .kernel
            .iter()
            .map(|k| KernelDelta {
                key: k.key.clone(),
                action: domain_action(&k.action),
                declared_value: k.value.clone(),
                actual_value: k.previous.clone(),
            })
            .collect(),
        packages: delta
            .packages
            .iter()
            .map(|p| PackageDelta {
                name: p.key.clone(),
                action: domain_action(&p.action),
                version: p.value.clone(),
            })
            .collect(),
        gpu: delta
            .gpu
            .iter()
            .map(|g| GpuDelta {
                gpu_index: g.key.parse().unwrap_or(0),
                action: domain_action(&g.action),
                detail: g.value.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use pact_common::types::{BootOverlay, ConfigState, EntryType, Identity, PrincipalType, Scope};
    use raft_hpc_core::{FileLogStore, GrpcNetworkFactory, HpcStateMachine, StateMachineState};
    use tokio_stream::StreamExt;

    fn test_identity() -> Identity {
        Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        }
    }

    fn test_entry(seq: u64, entry_type: EntryType) -> pact_common::types::ConfigEntry {
        pact_common::types::ConfigEntry {
            sequence: seq,
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

    fn populated_state() -> JournalState {
        let mut state = JournalState::default();
        // Add some entries
        state.apply(JournalCommand::AppendEntry(test_entry(0, EntryType::Commit)));
        state.apply(JournalCommand::AppendEntry(test_entry(0, EntryType::Rollback)));
        state.apply(JournalCommand::AppendEntry(test_entry(0, EntryType::DriftDetected)));
        // Add node state
        state.apply(JournalCommand::UpdateNodeState {
            node_id: "node-001".into(),
            state: ConfigState::Committed,
        });
        // Add overlay
        state.apply(JournalCommand::SetOverlay {
            vcluster_id: "ml-training".into(),
            overlay: BootOverlay {
                vcluster_id: "ml-training".into(),
                version: 3,
                data: vec![1, 2, 3, 4],
                checksum: "abc123".into(),
            },
        });
        state
    }

    // --- Proto conversion tests ---

    #[test]
    fn config_entry_roundtrip() {
        let entry = test_entry(42, EntryType::Commit);
        let proto = config_entry_to_proto(&entry);
        assert_eq!(proto.sequence, 42);
        assert_eq!(proto.entry_type, 1); // Commit
        assert!(proto.author.is_some());
        assert_eq!(proto.author.as_ref().unwrap().principal, "admin@example.com");
    }

    #[test]
    fn proto_to_domain_valid_entry() {
        let proto = ProtoConfigEntry {
            sequence: 0,
            timestamp: None,
            entry_type: 1, // Commit
            scope: Some(pact_common::proto::config::Scope {
                scope: Some(pact_common::proto::config::scope::Scope::VclusterId(
                    "ml-training".into(),
                )),
            }),
            author: Some(pact_common::proto::config::Identity {
                principal: "alice@example.com".into(),
                principal_type: "admin".into(),
                role: "pact-ops-ml-training".into(),
            }),
            parent: None,
            state_delta: None,
            policy_ref: String::new(),
            ttl: None,
            emergency_reason: None,
        };
        let entry = proto_to_config_entry(proto).unwrap();
        assert_eq!(entry.author.principal, "alice@example.com");
        assert!(matches!(entry.scope, Scope::VCluster(ref v) if v == "ml-training"));
        assert!(matches!(entry.entry_type, EntryType::Commit));
    }

    #[test]
    fn proto_to_domain_rejects_missing_author() {
        let proto = ProtoConfigEntry {
            sequence: 0,
            timestamp: None,
            entry_type: 1,
            scope: None,
            author: None,
            parent: None,
            state_delta: None,
            policy_ref: String::new(),
            ttl: None,
            emergency_reason: None,
        };
        let result = proto_to_config_entry(proto);
        assert!(result.is_err());
    }

    #[test]
    fn state_delta_roundtrip() {
        use pact_common::types::{DeltaAction, DeltaItem, StateDelta};

        let delta = StateDelta {
            mounts: vec![DeltaItem {
                action: DeltaAction::Add,
                key: "/data/scratch".into(),
                value: Some("tmpfs:none (size=1G)".into()),
                previous: None,
            }],
            files: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "/etc/ntp.conf".into(),
                value: Some("abc123".into()),
                previous: Some("root".into()),
            }],
            network: vec![],
            services: vec![DeltaItem {
                action: DeltaAction::Add,
                key: "nvidia-persistenced".into(),
                value: Some("running".into()),
                previous: None,
            }],
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.swappiness".into(),
                value: Some("10".into()),
                previous: Some("60".into()),
            }],
            packages: vec![],
            gpu: vec![DeltaItem {
                action: DeltaAction::Add,
                key: "0".into(),
                value: Some("GH200 healthy".into()),
                previous: None,
            }],
        };

        let mut entry = test_entry(0, EntryType::Commit);
        entry.state_delta = Some(delta);

        // Domain → proto
        let proto = config_entry_to_proto(&entry);
        assert!(proto.state_delta.is_some());
        let proto_delta = proto.state_delta.as_ref().unwrap();
        assert_eq!(proto_delta.mounts.len(), 1);
        assert_eq!(proto_delta.mounts[0].path, "/data/scratch");
        assert_eq!(proto_delta.mounts[0].action, 1); // Add
        assert_eq!(proto_delta.files.len(), 1);
        assert_eq!(proto_delta.services.len(), 1);
        assert_eq!(proto_delta.kernel.len(), 1);
        assert_eq!(proto_delta.gpu.len(), 1);
        assert_eq!(proto_delta.gpu[0].gpu_index, 0);

        // Proto → domain (via proto_to_config_entry)
        let back = proto_to_config_entry(proto).unwrap();
        let back_delta = back.state_delta.unwrap();
        assert_eq!(back_delta.mounts.len(), 1);
        assert_eq!(back_delta.mounts[0].key, "/data/scratch");
        assert!(matches!(back_delta.mounts[0].action, DeltaAction::Add));
        assert_eq!(back_delta.services.len(), 1);
        assert_eq!(back_delta.services[0].key, "nvidia-persistenced");
        assert_eq!(back_delta.kernel.len(), 1);
        assert_eq!(back_delta.kernel[0].value, Some("10".into()));
        assert_eq!(back_delta.kernel[0].previous, Some("60".into()));
        assert_eq!(back_delta.gpu.len(), 1);
        assert_eq!(back_delta.gpu[0].key, "0");
    }

    #[test]
    fn principal_type_conversion() {
        let proto = ProtoConfigEntry {
            sequence: 0,
            timestamp: None,
            entry_type: 1,
            scope: None,
            author: Some(pact_common::proto::config::Identity {
                principal: "mcp-agent@pact".into(),
                principal_type: "Agent".into(),
                role: "pact-service-ai".into(),
            }),
            parent: None,
            state_delta: None,
            policy_ref: String::new(),
            ttl: None,
            emergency_reason: None,
        };
        let entry = proto_to_config_entry(proto).unwrap();
        assert!(matches!(entry.author.principal_type, PrincipalType::Agent));

        // Roundtrip: domain → proto should preserve "Agent"
        let back = config_entry_to_proto(&entry);
        assert_eq!(back.author.unwrap().principal_type, "Agent");
    }

    #[test]
    fn proto_ttl_conversion() {
        let mut entry = test_entry(0, EntryType::Commit);
        entry.ttl_seconds = Some(3600);
        let proto = config_entry_to_proto(&entry);
        assert!(proto.ttl.is_some());
        assert_eq!(proto.ttl.unwrap().seconds, 3600);
    }

    // --- Read-path tests via ConfigService trait methods ---

    /// Create a ConfigServiceImpl backed by a real single-node Raft and pre-populated state.
    async fn test_service() -> (ConfigServiceImpl, tempfile::TempDir) {
        let state = Arc::new(RwLock::new(populated_state()));
        let temp = tempfile::tempdir().unwrap();
        let config = Arc::new(
            openraft::Config {
                heartbeat_interval: 500,
                election_timeout_min: 1500,
                election_timeout_max: 3000,
                ..Default::default()
            }
            .validate()
            .unwrap(),
        );
        let log_store = FileLogStore::<JournalTypeConfig>::new(temp.path()).unwrap();
        let snapshot_dir = temp.path().join("snapshots");
        std::fs::create_dir_all(&snapshot_dir).unwrap();
        let sm = HpcStateMachine::with_snapshot_dir(Arc::clone(&state), snapshot_dir).unwrap();
        let network = GrpcNetworkFactory::new();
        let raft = Raft::new(1, config, network, log_store, sm).await.unwrap();
        let notifier = crate::boot_service::ConfigUpdateNotifier::default();
        let svc = ConfigServiceImpl::new(raft, state, notifier);
        (svc, temp)
    }

    #[tokio::test]
    async fn get_entry_returns_existing_entries() {
        let (svc, _tmp) = test_service().await;
        // Entry at sequence 0 should be Commit
        let resp = svc.get_entry(Request::new(GetEntryRequest { sequence: 0 })).await.unwrap();
        let entry = resp.into_inner();
        assert_eq!(entry.sequence, 0);
        assert_eq!(entry.entry_type, 1); // Commit
        assert_eq!(entry.author.unwrap().principal, "admin@example.com");

        // Entry at sequence 1 should be Rollback
        let resp = svc.get_entry(Request::new(GetEntryRequest { sequence: 1 })).await.unwrap();
        assert_eq!(resp.into_inner().entry_type, 2); // Rollback
    }

    #[tokio::test]
    async fn get_entry_not_found() {
        let (svc, _tmp) = test_service().await;
        let result = svc.get_entry(Request::new(GetEntryRequest { sequence: 999 })).await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn get_node_state_returns_existing() {
        let (svc, _tmp) = test_service().await;
        let resp = svc
            .get_node_state(Request::new(GetNodeStateRequest { node_id: "node-001".into() }))
            .await
            .unwrap();
        let ns = resp.into_inner();
        assert_eq!(ns.node_id, "node-001");
        assert!(ns.config_state.contains("Committed"));
    }

    #[tokio::test]
    async fn get_node_state_not_found() {
        let (svc, _tmp) = test_service().await;
        let result = svc
            .get_node_state(Request::new(GetNodeStateRequest { node_id: "nonexistent".into() }))
            .await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn get_overlay_returns_existing() {
        let (svc, _tmp) = test_service().await;
        let resp = svc
            .get_overlay(Request::new(GetOverlayRequest { vcluster_id: "ml-training".into() }))
            .await
            .unwrap();
        let overlay = resp.into_inner();
        assert_eq!(overlay.vcluster_id, "ml-training");
        assert_eq!(overlay.version, 3);
        assert_eq!(overlay.data, vec![1, 2, 3, 4]);
        assert_eq!(overlay.checksum, "abc123");
    }

    #[tokio::test]
    async fn get_overlay_not_found() {
        let (svc, _tmp) = test_service().await;
        let result = svc
            .get_overlay(Request::new(GetOverlayRequest { vcluster_id: "nonexistent".into() }))
            .await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn list_entries_streams_filtered_range() {
        let (svc, _tmp) = test_service().await;
        let resp = svc
            .list_entries(Request::new(ListEntriesRequest {
                scope: None,
                from_sequence: Some(1),
                to_sequence: Some(2),
                limit: None,
            }))
            .await
            .unwrap();
        let mut stream = resp.into_inner();
        let mut entries = vec![];
        while let Some(Ok(entry)) = stream.next().await {
            entries.push(entry);
        }
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);
    }

    #[tokio::test]
    async fn list_entries_respects_limit() {
        let (svc, _tmp) = test_service().await;
        let resp = svc
            .list_entries(Request::new(ListEntriesRequest {
                scope: None,
                from_sequence: None,
                to_sequence: None,
                limit: Some(2),
            }))
            .await
            .unwrap();
        let mut stream = resp.into_inner();
        let mut entries = vec![];
        while let Some(Ok(entry)) = stream.next().await {
            entries.push(entry);
        }
        assert_eq!(entries.len(), 2);
    }
}
