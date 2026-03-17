//! Contract tests for policy invariants.
//!
//! These test that the enforcement mechanisms identified in enforcement-map.md
//! actually prevent invariant violations.
//!
//! Source: specs/invariants.md § Policy Invariants (P1-P8)

// ---------------------------------------------------------------------------
// P1: Every operation is authenticated
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P1
/// Spec: invariants.md § P1 — unauthenticated requests are rejected
/// If this test didn't exist: gRPC endpoints could serve unauthenticated callers.
#[test]
fn p1_unauthenticated_request_rejected() {
    let journal = stub_journal_client();

    // Request with no Bearer token
    let request = ConfigRequest {
        vcluster_id: "ml-training".into(),
        token: None,
    };
    let result = journal.get_config(request);
    assert_matches!(result, Err(PactError::Unauthenticated(_)),
        "missing OIDC token must yield UNAUTHENTICATED");
}

/// Contract: enforcement-map.md § P1
/// Spec: invariants.md § P1 — malformed/invalid tokens are rejected
/// If this test didn't exist: a garbage string could pass authentication.
#[test]
fn p1_invalid_token_rejected() {
    let journal = stub_journal_client();

    for bad_token in ["", "not-a-jwt", "eyJ.eyJ.INVALID", "Bearer "] {
        let request = ConfigRequest {
            vcluster_id: "ml-training".into(),
            token: Some(bad_token.into()),
        };
        let result = journal.get_config(request);
        assert_matches!(result, Err(PactError::Unauthenticated(_)),
            "token {:?} must be rejected as UNAUTHENTICATED", bad_token);
    }
}

// ---------------------------------------------------------------------------
// P2: Every operation is authorized
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P2
/// Spec: invariants.md § P2 — valid auth but wrong role yields PERMISSION_DENIED
/// If this test didn't exist: authenticated users could bypass RBAC entirely.
#[test]
fn p2_unauthorized_operation_denied() {
    let policy = stub_policy_engine();
    let caller = test_identity("viewer@example.com", "pact-viewer-ml-training");

    // Viewer attempts a state-changing operation
    let result = policy.evaluate(
        &caller,
        Operation::CommitConfig { vcluster_id: "ml-training".into() },
    );
    assert_matches!(result, Ok(PolicyDecision::Deny { reason }) => {
        assert!(!reason.is_empty(), "denial must include a reason");
    });
}

// ---------------------------------------------------------------------------
// P3: Role scoping
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P3
/// Spec: invariants.md § P3 — ops role scoped to its own vCluster
/// If this test didn't exist: ml-training ops could modify regulated-bio state.
#[test]
fn p3_role_scoped_to_vcluster() {
    let policy = stub_policy_engine();
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    let result = policy.evaluate(
        &caller,
        Operation::CommitConfig { vcluster_id: "regulated-bio".into() },
    );
    assert_matches!(result, Ok(PolicyDecision::Deny { .. }),
        "pact-ops-ml-training must be denied on regulated-bio");
}

/// Contract: enforcement-map.md § P3
/// Spec: invariants.md § P3 — ops role allowed on its own vCluster
/// If this test didn't exist: role scoping could be too restrictive.
#[test]
fn p3_role_scoping_allows_own_vcluster() {
    let policy = stub_policy_engine();
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    let result = policy.evaluate(
        &caller,
        Operation::CommitConfig { vcluster_id: "ml-training".into() },
    );
    assert_matches!(result, Ok(PolicyDecision::Allow),
        "pact-ops-ml-training must be allowed on ml-training");
}

// ---------------------------------------------------------------------------
// P4: Two-person approval for regulated vClusters
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P4
/// Spec: invariants.md § P4 — regulated + state-changing requires second approval
/// If this test didn't exist: regulated vClusters could be modified by one person.
#[test]
fn p4_two_person_approval_required() {
    let policy = stub_policy_engine_with_two_person("regulated-bio");
    let caller = test_identity("ops@example.com", "pact-ops-regulated-bio");

    let result = policy.evaluate(
        &caller,
        Operation::CommitConfig { vcluster_id: "regulated-bio".into() },
    );
    assert_matches!(result, Ok(PolicyDecision::RequireApproval { .. }),
        "regulated vCluster state change must require two-person approval");
}

/// Contract: enforcement-map.md § P4
/// Spec: invariants.md § P4 — requester cannot approve their own request
/// If this test didn't exist: one person could approve their own changes.
#[test]
fn p4_self_approval_denied() {
    let policy = stub_policy_engine_with_two_person("regulated-bio");
    let requester = test_identity("ops@example.com", "pact-ops-regulated-bio");

    let pending = PendingApproval {
        id: "approval-001".into(),
        requested_by: requester.clone(),
        operation: Operation::CommitConfig { vcluster_id: "regulated-bio".into() },
        expires_at: Utc::now() + Duration::minutes(30),
    };

    let result = policy.approve(&requester, &pending);
    assert_matches!(result, Err(PactError::SelfApprovalDenied),
        "same principal must not approve their own request");
}

// ---------------------------------------------------------------------------
// P5: Approval timeout
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P5
/// Spec: invariants.md § P5 — expired approval is rejected
/// If this test didn't exist: stale approvals could be used indefinitely.
#[test]
fn p5_expired_approval_rejected() {
    let policy = stub_policy_engine_with_two_person("regulated-bio");
    let requester = test_identity("ops@example.com", "pact-ops-regulated-bio");
    let approver = test_identity("admin@example.com", "pact-ops-regulated-bio");

    let expired = PendingApproval {
        id: "approval-002".into(),
        requested_by: requester,
        operation: Operation::CommitConfig { vcluster_id: "regulated-bio".into() },
        expires_at: Utc::now() - Duration::minutes(1), // already expired
    };

    let result = policy.approve(&approver, &expired);
    assert_matches!(result, Err(PactError::ApprovalExpired { .. }),
        "expired approval must be rejected");
}

// ---------------------------------------------------------------------------
// P6: Platform admin always authorized
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P6
/// Spec: invariants.md § P6 — platform-admin is authorized for any operation
/// If this test didn't exist: platform admins could be blocked by vCluster scoping.
#[test]
fn p6_platform_admin_always_allowed() {
    let policy = stub_policy_engine_with_two_person("regulated-bio");
    let admin = platform_admin();

    // Test across multiple vClusters and operation types
    let operations = vec![
        Operation::CommitConfig { vcluster_id: "regulated-bio".into() },
        Operation::CommitConfig { vcluster_id: "ml-training".into() },
        Operation::EnterEmergency { node_id: "compute-042".into() },
        Operation::DecommissionNode { node_id: "compute-042".into() },
    ];

    for op in operations {
        let result = policy.evaluate(&admin, op.clone());
        assert_matches!(result, Ok(PolicyDecision::Allow),
            "platform-admin must be allowed for {:?}", op);
    }
}

/// Contract: enforcement-map.md § P6
/// Spec: invariants.md § P6 — platform-admin actions are still recorded
/// If this test didn't exist: platform-admin could operate without audit trail.
#[test]
fn p6_platform_admin_still_logged() {
    let policy = stub_policy_engine();
    let audit = stub_audit_log();
    let admin = platform_admin();

    let op = Operation::CommitConfig { vcluster_id: "ml-training".into() };
    let _ = policy.evaluate_with_audit(&admin, op.clone(), &audit);

    assert!(audit.has_entry_for(&admin, &op),
        "platform-admin action must appear in audit log");
}

// ---------------------------------------------------------------------------
// P7: Degraded mode restrictions
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P7
/// Spec: invariants.md § P7 — cached whitelist honored in degraded mode
/// If this test didn't exist: degraded mode could deny all operations.
#[test]
fn p7_degraded_whitelist_honored() {
    let policy = stub_degraded_policy_engine(vec![
        ("pact-ops-ml-training", Operation::CommitConfig { vcluster_id: "ml-training".into() }),
    ]);
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    let result = policy.evaluate(
        &caller,
        Operation::CommitConfig { vcluster_id: "ml-training".into() },
    );
    assert_matches!(result, Ok(PolicyDecision::Allow),
        "cached whitelist entry must be honored in degraded mode");
}

/// Contract: enforcement-map.md § P7
/// Spec: invariants.md § P7 — two-person approval denied in degraded mode
/// If this test didn't exist: two-person could be deferred, allowing single-person bypass.
#[test]
fn p7_degraded_two_person_denied() {
    let policy = stub_degraded_policy_engine_with_two_person("regulated-bio");
    let caller = test_identity("ops@example.com", "pact-ops-regulated-bio");

    let result = policy.evaluate(
        &caller,
        Operation::CommitConfig { vcluster_id: "regulated-bio".into() },
    );
    assert_matches!(result, Ok(PolicyDecision::Deny { .. }),
        "two-person approval must fail-closed in degraded mode");
}

/// Contract: enforcement-map.md § P7
/// Spec: invariants.md § P7 — complex OPA rules denied in degraded mode
/// If this test didn't exist: OPA-only policies could silently pass without evaluation.
#[test]
fn p7_degraded_opa_denied() {
    let policy = stub_degraded_policy_engine(vec![]);
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    // Operation not in cached whitelist — requires OPA evaluation
    let result = policy.evaluate(
        &caller,
        Operation::ExecCommand {
            node_id: "compute-042".into(),
            command: "custom-script".into(),
        },
    );
    assert_matches!(result, Ok(PolicyDecision::Deny { .. }),
        "complex OPA rules must fail-closed in degraded mode");
}

// ---------------------------------------------------------------------------
// P8: AI agent emergency restriction
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § P8
/// Spec: invariants.md § P8 — pact-service-ai cannot enter emergency mode
/// If this test didn't exist: an AI agent could autonomously enter emergency mode.
#[test]
fn p8_ai_agent_emergency_denied() {
    let policy = stub_policy_engine();
    let ai_agent = Identity {
        principal: "claude@mcp.local".into(),
        principal_type: PrincipalType::Service,
        role: "pact-service-ai".into(),
    };

    let result = policy.evaluate(
        &ai_agent,
        Operation::EnterEmergency { node_id: "compute-042".into() },
    );
    assert_matches!(result, Ok(PolicyDecision::Deny { .. }),
        "pact-service-ai must be denied emergency mode");
}
