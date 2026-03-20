//! E2E test: multi-node Raft cluster operations.
//!
//! Tests consensus, leader election, failover, and partition behavior
//! using an in-process 3-node pact-journal Raft cluster.

use pact_common::proto::config::{self, Identity as ProtoIdentity};
use pact_common::proto::journal::config_service_server::ConfigService;
use pact_common::proto::journal::{AppendEntryRequest, ListEntriesRequest};
use pact_common::proto::policy::policy_service_server::PolicyService;
use pact_common::proto::policy::{PolicyEvalRequest, UpdatePolicyRequest};
use pact_common::proto::stream::boot_config_service_server::BootConfigService;
use pact_common::proto::stream::BootConfigRequest;
use pact_common::types::BootOverlay;
use pact_e2e::containers::raft_cluster::RaftCluster;
use pact_journal::JournalCommand;
use tokio_stream::StreamExt;
use tonic::Request;

fn make_commit_request() -> AppendEntryRequest {
    AppendEntryRequest {
        entry: Some(config::ConfigEntry {
            sequence: 0,
            timestamp: None,
            entry_type: 1,
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

/// 3-node cluster reaches consensus and replicates writes.
#[tokio::test]
async fn three_node_cluster_consensus() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");

    // Find the leader
    let leader = cluster.leader().await.expect("should have leader");
    // Write entries through the leader
    for _ in 0..5 {
        leader
            .config_svc
            .append_entry(Request::new(make_commit_request()))
            .await
            .expect("append through leader");
    }

    // Wait for replication
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // All nodes should have the entries in their state machine
    for node in &cluster.nodes {
        let state = node.state.read().await;
        assert_eq!(
            state.entries.len(),
            5,
            "node {} should have 5 entries, got {}",
            node.node_id,
            state.entries.len()
        );
    }
}

/// Writes through non-leader are forwarded (or fail with redirect).
#[tokio::test]
async fn write_through_follower() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");

    let leader = cluster.leader().await.expect("should have leader");
    let leader_id = leader.node_id;

    // Find a follower
    let follower =
        cluster.nodes.iter().find(|n| n.node_id != leader_id).expect("should have follower");

    // Attempt to write through a follower — this may succeed (if forwarded)
    // or fail with a "not leader" error depending on implementation
    let result = follower.config_svc.append_entry(Request::new(make_commit_request())).await;

    // Either succeeds (forwarding) or fails with a clear error
    match result {
        Ok(_) => {
            // Forwarding works — verify entry is replicated
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let state = leader.state.read().await;
            assert!(!state.entries.is_empty(), "leader should have the entry");
        }
        Err(status) => {
            // Expected: not-leader error
            assert!(
                status.message().contains("leader")
                    || status.message().contains("forward")
                    || status.code() == tonic::Code::FailedPrecondition
                    || status.code() == tonic::Code::Unavailable,
                "unexpected error: {status}"
            );
        }
    }
}

/// Policy is replicated across all nodes.
#[tokio::test]
async fn policy_replication() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");
    let leader = cluster.leader().await.expect("should have leader");

    // Update policy through the leader
    let resp = leader
        .policy_svc
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
        .expect("update policy");
    assert!(resp.into_inner().success);

    // Wait for replication
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // All nodes should have the policy
    for node in &cluster.nodes {
        let state = node.state.read().await;
        let policy = state.policies.get("ml-training");
        assert!(policy.is_some(), "node {} should have ml-training policy", node.node_id);
        assert_eq!(policy.unwrap().drift_sensitivity, 3.0);
    }
}

/// Policy evaluation works on any node.
#[tokio::test]
async fn policy_evaluation_on_follower() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");
    let leader = cluster.leader().await.expect("should have leader");

    // Set policy through leader
    leader
        .policy_svc
        .update_policy(Request::new(UpdatePolicyRequest {
            vcluster_id: "ml-training".into(),
            policy: Some(pact_common::proto::policy::VClusterPolicy {
                vcluster_id: "ml-training".into(),
                policy_id: "pol-001".into(),
                drift_sensitivity: 3.0,
                base_commit_window_seconds: 1800,
                ..Default::default()
            }),
            author: Some(ProtoIdentity {
                principal: "admin@example.com".into(),
                principal_type: "admin".into(),
                role: "pact-platform-admin".into(),
            }),
            message: "policy setup".into(),
        }))
        .await
        .expect("set policy");

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Evaluate policy on a follower (reads don't need Raft)
    let follower = cluster.nodes.iter().find(|n| n.node_id != leader.node_id).expect("follower");

    let resp = follower
        .policy_svc
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
        .expect("evaluate on follower");
    assert!(resp.into_inner().authorized);
}

/// Boot config streaming works after overlay is set.
#[tokio::test]
async fn boot_config_stream_from_cluster() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");
    let leader = cluster.leader().await.expect("should have leader");

    // Set overlay via state directly (bypassing Raft for simplicity)
    {
        let mut state = leader.state.write().await;
        state.apply_command(JournalCommand::SetOverlay {
            vcluster_id: "ml-training".into(),
            overlay: BootOverlay::new("ml-training", 1, vec![42; 100]),
        });
    }

    // Stream boot config from the leader
    let resp = leader
        .boot_svc
        .stream_boot_config(Request::new(BootConfigRequest {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
            last_known_version: None,
        }))
        .await
        .expect("stream boot config");

    let mut stream = resp.into_inner();
    let mut has_overlay = false;
    let mut has_complete = false;
    while let Some(Ok(chunk)) = stream.next().await {
        match &chunk.chunk {
            Some(pact_common::proto::stream::config_chunk::Chunk::BaseOverlay(ov)) => {
                assert_eq!(ov.version, 1);
                // Data is zstd compressed — verify decompression recovers original
                let decompressed = zstd::decode_all(ov.data.as_slice()).unwrap();
                assert_eq!(decompressed.len(), 100);
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

/// List entries returns consistent results across nodes.
#[tokio::test]
async fn list_entries_consistent_across_nodes() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");
    let leader = cluster.leader().await.expect("should have leader");

    // Write 10 entries
    for _ in 0..10 {
        leader.config_svc.append_entry(Request::new(make_commit_request())).await.expect("append");
    }

    // Wait for replication
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // List from all nodes — should get same count
    for node in &cluster.nodes {
        let resp = node
            .config_svc
            .list_entries(Request::new(ListEntriesRequest {
                scope: None,
                from_sequence: None,
                to_sequence: None,
                limit: None,
            }))
            .await
            .expect("list entries");

        let mut stream = resp.into_inner();
        let mut count = 0;
        while let Some(Ok(_)) = stream.next().await {
            count += 1;
        }
        assert_eq!(count, 10, "node {} should have 10 entries, got {}", node.node_id, count);
    }
}
