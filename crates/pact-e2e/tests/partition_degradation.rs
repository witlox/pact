//! E2E tests: partition, degradation, and consistency scenarios.
//!
//! Tests Raft replication consistency, follower reads, error handling for
//! unreachable journals, policy and overlay CRUD, concurrent writes, and
//! TTL validation through the full gRPC stack.

use pact_cli::commands::config::CliConfig;
use pact_cli::commands::execute;
use pact_common::proto::config::{
    scope::Scope as ProtoScope, ConfigEntry as ProtoConfigEntry, Identity as ProtoIdentity,
    Scope as ProtoScopeMsg,
};
use pact_common::proto::journal::config_service_client::ConfigServiceClient;
use pact_common::proto::journal::{AppendEntryRequest, GetOverlayRequest};
use pact_common::proto::policy::policy_service_client::PolicyServiceClient;
use pact_common::types::BootOverlay;
use pact_e2e::containers::raft_cluster::RaftCluster;
use tonic::transport::Channel;

/// Connect to the leader's gRPC address.
async fn connect_to_leader(cluster: &RaftCluster) -> Channel {
    let addr = cluster.leader_grpc_addr().await.expect("should have leader");
    let uri = format!("http://{addr}");
    Channel::from_shared(uri).unwrap().connect().await.unwrap()
}

/// Build a commit entry proto for appending directly via ConfigServiceClient.
fn make_commit_entry(vcluster: &str, message: &str) -> ProtoConfigEntry {
    ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 1, // Commit
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
        author: Some(ProtoIdentity {
            principal: "test@example.com".into(),
            principal_type: "Human".into(),
            role: "pact-platform-admin".into(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: message.to_string(),
        ttl: None,
        emergency_reason: None,
    }
}

/// Build a commit entry with a specific TTL (in seconds).
fn make_commit_entry_with_ttl(vcluster: &str, message: &str, ttl_seconds: i64) -> ProtoConfigEntry {
    let mut entry = make_commit_entry(vcluster, message);
    entry.ttl = Some(prost_types::Duration { seconds: ttl_seconds, nanos: 0 });
    entry
}

// ---------------------------------------------------------------------------
// Test 1: three_node_replication_consistency
// ---------------------------------------------------------------------------

/// Bootstrap 3-node cluster, write 10 entries through leader, verify all
/// followers have replicated all 10 entries in their local state.
#[tokio::test]
async fn three_node_replication_consistency() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;
    let mut client = ConfigServiceClient::new(channel);

    // Write 10 entries through the leader
    for i in 0..10 {
        let entry = make_commit_entry("ml-training", &format!("entry-{i}"));
        client
            .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
            .await
            .unwrap_or_else(|e| panic!("append entry {i} failed: {e}"));
    }

    // Wait for replication to propagate to all followers
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Verify every node has all 10 entries in its local state
    for node in &cluster.nodes {
        let state = node.state.read().await;
        assert_eq!(
            state.entries.len(),
            10,
            "node {} should have 10 entries, has {}",
            node.node_id,
            state.entries.len()
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2: follower_serves_reads
// ---------------------------------------------------------------------------

/// Write entries through leader, then connect a new ConfigServiceClient to a
/// follower and verify entries are readable via list_entries.
#[tokio::test]
async fn follower_serves_reads() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");
    let leader_channel = connect_to_leader(&cluster).await;
    let mut leader_client = ConfigServiceClient::new(leader_channel);

    // Write entries through leader
    for i in 0..3 {
        let entry = make_commit_entry("ml-training", &format!("read-test-{i}"));
        leader_client
            .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
            .await
            .unwrap();
    }

    // Wait for replication
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Find a follower (not the leader)
    let leader = cluster.leader().await.expect("should have leader");
    let follower = cluster
        .nodes
        .iter()
        .find(|n| n.node_id != leader.node_id)
        .expect("should have at least one follower");

    // Connect a NEW client to the follower's gRPC address
    let follower_uri = format!("http://{}", follower.grpc_addr);
    let follower_channel = Channel::from_shared(follower_uri).unwrap().connect().await.unwrap();
    let mut follower_client = ConfigServiceClient::new(follower_channel);

    // List entries through the follower — reads should be served from local state
    let resp = follower_client
        .list_entries(tonic::Request::new(pact_common::proto::journal::ListEntriesRequest {
            scope: None,
            from_sequence: None,
            to_sequence: None,
            limit: Some(10),
        }))
        .await
        .unwrap();

    // Collect streamed entries
    let mut stream = resp.into_inner();
    let mut count = 0;
    while let Some(entry) = tokio_stream::StreamExt::next(&mut stream).await {
        entry.unwrap();
        count += 1;
    }
    assert_eq!(count, 3, "follower should serve all 3 entries, got {count}");
}

// ---------------------------------------------------------------------------
// Test 3: write_through_follower
// ---------------------------------------------------------------------------

/// Try to append an entry through a follower. The Raft layer should either
/// forward to the leader or return an error — either is acceptable.
#[tokio::test]
async fn write_through_follower() {
    let cluster = RaftCluster::bootstrap(3).await.expect("cluster started");

    // Find a follower
    let leader = cluster.leader().await.expect("should have leader");
    let follower = cluster
        .nodes
        .iter()
        .find(|n| n.node_id != leader.node_id)
        .expect("should have at least one follower");

    // Connect to the follower
    let follower_uri = format!("http://{}", follower.grpc_addr);
    let follower_channel = Channel::from_shared(follower_uri).unwrap().connect().await.unwrap();
    let mut follower_client = ConfigServiceClient::new(follower_channel);

    // Attempt a write through the follower
    let entry = make_commit_entry("ml-training", "follower-write-test");
    let result = follower_client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await;

    // Either the write succeeds (leader forwarding) or fails with a clear error.
    // Both are valid Raft behaviors — the key is no panic and no hang.
    match result {
        Ok(resp) => {
            // Forwarded to leader successfully
            let seq = resp.into_inner().sequence;
            assert!(seq < u64::MAX, "should get a valid sequence number: {seq}");
        }
        Err(status) => {
            // Rejected with an error — must be a meaningful gRPC error, not a panic
            let msg = status.message().to_lowercase();
            assert!(
                msg.contains("not leader")
                    || msg.contains("forward")
                    || msg.contains("raft")
                    || msg.contains("write"),
                "error should indicate leader/Raft issue, got: {msg}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Test 4: journal_unreachable_returns_error
// ---------------------------------------------------------------------------

/// Connect to a non-existent journal endpoint and verify the CLI returns a
/// connection error, not a panic.
#[tokio::test]
async fn journal_unreachable_returns_error() {
    let config = CliConfig {
        endpoint: "http://127.0.0.1:1".to_string(),
        token: None,
        token_path: std::path::PathBuf::from("/nonexistent/token"),
        default_vcluster: None,
        output_format: pact_cli::commands::config::OutputFormat::Text,
        timeout_seconds: 2,
    };

    let result = execute::connect(&config).await;
    assert!(result.is_err(), "connecting to nothing should fail");
    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("connect") || err_msg.contains("refused") || err_msg.contains("error"),
        "should be a connection error, got: {err_msg}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: policy_set_and_retrieve
// ---------------------------------------------------------------------------

/// Set a vCluster policy and read it back, verifying all fields match.
#[tokio::test]
async fn policy_set_and_retrieve() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;
    let mut policy_client = PolicyServiceClient::new(channel);

    // Set a policy with specific fields
    let policy = pact_common::proto::policy::VClusterPolicy {
        vcluster_id: "test-vc".into(),
        policy_id: "pol-test-001".into(),
        regulated: true,
        two_person_approval: true,
        drift_sensitivity: 0.75,
        base_commit_window_seconds: 600,
        emergency_window_seconds: 300,
        enforcement_mode: "strict".into(),
        audit_retention_days: 90,
        supervisor_backend: "pact".into(),
        emergency_allowed: true,
        auto_converge_categories: vec!["time-sync".into()],
        require_ack_categories: vec!["kernel".into(), "mounts".into()],
        exec_whitelist: vec!["ps".into(), "top".into()],
        shell_whitelist: vec!["admin@example.com".into()],
        ..Default::default()
    };

    policy_client
        .update_policy(tonic::Request::new(pact_common::proto::policy::UpdatePolicyRequest {
            vcluster_id: "test-vc".into(),
            policy: Some(policy.clone()),
            author: Some(pact_common::proto::config::Identity {
                principal: "admin@example.com".into(),
                principal_type: "admin".into(),
                role: "pact-platform-admin".into(),
            }),
            message: "set test policy".into(),
        }))
        .await
        .expect("set policy should succeed");

    // Read it back
    let resp = policy_client
        .get_effective_policy(tonic::Request::new(pact_common::proto::policy::GetPolicyRequest {
            vcluster_id: "test-vc".into(),
        }))
        .await
        .expect("get policy should succeed");

    let retrieved = resp.into_inner();
    assert_eq!(retrieved.vcluster_id, "test-vc");
    assert_eq!(retrieved.policy_id, "pol-test-001");
    assert!(retrieved.regulated, "regulated should be true");
    assert!(retrieved.two_person_approval, "two_person_approval should be true");
    assert!(
        (retrieved.drift_sensitivity - 0.75).abs() < f64::EPSILON,
        "drift_sensitivity should be 0.75"
    );
    assert_eq!(retrieved.base_commit_window_seconds, 600);
    assert_eq!(retrieved.emergency_window_seconds, 300);
    assert_eq!(retrieved.enforcement_mode, "strict");
    assert_eq!(retrieved.audit_retention_days, 90);
    assert_eq!(retrieved.supervisor_backend, "pact");
    assert!(retrieved.emergency_allowed);
    assert_eq!(retrieved.auto_converge_categories, vec!["time-sync"]);
    assert_eq!(retrieved.require_ack_categories, vec!["kernel", "mounts"]);
    assert_eq!(retrieved.exec_whitelist, vec!["ps", "top"]);
    assert_eq!(retrieved.shell_whitelist, vec!["admin@example.com"]);
}

// ---------------------------------------------------------------------------
// Test 6: overlay_set_and_retrieve
// ---------------------------------------------------------------------------

/// Set an overlay via the Raft state machine directly, then read it back
/// through the gRPC GetOverlay endpoint. Verify data matches.
#[tokio::test]
async fn overlay_set_and_retrieve() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");

    let overlay_data = vec![10, 20, 30, 40, 50];
    let overlay = BootOverlay::new("gpu-cluster", 7, overlay_data.clone());

    // Write overlay through Raft (using the node's raft handle directly, since
    // there is no gRPC endpoint for SetOverlay — it's an internal command)
    let leader = cluster.leader().await.expect("should have leader");
    let cmd = pact_journal::raft::types::JournalCommand::SetOverlay {
        vcluster_id: "gpu-cluster".into(),
        overlay,
    };
    let resp = leader.raft.client_write(cmd).await.expect("raft write should succeed");
    assert!(
        matches!(resp.data, pact_journal::raft::types::JournalResponse::Ok),
        "SetOverlay should return Ok, got: {:?}",
        resp.data
    );

    // Read back via gRPC
    let channel = connect_to_leader(&cluster).await;
    let mut client = ConfigServiceClient::new(channel);
    let resp = client
        .get_overlay(tonic::Request::new(GetOverlayRequest { vcluster_id: "gpu-cluster".into() }))
        .await
        .expect("get overlay should succeed");

    let overlay_resp = resp.into_inner();
    assert_eq!(overlay_resp.vcluster_id, "gpu-cluster");
    assert_eq!(overlay_resp.version, 7);
    assert_eq!(overlay_resp.data, overlay_data);
    let expected_checksum = pact_common::types::compute_overlay_checksum(&overlay_data);
    assert_eq!(overlay_resp.checksum, expected_checksum);
}

// ---------------------------------------------------------------------------
// Test 7: concurrent_writes
// ---------------------------------------------------------------------------

/// Spawn 10 concurrent tasks, each appending an entry. Verify all 10 entries
/// exist with unique sequence numbers.
#[tokio::test]
async fn concurrent_writes() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let leader_addr = cluster.leader_grpc_addr().await.expect("should have leader");
    let uri = format!("http://{leader_addr}");

    let mut handles = Vec::new();
    for i in 0..10 {
        let uri = uri.clone();
        let handle = tokio::spawn(async move {
            let channel = Channel::from_shared(uri).unwrap().connect().await.unwrap();
            let mut client = ConfigServiceClient::new(channel);
            let entry = make_commit_entry("ml-training", &format!("concurrent-{i}"));
            let resp = client
                .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
                .await
                .unwrap();
            resp.into_inner().sequence
        });
        handles.push(handle);
    }

    // Wait for all to complete and collect sequence numbers
    let mut sequences = Vec::new();
    for handle in handles {
        let seq = handle.await.expect("task should not panic");
        sequences.push(seq);
    }

    // All 10 should have completed
    assert_eq!(sequences.len(), 10, "should have 10 results");

    // All sequence numbers should be unique
    sequences.sort_unstable();
    sequences.dedup();
    assert_eq!(sequences.len(), 10, "all 10 sequences should be unique: {sequences:?}");

    // Verify via state
    let leader = cluster.leader().await.expect("should have leader");
    let state = leader.state.read().await;
    assert_eq!(state.entries.len(), 10, "state should have 10 entries");
}

// ---------------------------------------------------------------------------
// Test 8: ttl_validation_rejects_invalid
// ---------------------------------------------------------------------------

/// Verify TTL validation through the full gRPC stack:
/// - TTL=100 (below minimum 900) should be rejected
/// - TTL=1000000 (above maximum 864000) should be rejected
/// - TTL=3600 (within bounds) should be accepted
#[tokio::test]
async fn ttl_validation_rejects_invalid() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;
    let mut client = ConfigServiceClient::new(channel);

    // TTL too low (100s < 900s minimum)
    let entry = make_commit_entry_with_ttl("ml-training", "low-ttl", 100);
    let result =
        client.append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) })).await;
    assert!(result.is_err(), "TTL=100 should be rejected");
    let err_msg = result.unwrap_err().message().to_string();
    assert!(
        err_msg.contains("15 minutes") || err_msg.contains("900"),
        "error should mention minimum TTL: {err_msg}"
    );

    // TTL too high (1000000s > 864000s maximum)
    let entry = make_commit_entry_with_ttl("ml-training", "high-ttl", 1_000_000);
    let result =
        client.append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) })).await;
    assert!(result.is_err(), "TTL=1000000 should be rejected");
    let err_msg = result.unwrap_err().message().to_string();
    assert!(
        err_msg.contains("10 days") || err_msg.contains("864000"),
        "error should mention maximum TTL: {err_msg}"
    );

    // TTL within bounds (3600s = 1 hour)
    let entry = make_commit_entry_with_ttl("ml-training", "valid-ttl", 3600);
    let result =
        client.append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) })).await;
    assert!(result.is_ok(), "TTL=3600 should be accepted: {:?}", result.err());
    assert_eq!(result.unwrap().into_inner().sequence, 0, "first valid entry should be seq 0");
}
