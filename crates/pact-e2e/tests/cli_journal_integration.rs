//! E2E test: CLI execute functions against a real Raft journal.
//!
//! Bootstraps a single-node Raft cluster with gRPC server, then calls
//! the CLI execute functions over tonic gRPC — the same path a real
//! `pact` CLI invocation would take.

use pact_cli::commands::execute;
use pact_common::proto::journal::config_service_client::ConfigServiceClient;
use pact_common::proto::policy::policy_service_client::PolicyServiceClient;
use pact_e2e::containers::raft_cluster::RaftCluster;
use tonic::transport::Channel;

/// Connect to the leader's gRPC address.
async fn connect_to_leader(cluster: &RaftCluster) -> Channel {
    let addr = cluster.leader_grpc_addr().await.expect("should have leader");
    let uri = format!("http://{addr}");
    Channel::from_shared(uri).unwrap().connect().await.unwrap()
}

/// Full flow: status → commit → log → rollback → verify log.
#[tokio::test]
async fn cli_commit_log_rollback_flow() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;
    let mut client = ConfigServiceClient::new(channel.clone());

    // 1. Status query — should get NotFound for unknown node
    let result = execute::status(&mut client, "node-042").await;
    assert!(result.is_err(), "unknown node should return error");

    // 2. Commit an entry
    let result = execute::commit(
        &mut client,
        "initial config for ml-training",
        "ml-training",
        "admin@example.com",
        "pact-platform-admin",
    )
    .await
    .unwrap();
    assert!(result.contains("Committed"), "should confirm commit: {result}");
    assert!(result.contains("seq:0"), "first entry should be seq 0: {result}");

    // 3. Commit another entry
    let result = execute::commit(
        &mut client,
        "add GPU monitoring",
        "ml-training",
        "admin@example.com",
        "pact-platform-admin",
    )
    .await
    .unwrap();
    assert!(result.contains("seq:1"), "second entry should be seq 1: {result}");

    // 4. Query log — should show both entries
    let log_output = execute::log(&mut client, 10, None).await.unwrap();
    assert!(log_output.contains("#0"), "log should contain entry 0: {log_output}");
    assert!(log_output.contains("#1"), "log should contain entry 1: {log_output}");
    assert!(log_output.contains("COMMIT"), "log should show COMMIT type: {log_output}");
    assert!(
        log_output.contains("admin@example.com"),
        "log should show author: {log_output}"
    );

    // 5. Rollback to seq 0
    let result = execute::rollback(
        &mut client,
        0,
        "ml-training",
        "admin@example.com",
        "pact-platform-admin",
    )
    .await
    .unwrap();
    assert!(result.contains("Rolled back"), "should confirm rollback: {result}");
    assert!(result.contains("seq:0"), "should reference target seq 0: {result}");

    // 6. Log should now have 3 entries (2 commits + 1 rollback)
    let log_output = execute::log(&mut client, 10, None).await.unwrap();
    assert!(log_output.contains("#2"), "should have rollback entry at seq 2: {log_output}");
    assert!(log_output.contains("ROLLBACK"), "should show ROLLBACK type: {log_output}");
}

/// Emergency start/end flow through CLI execute functions.
#[tokio::test]
async fn cli_emergency_flow() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;
    let mut client = ConfigServiceClient::new(channel.clone());

    // Start emergency
    let result = execute::emergency_start(
        &mut client,
        "GPU failure on node-042",
        "ml-training",
        "admin@example.com",
        "pact-platform-admin",
    )
    .await
    .unwrap();
    assert!(result.contains("ACTIVE"), "should confirm emergency active: {result}");
    assert!(
        result.contains("GPU failure"),
        "should include reason: {result}"
    );

    // End emergency
    let result = execute::emergency_end(
        &mut client,
        "ml-training",
        "admin@example.com",
        "pact-platform-admin",
    )
    .await
    .unwrap();
    assert!(result.contains("ENDED"), "should confirm emergency ended: {result}");

    // Log should show both entries
    let log_output = execute::log(&mut client, 10, None).await.unwrap();
    assert!(
        log_output.contains("EMERGENCY_ON"),
        "should show emergency start: {log_output}"
    );
    assert!(
        log_output.contains("EMERGENCY_OFF"),
        "should show emergency end: {log_output}"
    );
}

/// Approval list/decide flow through CLI execute functions.
#[tokio::test]
async fn cli_approval_flow() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;

    // Set up a regulated policy first
    let mut policy_client = PolicyServiceClient::new(channel.clone());
    policy_client
        .update_policy(tonic::Request::new(
            pact_common::proto::policy::UpdatePolicyRequest {
                vcluster_id: "sensitive-compute".into(),
                policy: Some(pact_common::proto::policy::VClusterPolicy {
                    vcluster_id: "sensitive-compute".into(),
                    policy_id: "pol-regulated".into(),
                    regulated: true,
                    two_person_approval: true,
                    ..Default::default()
                }),
                author: Some(pact_common::proto::config::Identity {
                    principal: "admin@example.com".into(),
                    principal_type: "admin".into(),
                    role: "pact-platform-admin".into(),
                }),
                message: "set up regulated policy".into(),
            },
        ))
        .await
        .expect("set policy");

    // Evaluate a regulated action — should get Defer (pending approval)
    let resp = policy_client
        .evaluate(tonic::Request::new(
            pact_common::proto::policy::PolicyEvalRequest {
                author: Some(pact_common::proto::config::Identity {
                    principal: "regulated-admin@example.com".into(),
                    principal_type: "admin".into(),
                    role: "pact-regulated-sensitive-compute".into(),
                }),
                scope: Some(pact_common::proto::config::Scope {
                    scope: Some(
                        pact_common::proto::config::scope::Scope::VclusterId(
                            "sensitive-compute".into(),
                        ),
                    ),
                }),
                action: "commit".into(),
                proposed_change: None,
                command: None,
            },
        ))
        .await
        .expect("evaluate");
    let eval = resp.into_inner();
    assert!(!eval.authorized, "regulated action should require approval");
    assert!(eval.approval.is_some(), "should have pending approval");
    let approval_id = eval.approval.unwrap().pending_approval_id;

    // List pending approvals
    let list_output = execute::approve_list(&channel, None).await.unwrap();
    assert!(
        list_output.contains(&approval_id[..10]),
        "approval list should include the pending approval: {list_output}"
    );

    // Approve it
    let result = execute::approve_decide(
        &channel,
        &approval_id,
        "approved",
        "approver@example.com",
        "pact-platform-admin",
        None,
    )
    .await
    .unwrap();
    assert!(result.contains("approved"), "should confirm approval: {result}");

    // Try to approve again — should fail (already decided)
    let result = execute::approve_decide(
        &channel,
        &approval_id,
        "approved",
        "second-approver@example.com",
        "pact-platform-admin",
        None,
    )
    .await;
    assert!(result.is_err(), "double approval should fail");
}

/// Log with scope filter returns only matching entries.
#[tokio::test]
async fn cli_log_scope_filter() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;
    let mut client = ConfigServiceClient::new(channel.clone());

    // Commit entries with different scopes (all go through journal, scope is metadata)
    execute::commit(
        &mut client,
        "global config",
        "ml-training",
        "admin@example.com",
        "pact-platform-admin",
    )
    .await
    .unwrap();

    execute::commit(
        &mut client,
        "dev config",
        "dev-sandbox",
        "admin@example.com",
        "pact-platform-admin",
    )
    .await
    .unwrap();

    // Query all — should get both
    let all = execute::log(&mut client, 10, None).await.unwrap();
    assert!(all.contains("#0"), "should have entry 0");
    assert!(all.contains("#1"), "should have entry 1");

    // Scope filtering: only ml-training entries
    let filtered = execute::log(&mut client, 10, Some("vc:ml-training")).await.unwrap();
    assert!(filtered.contains("#0"), "should include ml-training entry");
    assert!(
        !filtered.contains("vc:dev-sandbox"),
        "should NOT include dev-sandbox entry: {filtered}"
    );

    // Scope filtering: only dev-sandbox entries
    let filtered = execute::log(&mut client, 10, Some("vc:dev-sandbox")).await.unwrap();
    assert!(filtered.contains("#1"), "should include dev-sandbox entry");
    assert!(
        !filtered.contains("vc:ml-training"),
        "should NOT include ml-training entry: {filtered}"
    );
}

/// MCP connected dispatch against real journal.
#[tokio::test]
async fn mcp_connected_dispatch() {
    use pact_mcp::protocol::ToolCallResult;

    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let channel = connect_to_leader(&cluster).await;

    // Commit via MCP connected dispatch
    let result: Option<ToolCallResult> = pact_mcp::connected::dispatch_tool_connected(
        "pact_commit",
        &serde_json::json!({"message": "mcp-driven commit", "vcluster": "ml-training"}),
        &channel,
    )
    .await;
    let result = result.expect("pact_commit should be handled");
    assert!(!result.is_error, "commit should succeed: {:?}", result.content);

    // Log via MCP connected dispatch
    let result: Option<ToolCallResult> = pact_mcp::connected::dispatch_tool_connected(
        "pact_log",
        &serde_json::json!({"n": 10}),
        &channel,
    )
    .await;
    let result = result.expect("pact_log should be handled");
    assert!(!result.is_error, "log should succeed");
    let text = &result.content[0].text;
    assert!(text.contains("COMMIT"), "log should show commit: {text}");
    assert!(text.contains("mcp-agent"), "log should show mcp-agent author: {text}");

    // Emergency start — P8 should block
    let result: Option<ToolCallResult> = pact_mcp::connected::dispatch_tool_connected(
        "pact_emergency",
        &serde_json::json!({"action": "start"}),
        &channel,
    )
    .await;
    let result = result.expect("pact_emergency should be handled");
    assert!(result.is_error, "P8: AI agents cannot start emergency");

    // Unknown tool — should return None (not handled by connected dispatch)
    let result: Option<ToolCallResult> = pact_mcp::connected::dispatch_tool_connected(
        "pact_exec",
        &serde_json::json!({}),
        &channel,
    )
    .await;
    assert!(result.is_none(), "agent tools should fall through");
}
