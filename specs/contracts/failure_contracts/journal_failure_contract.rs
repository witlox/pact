//! Contract tests for journal failure mode degradation.
//!
//! These test that failure modes F1, F7, F8, F9 degrade as specified.
//!
//! Source: specs/failure-modes.md § F1, F7, F8, F9

// ---------------------------------------------------------------------------
// F1: Journal quorum loss
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F1
/// Spec: reads continue from surviving replicas when quorum is lost
/// If this test didn't exist: a quorum loss could block all operations including reads,
/// making status queries and boot streaming impossible during partial outages.
#[test]
fn f1_reads_continue_on_quorum_loss() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    journal.seed_entries(10);
    raft.simulate_quorum_loss();

    // Reads should still work from local state machine
    let entry = config_service_get_entry(&journal, &raft, &identity, 5).unwrap();
    assert!(entry.sequence == 5);
    assert_eq!(raft.proposals_received(), 0); // No Raft round-trip needed for reads
}

/// Contract: failure-modes.md § F1
/// Spec: write operations fail with timeout when quorum is lost
/// If this test didn't exist: writes could silently hang or succeed on a single node,
/// causing split-brain config state.
#[test]
fn f1_writes_blocked_on_quorum_loss() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    raft.simulate_quorum_loss();

    let entry = test_config_entry();
    let result = config_service_append_entry(&journal, &raft, &identity, entry);
    assert_matches!(result, Err(PactError::QuorumLost { .. }));
}

/// Contract: failure-modes.md § F1
/// Spec: boot config streaming continues from surviving replicas
/// If this test didn't exist: a quorum loss could prevent all new nodes from booting,
/// even though boot config is a read operation served from local state.
#[test]
fn f1_boot_streaming_continues() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = test_identity();

    journal.seed_overlay("ml-training", test_overlay_data());
    raft.simulate_quorum_loss();

    // Boot streaming is a read — should work from local state
    let chunks: Vec<_> = boot_config_service_stream(&journal, &identity, "ml-training")
        .collect();

    assert!(!chunks.is_empty());
}

// ---------------------------------------------------------------------------
// F7: OPA sidecar crash
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F7
/// Spec: PolicyService falls back to cached VClusterPolicy evaluation
/// If this test didn't exist: an OPA crash would cause all policy evaluations to fail,
/// blocking every admin operation on that journal node.
#[test]
fn f7_policy_falls_back_to_cached() {
    let journal = stub_journal_state();
    journal.set_opa_reachable(false);

    let request = PolicyEvaluateRequest {
        principal: platform_admin(),
        action: "status".into(),
        resource: "vcluster:ml-training".into(),
    };

    let decision = policy_service_evaluate(&journal, request).unwrap();
    assert_eq!(decision.effect, PolicyEffect::Allow);
    assert!(decision.from_cache, "decision must come from cached policy when OPA is down");
}

/// Contract: failure-modes.md § F7
/// Spec: basic RBAC + whitelist checks still work when OPA is down
/// If this test didn't exist: simple role-based checks that don't need OPA would
/// fail unnecessarily, denying legitimate operations.
#[test]
fn f7_basic_rbac_still_works() {
    let journal = stub_journal_state();
    journal.set_opa_reachable(false);

    let request = PolicyEvaluateRequest {
        principal: Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        action: "status".into(),
        resource: "vcluster:ml-training".into(),
    };

    let decision = policy_service_evaluate(&journal, request).unwrap();
    assert_eq!(decision.effect, PolicyEffect::Allow);
    assert!(decision.from_cache);
}

/// Contract: failure-modes.md § F7
/// Spec: complex Rego rules return denied (fail-closed) when OPA is down
/// If this test didn't exist: complex policy rules could silently pass without
/// evaluation, bypassing compliance constraints.
#[test]
fn f7_complex_rego_denied() {
    let journal = stub_journal_state();
    journal.set_opa_reachable(false);

    let request = PolicyEvaluateRequest {
        principal: Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-regulated-bio-compute".into(),
        },
        action: "exec".into(),
        resource: "vcluster:bio-compute".into(),
    };

    let decision = policy_service_evaluate(&journal, request).unwrap();
    assert_eq!(decision.effect, PolicyEffect::Deny);
    assert!(decision.reason.contains("OPA") || decision.reason.contains("degraded"));
}

// ---------------------------------------------------------------------------
// F8: Raft leader failover
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F8
/// Spec: reads continue from any replica during leader election
/// If this test didn't exist: a brief leader election could block all reads,
/// causing unnecessary boot delays and status query failures.
#[test]
fn f8_reads_continue_during_election() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    journal.seed_entries(5);
    raft.simulate_leader_election();

    // Reads don't need leader — should work from any replica
    let entry = config_service_get_entry(&journal, &raft, &identity, 3).unwrap();
    assert_eq!(entry.sequence, 3);
    assert_eq!(raft.proposals_received(), 0);
}

/// Contract: failure-modes.md § F8
/// Spec: writes resume after new leader is elected
/// If this test didn't exist: writes might remain blocked after election completes,
/// requiring manual intervention to restore write availability.
#[test]
fn f8_writes_resume_on_new_leader() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    // Election in progress — writes should fail
    raft.simulate_leader_election();
    let entry = test_config_entry();
    let result = config_service_append_entry(&journal, &raft, &identity, entry);
    assert!(result.is_err());

    // Election completes — writes should succeed
    raft.complete_election();
    let entry = test_config_entry();
    let result = config_service_append_entry(&journal, &raft, &identity, entry);
    assert!(result.is_ok());
    assert!(raft.term() > 1, "term should increment after election");
}

// ---------------------------------------------------------------------------
// F9: Stale overlay
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F9
/// Spec: stale overlay detected when version < latest config sequence
/// If this test didn't exist: nodes could silently boot with outdated config,
/// running services with stale parameters.
#[test]
fn f9_stale_overlay_detected() {
    let journal = stub_journal_state();
    let identity = test_identity();

    // Overlay built at sequence 5, but config is now at sequence 10
    journal.seed_overlay_at_sequence("ml-training", test_overlay_data(), 5);
    journal.seed_entries(10);

    let staleness = journal.check_overlay_staleness("ml-training").unwrap();
    assert!(staleness.is_stale);
    assert_eq!(staleness.overlay_sequence, 5);
    assert_eq!(staleness.latest_config_sequence, 10);
}

/// Contract: failure-modes.md § F9
/// Spec: stale overlay triggers on-demand rebuild before serving
/// If this test didn't exist: a stale overlay would be served as-is, and the
/// booting node would run with outdated configuration until the next config push.
#[test]
fn f9_on_demand_rebuild_triggered() {
    let journal = stub_journal_state();
    let identity = test_identity();

    // Overlay is stale
    journal.seed_overlay_at_sequence("ml-training", test_overlay_data(), 5);
    journal.seed_entries(10);

    // Requesting boot config for a stale overlay should trigger rebuild
    let chunks: Vec<_> = boot_config_service_stream(&journal, &identity, "ml-training")
        .collect();

    assert!(!chunks.is_empty());
    // Overlay should now be rebuilt to latest sequence
    let staleness = journal.check_overlay_staleness("ml-training").unwrap();
    assert!(!staleness.is_stale, "overlay must be rebuilt before serving");
}
