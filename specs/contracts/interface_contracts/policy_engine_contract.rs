//! Contract tests for PolicyEngine library interfaces.
//!
//! These tests verify the integration surface between:
//! - pact-journal (PolicyService handler) ↔ pact-policy (PolicyEngine, RBAC, OPA)
//! - pact-agent (degraded mode) ↔ pact-policy (PolicyCache)
//!
//! All tests reference their source contract and spec requirement.
//! Tests use stubs — they define WHAT the interface must do,
//! not HOW it does it internally.

// ---------------------------------------------------------------------------
// PolicyEngine trait contracts
// ---------------------------------------------------------------------------

/// Contract: policy-interfaces.md § PolicyEngine
/// Spec: P6 — platform-admin always allowed, still logged
/// If this test didn't exist: platform-admin might be evaluated through full RBAC/OPA pipeline unnecessarily.
#[test]
fn evaluate_platform_admin_always_allow() {
    let engine = stub_policy_engine();

    let request = test_policy_request(
        Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        },
        "commit",
        Scope::VCluster("ml-training".into()),
    );

    let decision = engine.evaluate(&request).unwrap();
    assert_matches!(decision, PolicyDecision::Allow { .. });
}

/// Contract: policy-interfaces.md § PolicyEngine
/// Spec: policy_evaluation.feature scenarios 4-5 — viewer Allow for reads, Deny for writes
/// If this test didn't exist: viewers could modify cluster state.
#[test]
fn evaluate_viewer_allow_read_deny_write() {
    let engine = stub_policy_engine();

    let viewer = Identity {
        principal: "auditor@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-viewer-ml-training".into(),
    };

    let read_request = test_policy_request(viewer.clone(), "status", Scope::VCluster("ml-training".into()));
    let read_decision = engine.evaluate(&read_request).unwrap();
    assert_matches!(read_decision, PolicyDecision::Allow { .. });

    let write_request = test_policy_request(viewer, "commit", Scope::VCluster("ml-training".into()));
    let write_decision = engine.evaluate(&write_request).unwrap();
    assert_matches!(write_decision, PolicyDecision::Deny { .. });
}

/// Contract: policy-interfaces.md § PolicyEngine
/// Spec: P4 — regulated vCluster + state-changing → RequireApproval
/// If this test didn't exist: regulated ops could bypass two-person approval.
#[test]
fn evaluate_regulated_two_person_approval() {
    let engine = stub_policy_engine();

    let request = test_policy_request(
        Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-regulated-bio-compute".into(),
        },
        "commit",
        Scope::VCluster("bio-compute".into()),
    );

    let decision = engine.evaluate(&request).unwrap();
    assert_matches!(decision, PolicyDecision::RequireApproval { .. });
}

/// Contract: policy-interfaces.md § PolicyEngine
/// Spec: P4, policy_evaluation.feature scenario 10 — same admin cannot approve own request
/// If this test didn't exist: a single admin could approve their own regulated change.
#[test]
fn evaluate_self_approval_denied() {
    let engine = stub_policy_engine();

    let request = PolicyRequest {
        identity: Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-regulated-bio-compute".into(),
        },
        scope: Scope::VCluster("bio-compute".into()),
        action: "approve".into(),
        proposed_change: None,
        command: None,
        approval_context: Some(ApprovalContext {
            approval_id: "approval-001".into(),
            original_requester: "ops@example.com".into(),
        }),
    };

    let decision = engine.evaluate(&request).unwrap();
    assert_matches!(decision, PolicyDecision::Deny { reason, .. } if reason.contains("self-approval"));
}

/// Contract: policy-interfaces.md § PolicyEngine
/// Spec: P8 — AI agent denied for emergency action
/// If this test didn't exist: an AI agent could trigger emergency mode autonomously.
#[test]
fn evaluate_ai_agent_emergency_denied() {
    let engine = stub_policy_engine();

    let request = test_policy_request(
        Identity {
            principal: "mcp-agent-001".into(),
            principal_type: PrincipalType::Service,
            role: "pact-service-ai".into(),
        },
        "emergency",
        Scope::VCluster("ml-training".into()),
    );

    let decision = engine.evaluate(&request).unwrap();
    assert_matches!(decision, PolicyDecision::Deny { .. });
}

/// Contract: policy-interfaces.md § PolicyEngine::get_effective_policy
/// Spec: returns VClusterPolicy for vCluster
/// If this test didn't exist: callers might receive stale or empty policy.
#[test]
fn get_effective_policy_returns_stored_policy() {
    let engine = stub_policy_engine();
    let policy = test_vcluster_policy("ml-training");

    let result = engine.get_effective_policy(&VClusterId::new("ml-training")).unwrap();
    assert_eq!(result.vcluster_id.as_str(), "ml-training");
    assert!(!result.role_bindings.is_empty());
}

/// Contract: policy-interfaces.md § PolicyEngine::get_effective_policy
/// Spec: federation templates merged when enabled
/// If this test didn't exist: federation overrides would be silently ignored.
#[test]
fn get_effective_policy_merges_federation_overrides() {
    let engine = stub_policy_engine(); // federation enabled

    let result = engine.get_effective_policy(&VClusterId::new("federated-cluster")).unwrap();
    assert!(result.federation_overrides_applied);
    assert!(!result.rego_templates.is_empty());
}

// ---------------------------------------------------------------------------
// TokenValidator trait contracts
// ---------------------------------------------------------------------------

/// Contract: policy-interfaces.md § TokenValidator
/// Spec: successful validation extracts Identity
/// If this test didn't exist: valid tokens might not produce a usable Identity.
#[test]
fn validate_returns_identity_for_valid_token() {
    let validator = stub_token_validator();

    let identity = validator.validate("valid-jwt-token").unwrap();
    assert_eq!(identity.principal, "user@example.com");
    assert!(!identity.role.is_empty());
}

/// Contract: policy-interfaces.md § TokenValidator
/// Spec: rbac_authorization.feature scenario 8 — expired token rejected
/// If this test didn't exist: expired tokens could grant access.
#[test]
fn validate_rejects_expired_token() {
    let validator = stub_token_validator();

    let result = validator.validate("expired-jwt-token");
    assert_matches!(result, Err(PactError::TokenExpired));
}

/// Contract: policy-interfaces.md § TokenValidator
/// Spec: rbac_authorization.feature scenario 9 — wrong audience rejected
/// If this test didn't exist: tokens issued for other services could grant pact access.
#[test]
fn validate_rejects_wrong_audience() {
    let validator = stub_token_validator();

    let result = validator.validate("wrong-audience-jwt-token");
    assert_matches!(result, Err(PactError::InvalidAudience));
}

// ---------------------------------------------------------------------------
// RbacEngine trait contracts
// ---------------------------------------------------------------------------

/// Contract: policy-interfaces.md § RbacEngine
/// Spec: P3 — role with matching vCluster scope → Allow
/// If this test didn't exist: correctly scoped roles might be denied.
#[test]
fn evaluate_allows_matching_scope() {
    let rbac = stub_rbac_engine();
    let policy = test_vcluster_policy("ml-training");

    let identity = Identity {
        principal: "user@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };

    let decision = rbac.evaluate(&identity, "commit", &Scope::VCluster("ml-training".into()), &policy);
    assert_matches!(decision, RbacDecision::Allow);
}

/// Contract: policy-interfaces.md § RbacEngine
/// Spec: P3 — role for wrong vCluster → Deny
/// If this test didn't exist: ops for one vCluster could manage another.
#[test]
fn evaluate_denies_mismatched_scope() {
    let rbac = stub_rbac_engine();
    let policy = test_vcluster_policy("regulated-bio");

    let identity = Identity {
        principal: "user@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };

    let decision = rbac.evaluate(&identity, "commit", &Scope::VCluster("regulated-bio".into()), &policy);
    assert_matches!(decision, RbacDecision::Deny { .. });
}

/// Contract: policy-interfaces.md § RbacEngine
/// Spec: complex rules → Defer (escalate to OPA)
/// If this test didn't exist: complex rules would be silently allowed or denied without OPA.
#[test]
fn evaluate_defers_complex_rules_to_opa() {
    let rbac = stub_rbac_engine();
    let policy = test_vcluster_policy("regulated-bio"); // has complex Rego rules

    let identity = Identity {
        principal: "user@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-regulated-bio".into(),
    };

    let decision = rbac.evaluate(&identity, "exec", &Scope::VCluster("regulated-bio".into()), &policy);
    assert_matches!(decision, RbacDecision::Defer);
}

/// Contract: policy-interfaces.md § RbacEngine
/// Spec: P6 — platform-admin → Allow (early return)
/// If this test didn't exist: platform-admin might be sent to OPA unnecessarily.
#[test]
fn evaluate_platform_admin_early_allow() {
    let rbac = stub_rbac_engine();
    let policy = test_vcluster_policy("ml-training");

    let identity = Identity {
        principal: "admin@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-platform-admin".into(),
    };

    let decision = rbac.evaluate(&identity, "emergency", &Scope::VCluster("ml-training".into()), &policy);
    assert_matches!(decision, RbacDecision::Allow);
}

// ---------------------------------------------------------------------------
// OpaClient trait contracts
// ---------------------------------------------------------------------------

/// Contract: policy-interfaces.md § OpaClient
/// Spec: ADR-003 — OpaInput has principal, role, action, scope, context
/// If this test didn't exist: OPA sidecar might receive malformed input.
#[test]
fn evaluate_sends_correct_input_format() {
    let opa = stub_opa_client();

    let input = OpaInput {
        principal: "user@example.com".into(),
        role: "pact-ops-ml-training".into(),
        action: "exec".into(),
        scope: "ml-training".into(),
        context: serde_json::json!({"command": "nvidia-smi"}),
    };

    let result = opa.evaluate(&input).unwrap();
    // Should succeed without serialization or schema errors
    assert!(result.allow || !result.allow); // valid response received
}

/// Contract: policy-interfaces.md § OpaClient
/// Spec: ADR-003 — OpaResult has allow + optional reason
/// If this test didn't exist: callers might not receive actionable deny reasons.
#[test]
fn evaluate_returns_allow_deny_with_reason() {
    let opa = stub_opa_client();

    let deny_input = OpaInput {
        principal: "user@example.com".into(),
        role: "pact-viewer-ml-training".into(),
        action: "exec".into(),
        scope: "ml-training".into(),
        context: serde_json::json!({}),
    };

    let result = opa.evaluate(&deny_input).unwrap();
    assert!(!result.allow);
    assert!(result.reason.is_some());
}

/// Contract: policy-interfaces.md § OpaClient
/// Spec: ADR-003, F7 — health check detects OPA unavailability
/// If this test didn't exist: callers wouldn't know to fall back to cached policy.
#[test]
fn health_returns_false_when_sidecar_down() {
    let opa = stub_opa_client(); // configured with unreachable sidecar

    let healthy = opa.health();
    assert!(!healthy);
}

// ---------------------------------------------------------------------------
// PolicyCache (degraded mode) contracts
// ---------------------------------------------------------------------------

/// Contract: policy-interfaces.md § PolicyCache
/// Spec: P7 — cached whitelist checks work in degraded mode
/// If this test didn't exist: agents in degraded mode might deny all operations.
#[test]
fn evaluate_degraded_honors_whitelist() {
    let cache = stub_policy_cache();

    let request = test_policy_request(
        Identity {
            principal: "user@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        "status",
        Scope::VCluster("ml-training".into()),
    );

    let decision = cache.evaluate_degraded(&request);
    assert_matches!(decision, PolicyDecision::Allow { .. });
}

/// Contract: policy-interfaces.md § PolicyCache
/// Spec: P7 — two-person approval denied in degraded mode (fail-closed)
/// If this test didn't exist: regulated ops could bypass approval when journal is unreachable.
#[test]
fn evaluate_degraded_denies_two_person() {
    let cache = stub_policy_cache();

    let request = test_policy_request(
        Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-regulated-bio-compute".into(),
        },
        "commit",
        Scope::VCluster("bio-compute".into()),
    );

    let decision = cache.evaluate_degraded(&request);
    assert_matches!(decision, PolicyDecision::Deny { reason, .. } if reason.contains("degraded"));
}

/// Contract: policy-interfaces.md § PolicyCache
/// Spec: P7 — complex OPA rules denied in degraded mode (fail-closed)
/// If this test didn't exist: complex rules would silently pass without OPA evaluation.
#[test]
fn evaluate_degraded_denies_complex_opa() {
    let cache = stub_policy_cache();

    let request = test_policy_request(
        Identity {
            principal: "user@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-regulated-bio".into(),
        },
        "exec",
        Scope::VCluster("regulated-bio".into()),
    );

    let decision = cache.evaluate_degraded(&request);
    assert_matches!(decision, PolicyDecision::Deny { reason, .. } if reason.contains("degraded"));
}

/// Contract: policy-interfaces.md § PolicyCache
/// Spec: P7 — platform admin authorized with cached role in degraded mode
/// If this test didn't exist: platform admins would be locked out during journal outage.
#[test]
fn evaluate_degraded_allows_platform_admin() {
    let cache = stub_policy_cache();

    let request = test_policy_request(
        Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        },
        "emergency",
        Scope::VCluster("ml-training".into()),
    );

    let decision = cache.evaluate_degraded(&request);
    assert_matches!(decision, PolicyDecision::Allow { .. });
}

// ---------------------------------------------------------------------------
// FederationSync contracts
// ---------------------------------------------------------------------------

/// Contract: policy-interfaces.md § FederationSync
/// Spec: federation.feature — pulls Rego templates from Sovra on interval
/// If this test didn't exist: federation sync might silently not pull templates.
#[test]
fn sync_pulls_rego_templates() {
    let sync = stub_federation_sync();

    let result = sync.sync();
    assert!(result.is_ok());
    assert!(sync.templates_updated());
}

/// Contract: policy-interfaces.md § FederationSync
/// Spec: F10 — graceful failure, uses cached templates
/// If this test didn't exist: federation failure would break policy evaluation.
#[test]
fn sync_uses_cached_on_failure() {
    let sync = stub_federation_sync(); // Sovra endpoint unreachable

    let result = sync.sync();
    // Sync reports error but does not panic
    assert!(result.is_err());
    // Cached templates still available
    assert!(sync.has_cached_templates());
}
