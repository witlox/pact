//! Contract tests for conflict resolution failure mode degradation.
//!
//! These test that failure modes F13, F14 degrade as specified.
//!
//! Source: specs/failure-modes.md § F13, F14

// ---------------------------------------------------------------------------
// F13: Merge conflict on partition reconnect
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F13
/// Spec: agent pauses convergence for conflicting keys only
/// If this test didn't exist: a single conflicting key could block convergence
/// for the entire config, leaving non-conflicting changes unapplied.
#[test]
fn f13_conflicting_keys_pause_convergence() {
    let agent = stub_agent_runtime();

    // Agent has local changes on keys A and B during partition
    agent.set_local_config("kernel.sysctl.vm.swappiness", "10");
    agent.set_local_config("kernel.sysctl.net.core.somaxconn", "4096");

    // Journal has a different value for key A, but no change to key B
    let journal_state = stub_journal_config_state();
    journal_state.set("kernel.sysctl.vm.swappiness", "60"); // conflicts with local "10"

    let conflicts = agent.detect_merge_conflicts(&journal_state);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].key, "kernel.sysctl.vm.swappiness");
    assert_eq!(conflicts[0].local_value, "10");
    assert_eq!(conflicts[0].journal_value, "60");

    // Agent should pause convergence for conflicting key
    assert!(agent.is_convergence_paused_for("kernel.sysctl.vm.swappiness"));
}

/// Contract: failure-modes.md § F13
/// Spec: non-conflicting keys sync normally during conflict
/// If this test didn't exist: a merge conflict on one key could block all
/// config synchronization, preventing the agent from converging on safe keys.
#[test]
fn f13_non_conflicting_keys_sync_normally() {
    let agent = stub_agent_runtime();

    // Agent has local changes on keys A and B
    agent.set_local_config("kernel.sysctl.vm.swappiness", "10");
    agent.set_local_config("kernel.sysctl.net.core.somaxconn", "4096");

    // Journal has a different value for key A only
    let journal_state = stub_journal_config_state();
    journal_state.set("kernel.sysctl.vm.swappiness", "60");

    agent.reconcile_with_journal(&journal_state);

    // Non-conflicting key B should be synced to journal
    assert!(!agent.is_convergence_paused_for("kernel.sysctl.net.core.somaxconn"));
}

/// Contract: failure-modes.md § F13
/// Spec: admin can resolve with AcceptLocal to promote local value to journal
/// If this test didn't exist: there would be no way to preserve an admin's
/// intentional local change made during a partition.
#[test]
fn f13_admin_can_accept_local() {
    let agent = stub_agent_runtime();
    let journal = stub_journal_client();

    let conflict = MergeConflict {
        key: "kernel.sysctl.vm.swappiness".into(),
        local_value: "10".into(),
        journal_value: "60".into(),
    };

    let result = agent.resolve_conflict(
        &conflict,
        ConflictResolution::AcceptLocal,
        &journal,
    );
    assert!(result.is_ok());

    // Local value should be promoted to journal
    let journal_value = journal.get_config_value("kernel.sysctl.vm.swappiness").unwrap();
    assert_eq!(journal_value, "10");
    assert!(!agent.is_convergence_paused_for("kernel.sysctl.vm.swappiness"));
}

/// Contract: failure-modes.md § F13
/// Spec: admin can resolve with AcceptJournal to overwrite local value
/// If this test didn't exist: there would be no way to discard a local change
/// and converge to the journal's authoritative state.
#[test]
fn f13_admin_can_accept_journal() {
    let agent = stub_agent_runtime();
    let journal = stub_journal_client();

    let conflict = MergeConflict {
        key: "kernel.sysctl.vm.swappiness".into(),
        local_value: "10".into(),
        journal_value: "60".into(),
    };

    let result = agent.resolve_conflict(
        &conflict,
        ConflictResolution::AcceptJournal,
        &journal,
    );
    assert!(result.is_ok());

    // Agent should apply journal value
    let local_value = agent.get_config_value("kernel.sysctl.vm.swappiness").unwrap();
    assert_eq!(local_value, "60");
    assert!(!agent.is_convergence_paused_for("kernel.sysctl.vm.swappiness"));
}

/// Contract: failure-modes.md § F13
/// Spec: grace period timeout causes journal-wins fallback, logged for audit
/// If this test didn't exist: unresolved conflicts could block convergence
/// indefinitely, leaving nodes in a permanently drifted state.
#[test]
fn f13_grace_period_timeout_journal_wins() {
    let agent = stub_agent_runtime();
    let journal = stub_journal_client();

    let conflict = MergeConflict {
        key: "kernel.sysctl.vm.swappiness".into(),
        local_value: "10".into(),
        journal_value: "60".into(),
    };

    agent.register_conflict(conflict);

    // Advance past grace period (default: commit window duration)
    let grace_period = agent.conflict_grace_period();
    agent.advance_time(grace_period + Duration::seconds(1));

    // Journal-wins fallback should trigger automatically
    agent.tick_conflict_resolution(&journal).unwrap();

    let local_value = agent.get_config_value("kernel.sysctl.vm.swappiness").unwrap();
    assert_eq!(local_value, "60", "journal-wins after grace period timeout");
    assert!(!agent.is_convergence_paused_for("kernel.sysctl.vm.swappiness"));

    // Fallback must be logged for audit
    let audit_log = journal.conflict_resolution_entries();
    assert_eq!(audit_log.len(), 1);
    assert_eq!(audit_log[0].resolution, "journal-wins-timeout");
}

/// Contract: failure-modes.md § F13
/// Spec: overwritten local changes logged for audit regardless of resolution path
/// If this test didn't exist: overwritten local changes would be invisible in
/// the audit trail, making post-incident analysis impossible.
#[test]
fn f13_overwritten_changes_logged() {
    let agent = stub_agent_runtime();
    let journal = stub_journal_client();

    let conflict = MergeConflict {
        key: "kernel.sysctl.vm.swappiness".into(),
        local_value: "10".into(),
        journal_value: "60".into(),
    };

    // Resolve by accepting journal — local value is overwritten
    agent.resolve_conflict(
        &conflict,
        ConflictResolution::AcceptJournal,
        &journal,
    ).unwrap();

    let audit_log = journal.conflict_resolution_entries();
    assert_eq!(audit_log.len(), 1);
    assert_eq!(audit_log[0].overwritten_value, "10");
    assert_eq!(audit_log[0].accepted_value, "60");
    assert_eq!(audit_log[0].key, "kernel.sysctl.vm.swappiness");
}

// ---------------------------------------------------------------------------
// F14: Promote conflicts with local node changes
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F14
/// Spec: promote blocked when target nodes have local changes on same keys
/// If this test didn't exist: a promote could silently overwrite intentional
/// per-node customizations without admin awareness.
#[test]
fn f14_promote_pauses_on_conflicts() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    // Node compute-042 has local delta on key A
    journal.seed_node_delta("compute-042", "kernel.sysctl.vm.swappiness", "10");

    // Promote from compute-001 changes the same key
    let promote_request = PromoteRequest {
        source_node: "compute-001".into(),
        vcluster_id: "ml-training".into(),
        keys: vec![("kernel.sysctl.vm.swappiness".into(), "60".into())],
    };

    let result = config_service_promote(&journal, &raft, &identity, promote_request);
    assert_matches!(result, Err(PactError::PromoteConflict { conflicts, .. }) => {
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].node_id, "compute-042");
        assert_eq!(conflicts[0].key, "kernel.sysctl.vm.swappiness");
    });
}

/// Contract: failure-modes.md § F14
/// Spec: admin must explicitly resolve each conflicting key before promote proceeds
/// If this test didn't exist: a bulk "accept all" could silently overwrite
/// node-specific customizations without per-key review.
#[test]
fn f14_each_conflict_requires_explicit_ack() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    // Multiple nodes have local deltas on different keys
    journal.seed_node_delta("compute-042", "kernel.sysctl.vm.swappiness", "10");
    journal.seed_node_delta("compute-043", "kernel.sysctl.net.core.somaxconn", "2048");

    let promote_request = PromoteRequest {
        source_node: "compute-001".into(),
        vcluster_id: "ml-training".into(),
        keys: vec![
            ("kernel.sysctl.vm.swappiness".into(), "60".into()),
            ("kernel.sysctl.net.core.somaxconn".into(), "4096".into()),
        ],
    };

    // Initial promote returns conflicts
    let result = config_service_promote(&journal, &raft, &identity, promote_request.clone());
    let conflicts = match result {
        Err(PactError::PromoteConflict { conflicts, .. }) => conflicts,
        other => panic!("expected PromoteConflict, got {:?}", other),
    };

    // Resolve only the first conflict
    let partial_acks = vec![
        ConflictAck {
            node_id: "compute-042".into(),
            key: "kernel.sysctl.vm.swappiness".into(),
            resolution: ConflictResolution::AcceptJournal,
        },
    ];

    // Promote with partial acks should still fail — second conflict unresolved
    let result = config_service_promote_with_acks(
        &journal, &raft, &identity, promote_request, partial_acks,
    );
    assert_matches!(result, Err(PactError::PromoteConflict { conflicts, .. }) => {
        assert_eq!(conflicts.len(), 1, "only unresolved conflicts should remain");
        assert_eq!(conflicts[0].node_id, "compute-043");
    });
}

/// Contract: failure-modes.md § F14
/// Spec: promote without conflicts succeeds immediately
/// If this test didn't exist: the conflict detection path could interfere
/// with conflict-free promotes, adding unnecessary admin friction.
#[test]
fn f14_no_conflicts_promote_proceeds() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    // No nodes have local deltas on the promoted keys
    let promote_request = PromoteRequest {
        source_node: "compute-001".into(),
        vcluster_id: "ml-training".into(),
        keys: vec![("kernel.sysctl.vm.swappiness".into(), "60".into())],
    };

    let result = config_service_promote(&journal, &raft, &identity, promote_request);
    assert!(result.is_ok(), "promote without conflicts must succeed immediately");
}
