//! Contract tests for conflict resolution and node delta invariants.
//!
//! These test that the enforcement mechanisms identified in enforcement-map.md
//! actually prevent invariant violations.
//!
//! Source: specs/invariants.md § Conflict Resolution Invariants (CR1-CR6)
//! Source: specs/invariants.md § Node Delta Invariants (ND1-ND3)

// ---------------------------------------------------------------------------
// CR1: Local changes fed back before journal sync
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § CR1
/// Spec: invariants.md § CR1 — agent sends pending entries before subscribing
/// If this test didn't exist: reconnecting agents could silently lose local changes.
#[test]
fn cr1_local_changes_fed_back_before_sync() {
    let agent = stub_agent_with_pending_changes(vec![
        PendingEntry { key: "kernel.sysctl.vm.swappiness".into(), value: "10".into() },
        PendingEntry { key: "mount./scratch".into(), value: "/dev/sda1".into() },
    ]);
    let journal = stub_journal_client();

    let reconnect = agent.reconnect(&journal);
    let feed_back_seq = reconnect.feed_back_sequence;
    let subscribe_seq = reconnect.subscribe_sequence;

    assert!(feed_back_seq < subscribe_seq,
        "pending entries (seq {}) must be sent before stream subscription (seq {})",
        feed_back_seq, subscribe_seq);
    assert_eq!(reconnect.fed_back_entries.len(), 2,
        "all pending entries must be reported");
}

// ---------------------------------------------------------------------------
// CR2: Merge conflict pauses agent convergence
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § CR2
/// Spec: invariants.md § CR2 — conflicting keys pause convergence
/// If this test didn't exist: journal state could silently overwrite local changes.
#[test]
fn cr2_merge_conflict_pauses_convergence() {
    let agent = stub_agent_with_local_state(vec![
        ("kernel.sysctl.vm.swappiness", "10"),
    ]);
    let journal_state = vec![
        ConfigEntry { key: "kernel.sysctl.vm.swappiness".into(), value: "60".into() },
    ];

    let result = agent.apply_journal_state(&journal_state);
    assert_matches!(result, Err(AgentError::MergeConflict { conflicts }) => {
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].key, "kernel.sysctl.vm.swappiness");
        assert_eq!(conflicts[0].local_value, "10");
        assert_eq!(conflicts[0].journal_value, "60");
    });

    // Agent state must NOT have been changed
    assert_eq!(agent.get_local("kernel.sysctl.vm.swappiness"), Some("10"),
        "local state must be preserved during conflict");
}

/// Contract: enforcement-map.md § CR2
/// Spec: invariants.md § CR2 — non-conflicting keys proceed normally
/// If this test didn't exist: conflict detection could block all sync, not just conflicts.
#[test]
fn cr2_non_conflicting_keys_sync_normally() {
    let agent = stub_agent_with_local_state(vec![
        ("kernel.sysctl.vm.swappiness", "10"),
    ]);
    let journal_state = vec![
        ConfigEntry { key: "mount./data".into(), value: "/dev/sdb1".into() },
    ];

    let result = agent.apply_journal_state(&journal_state);
    assert!(result.is_ok(), "non-conflicting keys must sync without error");
    assert_eq!(agent.get_local("mount./data"), Some("/dev/sdb1"),
        "non-conflicting journal entry must be applied");
    assert_eq!(agent.get_local("kernel.sysctl.vm.swappiness"), Some("10"),
        "existing local state must be preserved");
}

// ---------------------------------------------------------------------------
// CR3: Grace period fallback to journal-wins
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § CR3
/// Spec: invariants.md § CR3 — unresolved conflict after timeout falls back to journal-wins
/// If this test didn't exist: conflicts could persist indefinitely, blocking convergence.
#[test]
fn cr3_grace_period_fallback_to_journal_wins() {
    let agent = stub_agent_with_conflict(MergeConflict {
        key: "kernel.sysctl.vm.swappiness".into(),
        local_value: "10".into(),
        journal_value: "60".into(),
        detected_at: Utc::now() - Duration::hours(2), // well past grace period
    });
    let grace_period = Duration::minutes(30);

    let result = agent.resolve_expired_conflicts(grace_period);
    assert!(result.is_ok());
    assert_eq!(agent.get_local("kernel.sysctl.vm.swappiness"), Some("60"),
        "journal value must win after grace period expires");
}

/// Contract: enforcement-map.md § CR3
/// Spec: invariants.md § CR3 — overwritten changes are logged in audit
/// If this test didn't exist: local changes could be lost without any record.
#[test]
fn cr3_overwritten_changes_logged() {
    let agent = stub_agent_with_conflict(MergeConflict {
        key: "kernel.sysctl.vm.swappiness".into(),
        local_value: "10".into(),
        journal_value: "60".into(),
        detected_at: Utc::now() - Duration::hours(2),
    });
    let audit = stub_audit_log();
    let grace_period = Duration::minutes(30);

    agent.resolve_expired_conflicts_with_audit(grace_period, &audit).unwrap();

    let overwrite_entry = audit.last_entry();
    assert_eq!(overwrite_entry.event_type, AuditEventType::ConflictAutoResolved);
    assert!(overwrite_entry.details.contains("swappiness"),
        "audit entry must identify the overwritten key");
    assert!(overwrite_entry.details.contains("10"),
        "audit entry must record the overwritten local value");
}

// ---------------------------------------------------------------------------
// CR4: Promote requires conflict acknowledgment
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § CR4
/// Spec: invariants.md § CR4 — promote with conflicts blocks until resolved
/// If this test didn't exist: promote could silently overwrite node-local changes.
#[test]
fn cr4_promote_blocked_on_conflicts() {
    let journal = stub_journal_with_node_conflicts("ml-training", vec![
        NodeConflict {
            node_id: "compute-042".into(),
            key: "kernel.sysctl.vm.swappiness".into(),
            node_value: "10".into(),
            overlay_value: "60".into(),
        },
    ]);
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    let result = journal.promote_delta(
        &caller,
        PromoteRequest {
            vcluster_id: "ml-training".into(),
            source_node: "compute-001".into(),
            keys: vec!["kernel.sysctl.vm.swappiness".into()],
            conflict_resolution: None, // no explicit resolution
        },
    );
    assert_matches!(result, Err(PactError::UnresolvedConflicts { conflicts }) => {
        assert_eq!(conflicts.len(), 1);
    });
}

/// Contract: enforcement-map.md § CR4
/// Spec: invariants.md § CR4 — promote without conflicts succeeds immediately
/// If this test didn't exist: conflict detection could be over-aggressive.
#[test]
fn cr4_promote_no_conflicts_proceeds() {
    let journal = stub_journal_without_node_conflicts("ml-training");
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    let result = journal.promote_delta(
        &caller,
        PromoteRequest {
            vcluster_id: "ml-training".into(),
            source_node: "compute-001".into(),
            keys: vec!["mount./scratch".into()],
            conflict_resolution: None,
        },
    );
    assert!(result.is_ok(), "promote without conflicts must succeed immediately");
}

// ---------------------------------------------------------------------------
// CR5: Admin notification on overwrite
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § CR5
/// Spec: invariants.md § CR5 — active CLI session gets notification on overwrite
/// If this test didn't exist: admins could lose work without knowing.
#[test]
fn cr5_admin_notified_on_overwrite() {
    let session = stub_active_cli_session("ops@example.com");
    let notification_sink = stub_notification_sink();

    // Simulate a promote that overwrites the admin's uncommitted changes
    let overwrite_event = OverwriteEvent {
        affected_principal: "ops@example.com".into(),
        key: "kernel.sysctl.vm.swappiness".into(),
        old_value: "10".into(),
        new_value: "60".into(),
        cause: OverwriteCause::Promote,
    };

    session.notify_overwrite(&notification_sink, &overwrite_event);

    assert!(notification_sink.has_notification_for("ops@example.com"),
        "active CLI session must be notified of overwrite");
    let notification = notification_sink.last_notification();
    assert!(notification.message.contains("swappiness"),
        "notification must identify the overwritten key");
}

// ---------------------------------------------------------------------------
// CR6: No cross-vCluster atomicity
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § CR6
/// Spec: invariants.md § CR6 — each commit scoped to single vCluster
/// If this test didn't exist: cross-vCluster atomic commits could be attempted.
#[test]
fn cr6_no_cross_vcluster_atomicity() {
    let journal = stub_journal_client();
    let caller = platform_admin();

    let result = journal.commit_config(
        &caller,
        CommitRequest {
            vcluster_ids: vec!["ml-training".into(), "regulated-bio".into()],
            entries: vec![
                ConfigEntry { key: "kernel.sysctl.vm.swappiness".into(), value: "60".into() },
            ],
        },
    );
    assert_matches!(result, Err(PactError::ValidationError(msg)) => {
        assert!(msg.contains("single vCluster"),
            "error must explain that commits are scoped to a single vCluster");
    });
}

// ===========================================================================
// Node Delta Invariants (ND1-ND3)
// ===========================================================================

// ---------------------------------------------------------------------------
// ND1: TTL minimum bound (15 minutes = 900 seconds)
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § ND1
/// Spec: invariants.md § ND1 — TTL < 900 seconds rejected
/// If this test didn't exist: ephemeral deltas could expire before anyone notices.
#[test]
fn nd1_ttl_minimum_15_minutes() {
    let journal = stub_journal_client();
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    for ttl_secs in [0, 1, 60, 300, 899] {
        let result = journal.commit_node_delta(
            &caller,
            NodeDeltaRequest {
                vcluster_id: "ml-training".into(),
                node_id: "compute-042".into(),
                key: "mount./scratch".into(),
                value: "/dev/sda1".into(),
                ttl_seconds: ttl_secs,
            },
        );
        assert_matches!(result, Err(PactError::ValidationError(_)),
            "TTL {} seconds must be rejected (minimum 900)", ttl_secs);
    }
}

/// Contract: enforcement-map.md § ND1
/// Spec: invariants.md § ND1 — TTL exactly 900 seconds accepted
/// If this test didn't exist: boundary could be off-by-one.
#[test]
fn nd1_ttl_exactly_15_minutes_accepted() {
    let journal = stub_journal_client();
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    let result = journal.commit_node_delta(
        &caller,
        NodeDeltaRequest {
            vcluster_id: "ml-training".into(),
            node_id: "compute-042".into(),
            key: "mount./scratch".into(),
            value: "/dev/sda1".into(),
            ttl_seconds: 900,
        },
    );
    assert!(result.is_ok(), "TTL exactly 900 seconds must be accepted");
}

// ---------------------------------------------------------------------------
// ND2: TTL maximum bound (10 days = 864000 seconds)
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § ND2
/// Spec: invariants.md § ND2 — TTL > 864000 seconds rejected
/// If this test didn't exist: permanent deltas could defeat vCluster homogeneity.
#[test]
fn nd2_ttl_maximum_10_days() {
    let journal = stub_journal_client();
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    for ttl_secs in [864_001, 1_000_000, u64::MAX] {
        let result = journal.commit_node_delta(
            &caller,
            NodeDeltaRequest {
                vcluster_id: "ml-training".into(),
                node_id: "compute-042".into(),
                key: "mount./scratch".into(),
                value: "/dev/sda1".into(),
                ttl_seconds: ttl_secs,
            },
        );
        assert_matches!(result, Err(PactError::ValidationError(_)),
            "TTL {} seconds must be rejected (maximum 864000)", ttl_secs);
    }
}

/// Contract: enforcement-map.md § ND2
/// Spec: invariants.md § ND2 — TTL exactly 864000 seconds accepted
/// If this test didn't exist: boundary could be off-by-one.
#[test]
fn nd2_ttl_exactly_10_days_accepted() {
    let journal = stub_journal_client();
    let caller = test_identity("ops@example.com", "pact-ops-ml-training");

    let result = journal.commit_node_delta(
        &caller,
        NodeDeltaRequest {
            vcluster_id: "ml-training".into(),
            node_id: "compute-042".into(),
            key: "mount./scratch".into(),
            value: "/dev/sda1".into(),
            ttl_seconds: 864_000,
        },
    );
    assert!(result.is_ok(), "TTL exactly 864000 seconds must be accepted");
}

// ---------------------------------------------------------------------------
// ND3: vCluster homogeneity expectation
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § ND3
/// Spec: invariants.md § ND3 — nodes with deltas differing from overlay emit warning
/// If this test didn't exist: divergent nodes could go unnoticed.
#[test]
fn nd3_divergent_nodes_warned() {
    let journal = stub_journal_with_overlay("ml-training", vec![
        ("kernel.sysctl.vm.swappiness", "60"),
    ]);
    // Node has a delta that differs from the overlay
    journal.add_node_delta("compute-042", "kernel.sysctl.vm.swappiness", "10", 3600);
    let diagnostics = stub_diagnostics();

    let result = journal.check_vcluster_homogeneity("ml-training", &diagnostics);
    assert!(result.is_ok());

    let warnings = diagnostics.warnings();
    assert!(!warnings.is_empty(), "divergent node must produce a warning");
    assert!(warnings[0].contains("compute-042"),
        "warning must identify the divergent node");
    assert!(warnings[0].contains("swappiness"),
        "warning must identify the divergent key");
}
