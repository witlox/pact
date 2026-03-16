//! Integration test: single-node journal cluster.
//!
//! Bootstraps a single-node Raft cluster, writes entries through the
//! ConfigService gRPC interface, and reads them back. Verifies the full
//! Raft consensus → state machine → gRPC read path.

use std::sync::Arc;

use openraft::impls::BasicNode;
use openraft::Raft;
use pact_common::proto::config::{self, Identity as ProtoIdentity};
use pact_common::proto::journal::config_service_server::ConfigService;
use pact_common::proto::journal::{AppendEntryRequest, GetEntryRequest, ListEntriesRequest};
use pact_common::proto::policy::policy_service_server::PolicyService;
use pact_common::proto::policy::{GetPolicyRequest, PolicyEvalRequest, UpdatePolicyRequest};
use pact_common::proto::stream::boot_config_service_server::BootConfigService;
use pact_common::proto::stream::{BootConfigRequest, SubscribeRequest};
use pact_common::types::BootOverlay;
use raft_hpc_core::{FileLogStore, GrpcNetworkFactory, HpcStateMachine, StateMachineState};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tonic::Request;

use pact_journal::boot_service::{BootConfigServiceImpl, ConfigUpdateNotifier};
use pact_journal::policy_service::PolicyServiceImpl;
use pact_journal::raft::types::{JournalCommand, JournalTypeConfig};
use pact_journal::service::ConfigServiceImpl;
use pact_journal::JournalState;

/// Bootstrap a single-node Raft cluster and return the services.
async fn bootstrap_single_node() -> (
    ConfigServiceImpl,
    PolicyServiceImpl,
    BootConfigServiceImpl,
    Arc<RwLock<JournalState>>,
    tempfile::TempDir,
) {
    let state = Arc::new(RwLock::new(JournalState::default()));
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
    let raft: Raft<JournalTypeConfig> = Raft::new(1, config, network, log_store, sm).await.unwrap();

    // Bootstrap as single-node cluster
    let mut members = std::collections::BTreeMap::new();
    members.insert(1, BasicNode::new("127.0.0.1:19443".to_string()));
    raft.initialize(members).await.unwrap();

    // Wait for leader election
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let notifier = ConfigUpdateNotifier::default();
    let config_svc = ConfigServiceImpl::new(raft.clone(), Arc::clone(&state), notifier.clone());
    let policy_svc = PolicyServiceImpl::new(raft.clone(), Arc::clone(&state));
    let boot_svc = BootConfigServiceImpl::new(Arc::clone(&state), notifier);

    (config_svc, policy_svc, boot_svc, state, temp)
}

fn make_append_request(entry_type: i32) -> AppendEntryRequest {
    AppendEntryRequest {
        entry: Some(config::ConfigEntry {
            sequence: 0,
            timestamp: None,
            entry_type,
            scope: Some(config::Scope { scope: Some(config::scope::Scope::Global(true)) }),
            author: Some(ProtoIdentity {
                principal: "admin@example.com".into(),
                principal_type: "admin".into(),
                role: "pact-platform-admin".into(),
            }),
            parent: None,
            state_delta: None,
            policy_ref: String::new(),
            ttl: None,
            emergency_reason: None,
        }),
    }
}

#[tokio::test]
async fn write_and_read_back_through_raft() {
    let (config_svc, _, _, _, _tmp) = bootstrap_single_node().await;

    // Write a Commit entry through Raft
    let resp = config_svc
        .append_entry(Request::new(make_append_request(1))) // Commit
        .await
        .unwrap();
    assert_eq!(resp.into_inner().sequence, 0);

    // Write a Rollback entry
    let resp = config_svc
        .append_entry(Request::new(make_append_request(2))) // Rollback
        .await
        .unwrap();
    assert_eq!(resp.into_inner().sequence, 1);

    // Read back entry 0
    let resp = config_svc.get_entry(Request::new(GetEntryRequest { sequence: 0 })).await.unwrap();
    let entry = resp.into_inner();
    assert_eq!(entry.sequence, 0);
    assert_eq!(entry.entry_type, 1); // Commit
    assert_eq!(entry.author.unwrap().principal, "admin@example.com");

    // Read back entry 1
    let resp = config_svc.get_entry(Request::new(GetEntryRequest { sequence: 1 })).await.unwrap();
    assert_eq!(resp.into_inner().entry_type, 2); // Rollback
}

#[tokio::test]
async fn list_entries_after_raft_writes() {
    let (config_svc, _, _, _, _tmp) = bootstrap_single_node().await;

    // Write 3 entries
    for entry_type in [1, 2, 3] {
        // Commit, Rollback, AutoConverge
        config_svc.append_entry(Request::new(make_append_request(entry_type))).await.unwrap();
    }

    // List all entries
    let resp = config_svc
        .list_entries(Request::new(ListEntriesRequest {
            scope: None,
            from_sequence: None,
            to_sequence: None,
            limit: None,
        }))
        .await
        .unwrap();

    let mut stream = resp.into_inner();
    let mut entries = vec![];
    while let Some(Ok(entry)) = stream.next().await {
        entries.push(entry);
    }
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].entry_type, 1);
    assert_eq!(entries[1].entry_type, 2);
    assert_eq!(entries[2].entry_type, 3);
}

#[tokio::test]
async fn validation_rejects_bad_entries() {
    let (config_svc, _, _, _, _tmp) = bootstrap_single_node().await;

    // Missing author
    let bad_req = AppendEntryRequest {
        entry: Some(config::ConfigEntry {
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
        }),
    };
    let result = config_svc.append_entry(Request::new(bad_req)).await;
    assert!(result.is_err());

    // TTL below minimum (through Raft)
    let mut req = make_append_request(1);
    req.entry.as_mut().unwrap().ttl = Some(prost_types::Duration {
        seconds: 60, // 1 minute — below 15 min minimum
        nanos: 0,
    });
    let result = config_svc.append_entry(Request::new(req)).await;
    // ValidationError comes back as failed_precondition
    assert!(result.is_err());
}

#[tokio::test]
async fn policy_service_evaluate_and_update() {
    let (_, policy_svc, _, _state, _tmp) = bootstrap_single_node().await;

    // Admin should be authorized (P6: platform admin bypass)
    let resp = policy_svc
        .evaluate(Request::new(PolicyEvalRequest {
            author: Some(ProtoIdentity {
                principal: "admin@example.com".into(),
                principal_type: "admin".into(),
                role: "pact-platform-admin".into(),
            }),
            scope: Some(config::Scope { scope: Some(config::scope::Scope::Global(true)) }),
            action: "commit".into(),
            proposed_change: None,
            command: None,
        }))
        .await
        .unwrap();
    assert!(resp.into_inner().authorized);

    // Anonymous with no role should be denied
    let resp = policy_svc
        .evaluate(Request::new(PolicyEvalRequest {
            author: None,
            scope: None,
            action: "commit".into(),
            proposed_change: None,
            command: None,
        }))
        .await
        .unwrap();
    assert!(!resp.into_inner().authorized);

    // Update policy through Raft
    let resp = policy_svc
        .update_policy(Request::new(UpdatePolicyRequest {
            vcluster_id: "ml-training".into(),
            policy: Some(pact_common::proto::policy::VClusterPolicy {
                vcluster_id: "ml-training".into(),
                policy_id: "pol-001".into(),
                drift_sensitivity: 3.0,
                base_commit_window_seconds: 1800,
                regulated: true,
                ..Default::default()
            }),
            author: Some(ProtoIdentity {
                principal: "admin@example.com".into(),
                principal_type: "admin".into(),
                role: "pact-platform-admin".into(),
            }),
            message: "initial policy".into(),
        }))
        .await
        .unwrap();
    assert!(resp.into_inner().success);

    // Read back the policy
    let resp = policy_svc
        .get_effective_policy(Request::new(GetPolicyRequest { vcluster_id: "ml-training".into() }))
        .await
        .unwrap();
    let policy = resp.into_inner();
    assert_eq!(policy.vcluster_id, "ml-training");
    assert_eq!(policy.drift_sensitivity, 3.0);
    assert!(policy.regulated);
}

#[tokio::test]
async fn boot_config_stream_after_overlay_set() {
    let (_, _, boot_svc, state, _tmp) = bootstrap_single_node().await;

    // Set overlay via state directly (normally done through Raft SetOverlay command)
    {
        let mut s = state.write().await;
        s.apply(JournalCommand::SetOverlay {
            vcluster_id: "ml-training".into(),
            overlay: BootOverlay::new("ml-training", 1, vec![42; 50]),
        });
    }

    // Stream boot config
    let resp = boot_svc
        .stream_boot_config(Request::new(BootConfigRequest {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
            last_known_version: None,
        }))
        .await
        .unwrap();

    let mut stream = resp.into_inner();
    let mut has_overlay = false;
    let mut has_complete = false;
    while let Some(Ok(chunk)) = stream.next().await {
        match &chunk.chunk {
            Some(pact_common::proto::stream::config_chunk::Chunk::BaseOverlay(ov)) => {
                assert_eq!(ov.version, 1);
                assert_eq!(ov.data.len(), 50);
                has_overlay = true;
            }
            Some(pact_common::proto::stream::config_chunk::Chunk::Complete(c)) => {
                assert_eq!(c.base_version, 1);
                has_complete = true;
            }
            _ => {}
        }
    }
    assert!(has_overlay, "should receive overlay chunk");
    assert!(has_complete, "should receive complete marker");
}

#[tokio::test]
async fn subscribe_receives_raft_written_entries() {
    let (config_svc, _, boot_svc, _, _tmp) = bootstrap_single_node().await;

    // Write entries through Raft
    config_svc.append_entry(Request::new(make_append_request(1))).await.unwrap();
    config_svc
        .append_entry(Request::new(make_append_request(4))) // DriftDetected
        .await
        .unwrap();

    // Subscribe from sequence 0
    let resp = boot_svc
        .subscribe_config_updates(Request::new(SubscribeRequest {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
            from_sequence: 0,
        }))
        .await
        .unwrap();

    // Stream stays open for live push, so collect with a timeout
    let mut stream = resp.into_inner();
    let mut updates = vec![];
    while let Ok(Some(Ok(update))) =
        tokio::time::timeout(tokio::time::Duration::from_millis(200), stream.next()).await
    {
        updates.push(update);
    }
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].sequence, 0);
    assert_eq!(updates[1].sequence, 1);
}

#[tokio::test]
async fn subscribe_receives_live_updates() {
    let (config_svc, _, boot_svc, _, _tmp) = bootstrap_single_node().await;

    // Write one entry before subscribing
    config_svc.append_entry(Request::new(make_append_request(1))).await.unwrap();

    // Subscribe from sequence 0 — should get catch-up entry
    let resp = boot_svc
        .subscribe_config_updates(Request::new(SubscribeRequest {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
            from_sequence: 0,
        }))
        .await
        .unwrap();

    let mut stream = resp.into_inner();

    // Receive catch-up entry
    let catchup = tokio::time::timeout(tokio::time::Duration::from_secs(1), stream.next())
        .await
        .expect("timeout waiting for catch-up")
        .expect("stream ended")
        .expect("stream error");
    assert_eq!(catchup.sequence, 0);

    // Write another entry — should arrive as live push
    config_svc.append_entry(Request::new(make_append_request(2))).await.unwrap();

    let live = tokio::time::timeout(tokio::time::Duration::from_secs(1), stream.next())
        .await
        .expect("timeout waiting for live update")
        .expect("stream ended")
        .expect("stream error");
    assert_eq!(live.sequence, 1);
}
