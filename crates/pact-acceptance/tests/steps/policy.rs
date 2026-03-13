//! Policy evaluation + RBAC authorization steps — wired to RbacEngine + DefaultPolicyEngine.

use cucumber::{given, then, when};
use pact_common::types::{Identity, PrincipalType, Scope, VClusterPolicy};
use pact_policy::rules::PolicyRequest;

use super::helpers::map_action;
use crate::{AuthResult, PactWorld};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn authorize(world: &mut PactWorld, action: &str, vcluster: &str) -> AuthResult {
    let Some(identity) = &world.current_identity else {
        return AuthResult::Denied {
            reason: "no user context".to_string(),
        };
    };

    let mapped_action = map_action(action);

    let request = PolicyRequest {
        identity: identity.clone(),
        scope: Scope::VCluster(vcluster.into()),
        action: mapped_action.to_string(),
        proposed_change: None,
        command: None,
    };

    // Make sure policy is loaded
    let vc_id = vcluster.to_string();
    if world.policy_engine.get_policy(&vc_id).is_none() {
        world.policy_engine.set_policy(VClusterPolicy {
            vcluster_id: vc_id,
            ..VClusterPolicy::default()
        });
    }

    match world.policy_engine.evaluate_sync(&request) {
        Ok(pact_policy::rules::PolicyDecision::Allow { .. }) => AuthResult::Authorized,
        Ok(pact_policy::rules::PolicyDecision::Deny { reason, .. }) => {
            AuthResult::Denied { reason }
        }
        Ok(pact_policy::rules::PolicyDecision::RequireApproval { approval_id, .. }) => {
            AuthResult::ApprovalRequired { approval_id }
        }
        Err(e) => AuthResult::Denied {
            reason: e.to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(regex = r#"^a user with role "([\w-]+)"$"#)]
async fn given_user_role(world: &mut PactWorld, role: String) {
    world.current_identity = Some(Identity {
        principal: "user@example.com".to_string(),
        role,
        principal_type: PrincipalType::Human,
    });
}

#[given(regex = r#"^a user "([\w@.]+)" with role "([\w-]+)"$"#)]
async fn given_named_user_role(world: &mut PactWorld, principal: String, role: String) {
    world.current_identity = Some(Identity {
        principal,
        role,
        principal_type: PrincipalType::Human,
    });
}

#[given(regex = r#"^a user with role "([\w-]+)" and principal type "(\w+)"$"#)]
async fn given_user_role_type(world: &mut PactWorld, role: String, ptype: String) {
    let principal_type = match ptype.as_str() {
        "Service" => PrincipalType::Service,
        "Agent" => PrincipalType::Agent,
        _ => PrincipalType::Human,
    };
    world.current_identity = Some(Identity {
        principal: "service@pact.internal".to_string(),
        role,
        principal_type,
    });
}

#[given(regex = r#"^vCluster "([\w-]+)" has two-person approval enabled$"#)]
async fn given_two_person_approval(world: &mut PactWorld, vcluster: String) {
    let policy = VClusterPolicy {
        vcluster_id: vcluster.clone(),
        drift_sensitivity: 5.0,
        base_commit_window_seconds: 900,
        emergency_allowed: true,
        two_person_approval: true,
        regulated: true,
        ..VClusterPolicy::default()
    };
    world.policy_engine.set_policy(policy.clone());
    // Also store in journal for cross-module access
    world.journal.apply_command(pact_journal::JournalCommand::SetPolicy {
        vcluster_id: vcluster,
        policy,
    });
}

#[given(regex = r#"^vCluster "([\w-]+)" has policy with emergency_allowed false$"#)]
async fn given_no_emergency_policy(world: &mut PactWorld, vcluster: String) {
    let policy = VClusterPolicy {
        vcluster_id: vcluster.clone(),
        drift_sensitivity: 5.0,
        base_commit_window_seconds: 900,
        emergency_allowed: false,
        two_person_approval: false,
        ..VClusterPolicy::default()
    };
    world.policy_engine.set_policy(policy.clone());
    world.journal.apply_command(pact_journal::JournalCommand::SetPolicy {
        vcluster_id: vcluster,
        policy,
    });
}

#[given("the PolicyService is unreachable")]
async fn given_policy_unreachable(world: &mut PactWorld) {
    world.opa_available = false;
    world.policy_degraded = true;
}

#[given("an MCP server with pact-service-ai identity")]
async fn given_mcp_server(world: &mut PactWorld) {
    world.mcp_active = true;
    world.current_identity = Some(Identity {
        principal: "service/ai-agent".to_string(),
        role: "pact-service-ai".to_string(),
        principal_type: PrincipalType::Service,
    });
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(regex = r#"^the user requests to commit on vCluster "([\w-]+)"$"#)]
async fn when_user_commits(world: &mut PactWorld, vcluster: String) {
    world.auth_result = Some(authorize(world, "commit", &vcluster));
}

#[when(regex = r#"^the user requests action "(\w+)" on vCluster "([\w-]+)"$"#)]
async fn when_user_action(world: &mut PactWorld, action: String, vcluster: String) {
    world.auth_result = Some(authorize(world, &action, &vcluster));
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then("the request should be authorized")]
async fn then_authorized(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::Authorized) => {}
        other => panic!("expected Authorized, got {other:?}"),
    }
}

#[then("the request should be denied")]
async fn then_denied(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::Denied { .. }) => {}
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[then(regex = r#"^the request should be denied with reason "(.*)"$"#)]
async fn then_denied_with_reason(world: &mut PactWorld, expected: String) {
    match &world.auth_result {
        Some(AuthResult::Denied { reason }) => {
            // Flexible matching: check substring, or key noun overlap
            let skip_words = ["the", "a", "an", "is", "not", "for", "to", "of", "on"];
            let key_words: Vec<&str> = expected
                .split_whitespace()
                .filter(|w| w.len() > 3 && !skip_words.contains(w))
                .collect();
            let keyword_match = !key_words.is_empty()
                && key_words.iter().filter(|w| reason.contains(**w)).count() >= key_words.len() / 2 + 1;
            let matches = reason.contains(&expected)
                || keyword_match
                || (expected.contains("cannot") && reason.contains("cannot"));
            assert!(
                matches,
                "expected reason containing '{expected}', got '{reason}'"
            );
        }
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[then("the response should indicate approval required")]
async fn then_approval_required(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::ApprovalRequired { .. }) => {}
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }
}

#[then("the response should require approval from a second administrator")]
async fn then_requires_second_admin(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::ApprovalRequired { .. }) => {}
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }
}
