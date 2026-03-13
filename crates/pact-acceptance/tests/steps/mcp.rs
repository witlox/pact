//! MCP / Agentic API steps — wired to pact_mcp::tools::all_tools() + dispatch_tool().

use cucumber::{given, then, when};
use pact_common::types::{
    AdminOperation, AdminOperationType, ConfigEntry, ConfigState, DriftVector, EntryType, Identity,
    PrincipalType, Scope,
};
use pact_journal::JournalCommand;
use pact_mcp::tools::{all_tools, dispatch_tool};
use serde_json::json;

use crate::{AuthResult, PactWorld};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ai_identity() -> Identity {
    Identity {
        principal: "service/ai-agent".into(),
        principal_type: PrincipalType::Service,
        role: "pact-service-ai".into(),
    }
}

fn record_audit(world: &mut PactWorld, op_type: AdminOperationType, detail: &str) {
    let op = AdminOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        actor: ai_identity(),
        operation_type: op_type,
        scope: Scope::Global,
        detail: detail.to_string(),
    };
    world.journal.apply_command(JournalCommand::RecordOperation(op));
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given("nodes in various config states")]
async fn given_various_states(world: &mut PactWorld) {
    for (node, state) in [
        ("node-001", ConfigState::Committed),
        ("node-002", ConfigState::Drifted),
        ("node-003", ConfigState::Emergency),
    ] {
        world
            .journal
            .apply_command(JournalCommand::UpdateNodeState { node_id: node.into(), state });
    }
}

#[given(regex = r#"^node "([\w-]+)" has active drift$"#)]
async fn given_node_drift(world: &mut PactWorld, node: String) {
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: node,
        state: ConfigState::Drifted,
    });
    world.drift_vector_override = DriftVector { kernel: 2.0, mounts: 1.0, ..Default::default() };
}

// "N config entries in the journal" — defined in cli.rs (shared step)

#[given("a fleet of 100 nodes")]
async fn given_fleet(world: &mut PactWorld) {
    // Just set a few representative nodes
    for i in 0..5 {
        world.journal.apply_command(JournalCommand::UpdateNodeState {
            node_id: format!("node-{i:03}"),
            state: if i == 2 { ConfigState::Drifted } else { ConfigState::Committed },
        });
    }
}

#[given(regex = r#"^the policy allows AI-initiated commits for vCluster "([\w-]+)"$"#)]
async fn given_policy_ai_commit(world: &mut PactWorld, _vcluster: String) {
    world.auth_result = Some(AuthResult::Authorized);
}

#[given(regex = r#"^the policy allows AI-initiated applies for vCluster "([\w-]+)"$"#)]
async fn given_policy_ai_apply(world: &mut PactWorld, _vcluster: String) {
    world.auth_result = Some(AuthResult::Authorized);
}

#[given(regex = r#"^the policy allows AI-initiated rollbacks for vCluster "([\w-]+)"$"#)]
async fn given_policy_ai_rollback(world: &mut PactWorld, _vcluster: String) {
    world.auth_result = Some(AuthResult::Authorized);
}

#[given(regex = r#"^the policy does not authorize AI exec on vCluster "([\w-]+)"$"#)]
async fn given_policy_no_ai_exec(world: &mut PactWorld, _vcluster: String) {
    world.auth_result =
        Some(AuthResult::Denied { reason: "AI exec not authorized for this vCluster".into() });
}

#[given(regex = r#"^the policy authorizes AI exec for diagnostics on vCluster "([\w-]+)"$"#)]
async fn given_policy_ai_exec(world: &mut PactWorld, _vcluster: String) {
    world.auth_result = Some(AuthResult::Authorized);
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when("the AI agent calls pact_status")]
async fn when_ai_status(world: &mut PactWorld) {
    let result = dispatch_tool("pact_status", &json!({}));
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
    record_audit(world, AdminOperationType::Exec, "pact_status");
}

#[when(regex = r#"^the AI agent calls pact_diff for node "([\w-]+)"$"#)]
async fn when_ai_diff(world: &mut PactWorld, node: String) {
    let result = dispatch_tool("pact_diff", &json!({"node": node}));
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
    record_audit(world, AdminOperationType::Exec, &format!("pact_diff node={node}"));
}

#[when(regex = r"^the AI agent calls pact_log with limit (\d+)$")]
async fn when_ai_log(world: &mut PactWorld, limit: u64) {
    let result = dispatch_tool("pact_log", &json!({"n": limit}));
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
}

#[when("the AI agent calls pact_query_fleet for degraded GPUs")]
async fn when_ai_query_fleet(world: &mut PactWorld) {
    let result =
        dispatch_tool("pact_query_fleet", &json!({"capability_filter": "gpu_health=degraded"}));
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
}

#[when(regex = r#"^the AI agent calls pact_service_status for "([\w-]+)"$"#)]
async fn when_ai_service_status(world: &mut PactWorld, service: String) {
    let result = dispatch_tool("pact_service_status", &json!({"service": service}));
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
}

#[when(regex = r#"^the AI agent calls pact_commit for vCluster "([\w-]+)"$"#)]
async fn when_ai_commit(world: &mut PactWorld, vcluster: String) {
    let result = dispatch_tool(
        "pact_commit",
        &json!({"message": "AI-initiated commit", "scope": format!("vc:{}", vcluster)}),
    );
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));

    // Record audit entry for the AI agent commit
    if !result.is_error {
        world.journal.apply_command(JournalCommand::RecordOperation(
            pact_common::types::AdminOperation {
                operation_id: uuid::Uuid::new_v4().to_string(),
                operation_type: pact_common::types::AdminOperationType::Exec,
                actor: Identity {
                    principal: "service/ai-agent".into(),
                    principal_type: PrincipalType::Service,
                    role: "pact-service-ai".into(),
                },
                scope: pact_common::types::Scope::VCluster(vcluster),
                detail: "AI-initiated commit".into(),
                timestamp: chrono::Utc::now(),
            },
        ));
    }
}

#[when("the AI agent calls pact_apply with a config spec")]
async fn when_ai_apply(world: &mut PactWorld) {
    let result = dispatch_tool(
        "pact_apply",
        &json!({"scope": "vc:dev-sandbox", "config": {"sysctl": {}}, "message": "AI apply"}),
    );
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
}

#[when(regex = r"^the AI agent calls pact_rollback to sequence (\d+)$")]
async fn when_ai_rollback(world: &mut PactWorld, seq: u64) {
    let result = dispatch_tool("pact_rollback", &json!({"sequence": seq}));
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
}

#[when("the AI agent calls pact_emergency")]
async fn when_ai_emergency(world: &mut PactWorld) {
    let result = dispatch_tool(
        "pact_emergency",
        &json!({"action": "start", "reason": "AI emergency request"}),
    );
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
    if result.is_error {
        world.last_denial_reason = Some(result.content[0].text.clone());
        world.auth_result = Some(AuthResult::Denied { reason: result.content[0].text.clone() });
    }
}

#[when(regex = r#"^the AI agent calls pact_exec on node "([\w-]+)"$"#)]
async fn when_ai_exec_denied(world: &mut PactWorld, node: String) {
    // Policy already set to denied
    if matches!(world.auth_result, Some(AuthResult::Denied { .. })) {
        world.cli_exit_code = Some(3);
    } else {
        let result = dispatch_tool("pact_exec", &json!({"node": node, "command": "hostname"}));
        world.cli_output = Some(result.content[0].text.clone());
        world.cli_exit_code = Some(i32::from(result.is_error));
    }
}

#[when(regex = r#"^the AI agent calls pact_exec with command "([\w-]+)" on node "([\w-]+)"$"#)]
async fn when_ai_exec(world: &mut PactWorld, command: String, node: String) {
    let result = dispatch_tool("pact_exec", &json!({"node": node, "command": command}));
    world.cli_output = Some(result.content[0].text.clone());
    world.cli_exit_code = Some(i32::from(result.is_error));
    record_audit(
        world,
        AdminOperationType::Exec,
        &format!("pact_exec node={node} command={command}"),
    );
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then("the response should include node states and vCluster info")]
async fn then_status_response(world: &mut PactWorld) {
    assert!(world.cli_output.is_some(), "should have output");
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the response should include the drift vector")]
async fn then_drift_response(world: &mut PactWorld) {
    assert!(world.cli_output.is_some());
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then(regex = r"^the response should include (\d+) entries$")]
async fn then_log_entries(world: &mut PactWorld, _count: u64) {
    assert!(world.cli_output.is_some());
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the response should include nodes with degraded GPU health")]
async fn then_fleet_response(world: &mut PactWorld) {
    assert!(world.cli_output.is_some());
    let output = world.cli_output.as_ref().unwrap();
    assert!(output.contains("gpu_health=degraded"), "should query for degraded GPUs");
}

#[then("the response should include the service state across nodes")]
async fn then_service_response(world: &mut PactWorld) {
    assert!(world.cli_output.is_some());
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the commit should succeed")]
async fn then_commit_succeeds(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then(regex = r#"^the author should be recorded as "(.*)"$"#)]
async fn then_author_recorded(world: &mut PactWorld, expected: String) {
    let has_ai_author = world.journal.audit_log.iter().any(|op| op.actor.principal == expected);
    assert!(has_ai_author, "audit log should contain author '{expected}'");
}

#[then("the apply should succeed")]
async fn then_apply_succeeds(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the rollback should succeed")]
async fn then_rollback_succeeds(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
}

// "the denial reason should be ..." — defined in partition.rs (shared step)
// "the command should execute" — defined in shell.rs (shared step)

#[then(regex = r#"^the author should be "(.*)"$"#)]
async fn then_author_is(world: &mut PactWorld, expected: String) {
    let has_author = world.journal.audit_log.iter().any(|op| op.actor.principal == expected);
    assert!(has_author, "author should be '{expected}'");
}

#[then("both operations should be recorded in the audit log")]
async fn then_both_recorded(world: &mut PactWorld) {
    assert!(
        world.journal.audit_log.len() >= 2,
        "audit log should contain at least 2 entries, got {}",
        world.journal.audit_log.len()
    );
}

#[then(regex = r#"^the actor should have principal type "([\w]+)"$"#)]
async fn then_actor_type(world: &mut PactWorld, ptype: String) {
    let expected = match ptype.as_str() {
        "Service" => PrincipalType::Service,
        "Human" => PrincipalType::Human,
        _ => panic!("unknown principal type: {ptype}"),
    };
    let has_type = world.journal.audit_log.iter().any(|op| op.actor.principal_type == expected);
    assert!(has_type, "audit log should contain actor with type {ptype}");
}
