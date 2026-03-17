//! Contract tests for event producer/consumer pairs.
//!
//! These tests verify the integration surface between:
//! - Event producers (agents, journal, CLI, policy service)
//! - Event consumers (journal state, CLI commands, scheduler, Loki, audit trail)
//!
//! All tests reference their source contract and spec requirement.
//! Tests use stubs — they define WHAT the event flow must do,
//! not HOW it does it internally.

// ---------------------------------------------------------------------------
// Journal Event Producer/Consumer Pairs
// ---------------------------------------------------------------------------

/// Contract: event-catalog.md § Journal Events — Commit
/// Spec: J7 — all entries go through Raft consensus
/// If this test didn't exist: commit events might not be readable by journal state or CLI log/diff.
#[test]
fn commit_entry_consumed_by_journal_and_cli() {
    let producer = stub_event_producer();
    let journal_consumer = stub_event_consumer("journal");
    let cli_consumer = stub_event_consumer("cli");

    let commit_event = producer.emit_commit(
        "compute-042",
        "ml-training",
        Identity { principal: "admin@example.com".into(), role: "pact-ops-ml-training".into() },
        StateDelta { keys: vec![("kernel.shmmax".into(), "68719476736".into())] },
    );

    assert!(commit_event.sequence > 0);
    assert!(commit_event.timestamp <= Utc::now());
    assert_eq!(commit_event.entry_type, EntryType::Commit);

    // Journal state machine can apply it
    let journal_result = journal_consumer.apply(commit_event.clone());
    assert!(journal_result.is_ok());

    // CLI log/diff can read it
    let cli_result = cli_consumer.read_entry(commit_event.sequence);
    assert_eq!(cli_result.entry_type, EntryType::Commit);
    assert_eq!(cli_result.scope.node_id, "compute-042");
}

/// Contract: event-catalog.md § Journal Events — Rollback
/// Spec: A4 — auto-rollback on expiry
/// If this test didn't exist: rollback entries could lack a parent reference, making undo chains unverifiable.
#[test]
fn rollback_entry_links_to_original() {
    let producer = stub_event_producer();

    let commit_event = producer.emit_commit(
        "compute-042",
        "ml-training",
        Identity { principal: "admin@example.com".into(), role: "pact-ops-ml-training".into() },
        StateDelta { keys: vec![("kernel.shmmax".into(), "68719476736".into())] },
    );

    let rollback_event = producer.emit_rollback(
        "compute-042",
        "ml-training",
        Identity { principal: "admin@example.com".into(), role: "pact-ops-ml-training".into() },
        commit_event.sequence, // parent reference
    );

    assert_eq!(rollback_event.entry_type, EntryType::Rollback);
    assert_eq!(rollback_event.parent_sequence, Some(commit_event.sequence));
    assert!(rollback_event.sequence > commit_event.sequence);
}

/// Contract: event-catalog.md § Journal Events — DriftDetected
/// Spec: D4 — weight calculation triggers detection
/// If this test didn't exist: DriftDetected events might not feed pact status display.
#[test]
fn drift_detected_consumed_by_status() {
    let producer = stub_event_producer();
    let status_consumer = stub_event_consumer("cli-status");

    let drift_event = producer.emit_drift_detected(
        "compute-042",
        DriftVector {
            dimensions: vec![
                DriftComponent { dimension: DriftDimension::Files, key: "/etc/hosts".into(), magnitude: 0.8 },
            ],
            total_magnitude: 0.8,
        },
    );

    assert_eq!(drift_event.entry_type, EntryType::DriftDetected);

    let status = status_consumer.read_drift_status("compute-042");
    assert!(status.has_drift);
    assert_eq!(status.drift_dimensions, vec![DriftDimension::Files]);
}

/// Contract: event-catalog.md § Journal Events — CapabilityChange
/// Spec: lattice scheduler integration
/// If this test didn't exist: GPU state changes might not reach the scheduler, causing stale node capabilities.
#[test]
fn capability_change_consumed_by_scheduler() {
    let producer = stub_event_producer();
    let scheduler_consumer = stub_event_consumer("lattice-scheduler");

    let cap_event = producer.emit_capability_change(
        "compute-042",
        CapabilityDelta {
            gpu_id: "GPU-0".into(),
            old_state: GpuState::Healthy,
            new_state: GpuState::Failed,
        },
    );

    assert_eq!(cap_event.entry_type, EntryType::CapabilityChange);

    let scheduler_update = scheduler_consumer.receive_capability_update("compute-042");
    assert_eq!(scheduler_update.gpu_id, "GPU-0");
    assert_eq!(scheduler_update.new_state, GpuState::Failed);
}

/// Contract: event-catalog.md § Journal Events — PolicyUpdate
/// Spec: SubscribeConfigUpdates delivery to all agents
/// If this test didn't exist: policy updates could be missed by agents, leaving stale RBAC rules.
#[test]
fn policy_update_consumed_by_agents() {
    let producer = stub_event_producer();
    let agent_consumers = vec![
        stub_event_consumer("agent-compute-001"),
        stub_event_consumer("agent-compute-002"),
        stub_event_consumer("agent-compute-003"),
    ];

    let policy_event = producer.emit_policy_update(
        "ml-training",
        PolicyDelta { role: "pact-ops-ml-training".into(), permissions_changed: true },
    );

    assert_eq!(policy_event.entry_type, EntryType::PolicyUpdate);
    assert_eq!(policy_event.scope.vcluster_id, "ml-training");

    // All agents receive the update via SubscribeConfigUpdates
    for consumer in &agent_consumers {
        let update = consumer.receive_config_update();
        assert_eq!(update.entry_type, EntryType::PolicyUpdate);
        assert_eq!(update.scope.vcluster_id, "ml-training");
    }
}

/// Contract: event-catalog.md § Journal Events — EmergencyStart
/// Spec: O3 — audit trail never interrupted; Loki alert always fires
/// If this test didn't exist: emergency mode could start without triggering an alert.
#[test]
fn emergency_start_triggers_loki_alert() {
    let producer = stub_event_producer();
    let loki_consumer = stub_event_consumer("loki");

    let emergency_event = producer.emit_emergency_start(
        "compute-042",
        Identity { principal: "admin@example.com".into(), role: "pact-ops-ml-training".into() },
        "node unresponsive to workload scheduler",
    );

    assert_eq!(emergency_event.entry_type, EntryType::EmergencyStart);

    let alert = loki_consumer.receive_alert("compute-042");
    assert!(alert.always_fires);
    assert_eq!(alert.category, "emergency_mode");
    assert!(!alert.reason.is_empty());
}

/// Contract: event-catalog.md § Admin Operation Events — Exec
/// Spec: O3 — audit log entries never deleted
/// If this test didn't exist: exec commands could execute without audit trail entries.
#[test]
fn exec_log_consumed_by_audit() {
    let producer = stub_event_producer();
    let audit_consumer = stub_event_consumer("audit");

    let exec_event = producer.emit_exec_log(
        "compute-042",
        Identity { principal: "admin@example.com".into(), role: "pact-ops-ml-training".into() },
        "nvidia-smi",
    );

    assert_eq!(exec_event.entry_type, EntryType::ExecLog);

    let audit_entry = audit_consumer.read_audit_entry(exec_event.sequence);
    assert_eq!(audit_entry.operation_type, AdminOperationType::Exec);
    assert_eq!(audit_entry.identity.principal, "admin@example.com");
    assert_eq!(audit_entry.detail, "nvidia-smi");
}

/// Contract: event-catalog.md § Journal Events — ServiceLifecycle
/// Spec: pact status displays service state
/// If this test didn't exist: service start/stop/crash events might not appear in status output.
#[test]
fn service_lifecycle_consumed_by_status() {
    let producer = stub_event_producer();
    let status_consumer = stub_event_consumer("cli-status");

    let lifecycle_event = producer.emit_service_lifecycle(
        "compute-042",
        "lattice-node-agent",
        ServiceEvent::Started,
    );

    assert_eq!(lifecycle_event.entry_type, EntryType::ServiceLifecycle);

    let status = status_consumer.read_service_status("compute-042", "lattice-node-agent");
    assert_eq!(status.state, ServiceState::Running);
}

/// Contract: event-catalog.md § Journal Events — PendingApproval
/// Spec: P4 — two-person approval requested
/// If this test didn't exist: pending approvals might not be readable by `pact approve`.
#[test]
fn pending_approval_consumed_by_approve_command() {
    let producer = stub_event_producer();
    let cli_consumer = stub_event_consumer("cli-approve");

    let approval_event = producer.emit_pending_approval(
        "ml-training",
        Identity { principal: "requester@example.com".into(), role: "pact-ops-ml-training".into() },
        "service stop lattice-node-agent",
    );

    assert_eq!(approval_event.entry_type, EntryType::PendingApproval);

    let pending = cli_consumer.list_pending_approvals("ml-training");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].requester.principal, "requester@example.com");
    assert_eq!(pending[0].action, "service stop lattice-node-agent");
}

// ---------------------------------------------------------------------------
// Agent Runtime Event Flow
// ---------------------------------------------------------------------------

/// Contract: event-catalog.md § Agent Runtime Events — DriftEvent
/// Spec: mpsc channel — all observers feed same evaluator
/// If this test didn't exist: drift events from observers might not reach the DriftEvaluator.
#[test]
fn drift_event_flows_through_mpsc_channel() {
    let pipeline = stub_drift_pipeline();

    let events = test_drift_events(vec![
        ("inotify", DriftDimension::Files, "/etc/hosts", "content changed"),
        ("netlink", DriftDimension::Network, "eth0", "address changed"),
    ]);

    for event in &events {
        pipeline.send(event.clone());
    }

    let received = pipeline.evaluator_receive_all();
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].dimension, DriftDimension::Files);
    assert_eq!(received[1].dimension, DriftDimension::Network);
}

/// Contract: event-catalog.md § Agent Runtime Events — DriftEvent
/// Spec: D1 — blacklisted paths filtered before emission
/// If this test didn't exist: drift in /tmp, /var/log, /proc etc. would trigger false commit windows.
#[test]
fn blacklisted_drift_event_filtered() {
    let pipeline = stub_drift_pipeline();

    let events = test_drift_events(vec![
        ("inotify", DriftDimension::Files, "/tmp/scratch/data.bin", "created"),
        ("inotify", DriftDimension::Files, "/var/log/syslog", "rotated"),
        ("ebpf", DriftDimension::Files, "/proc/sys/vm/swappiness", "read"),
        ("inotify", DriftDimension::Files, "/sys/class/net/eth0/speed", "changed"),
        ("inotify", DriftDimension::Files, "/dev/shm/workspace", "created"),
        ("inotify", DriftDimension::Files, "/run/user/1000/bus", "changed"),
        ("inotify", DriftDimension::Files, "/etc/hosts", "modified"), // NOT blacklisted
    ]);

    for event in &events {
        pipeline.send(event.clone());
    }

    let received = pipeline.evaluator_receive_all();
    // Only /etc/hosts should pass the blacklist filter
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].key, "/etc/hosts");
}

/// Contract: event-catalog.md § Agent Runtime Events — DriftEvent
/// Spec: D4 — multiple DriftEvents aggregate into single DriftVector
/// If this test didn't exist: each drift event could trigger its own commit window instead of aggregating.
#[test]
fn drift_event_aggregates_into_drift_vector() {
    let pipeline = stub_drift_pipeline();

    let events = test_drift_events(vec![
        ("inotify", DriftDimension::Files, "/etc/hosts", "modified"),
        ("inotify", DriftDimension::Files, "/etc/resolv.conf", "modified"),
        ("netlink", DriftDimension::Network, "eth0", "address changed"),
    ]);

    for event in &events {
        pipeline.send(event.clone());
    }

    let vector = pipeline.evaluate_drift_vector();
    assert_eq!(vector.dimensions.len(), 3);
    assert!(vector.total_magnitude > 0.0);
    // Aggregated, not three separate vectors
    assert_eq!(vector.dimensions.iter().filter(|d| d.dimension == DriftDimension::Files).count(), 2);
    assert_eq!(vector.dimensions.iter().filter(|d| d.dimension == DriftDimension::Network).count(), 1);
}

/// Contract: event-catalog.md § Agent Runtime Events — DriftEvent
/// Spec: D5 — observe-only mode: logged but no commit window opened
/// If this test didn't exist: observe-only bootstrap could silently converge, violating the principle.
#[test]
fn observe_only_drift_event_logged_not_enforced() {
    let pipeline = stub_drift_pipeline();
    pipeline.set_mode(DriftMode::ObserveOnly);

    let events = test_drift_events(vec![
        ("inotify", DriftDimension::Files, "/etc/hosts", "modified"),
    ]);

    for event in &events {
        pipeline.send(event.clone());
    }

    let vector = pipeline.evaluate_drift_vector();
    assert!(vector.total_magnitude > 0.0);

    // Logged to journal
    assert!(pipeline.was_logged(&events[0]));
    // But no commit window opened
    assert!(!pipeline.commit_window_opened());
}

// ---------------------------------------------------------------------------
// Conflict Events
// ---------------------------------------------------------------------------

/// Contract: event-catalog.md § Journal Events — MergeConflictDetected
/// Spec: CR2 — local changes conflict with journal state on same keys
/// If this test didn't exist: agent reconnect with conflicting keys could silently overwrite.
#[test]
fn merge_conflict_produced_on_reconnect() {
    let producer = stub_event_producer();

    let conflict_event = producer.emit_merge_conflict_detected(
        "compute-042",
        ConflictDetail {
            conflicting_keys: vec!["kernel.shmmax".into(), "net.core.rmem_max".into()],
            local_values: vec!["68719476736".into(), "16777216".into()],
            journal_values: vec!["34359738368".into(), "8388608".into()],
        },
    );

    assert_eq!(conflict_event.entry_type, EntryType::MergeConflictDetected);
    assert_eq!(conflict_event.scope.node_id, "compute-042");
    assert_eq!(conflict_event.detail.conflicting_keys.len(), 2);
}

/// Contract: event-catalog.md § Journal Events — MergeConflictDetected
/// Spec: CR5 — active CLI sessions notified of merge conflict
/// If this test didn't exist: admins in active shells might not know about conflicts.
#[test]
fn merge_conflict_consumed_by_cli_notification() {
    let producer = stub_event_producer();
    let cli_consumer = stub_event_consumer("cli-active-session");

    let conflict_event = producer.emit_merge_conflict_detected(
        "compute-042",
        ConflictDetail {
            conflicting_keys: vec!["kernel.shmmax".into()],
            local_values: vec!["68719476736".into()],
            journal_values: vec!["34359738368".into()],
        },
    );

    let notification = cli_consumer.receive_conflict_notification("compute-042");
    assert!(notification.is_some());
    assert_eq!(notification.unwrap().conflicting_keys, vec!["kernel.shmmax"]);
}

/// Contract: event-catalog.md § Journal Events — GracePeriodOverwrite
/// Spec: CR3 — journal-wins applied after grace period expires
/// If this test didn't exist: grace period expiry might not produce an auditable event.
#[test]
fn grace_period_overwrite_produces_event() {
    let producer = stub_event_producer();
    let loki_consumer = stub_event_consumer("loki");

    let overwrite_event = producer.emit_grace_period_overwrite(
        "compute-042",
        OverwriteDetail {
            overwritten_keys: vec!["kernel.shmmax".into()],
            grace_period_seconds: 300,
        },
    );

    assert_eq!(overwrite_event.entry_type, EntryType::GracePeriodOverwrite);

    let alert = loki_consumer.receive_alert("compute-042");
    assert_eq!(alert.category, "grace_period_overwrite");
    assert!(alert.always_fires);
}

/// Contract: event-catalog.md § Journal Events — PromoteConflictDetected
/// Spec: CR4 — promote blocked by conflicting local changes on target nodes
/// If this test didn't exist: promote could overwrite local changes on target nodes without warning.
#[test]
fn promote_conflict_blocks_cli() {
    let producer = stub_event_producer();
    let cli_consumer = stub_event_consumer("cli-promote");

    let conflict_event = producer.emit_promote_conflict_detected(
        "ml-training",
        PromoteConflictDetail {
            promoting_node: "compute-001".into(),
            conflicting_nodes: vec!["compute-002".into(), "compute-003".into()],
            keys: vec!["kernel.shmmax".into()],
        },
    );

    assert_eq!(conflict_event.entry_type, EntryType::PromoteConflictDetected);

    let blocked = cli_consumer.is_promote_blocked("ml-training");
    assert!(blocked);
}

// ---------------------------------------------------------------------------
// Malformed Event Handling
// ---------------------------------------------------------------------------

/// Contract: event-catalog.md § Invariants — J3, J7
/// Spec: missing required fields rejected, not silently dropped
/// If this test didn't exist: malformed entries could pollute the journal log.
#[test]
fn malformed_config_entry_rejected() {
    let consumer = stub_event_consumer("journal");

    // Entry with empty scope
    let malformed = ConfigEntry {
        entry_type: EntryType::Commit,
        scope: Scope { node_id: "".into(), vcluster_id: "".into() },
        identity: Identity { principal: "admin@example.com".into(), role: "pact-ops-ml-training".into() },
        state_delta: Some(StateDelta { keys: vec![("key".into(), "value".into())] }),
        timestamp: Utc::now(),
        sequence: 1,
        parent_sequence: None,
    };

    let result = consumer.apply(malformed);
    assert!(result.is_err());
    assert_matches!(result, Err(EventError::MalformedEntry { .. }));
}

/// Contract: event-catalog.md § Invariants — J3
/// Spec: J3 — every entry has authenticated Identity (non-empty principal + role)
/// If this test didn't exist: entries without an author could enter the audit trail.
#[test]
fn config_entry_with_missing_author_rejected() {
    let consumer = stub_event_consumer("journal");

    let entry_no_principal = ConfigEntry {
        entry_type: EntryType::Commit,
        scope: Scope { node_id: "compute-042".into(), vcluster_id: "ml-training".into() },
        identity: Identity { principal: "".into(), role: "pact-ops-ml-training".into() },
        state_delta: Some(StateDelta { keys: vec![("key".into(), "value".into())] }),
        timestamp: Utc::now(),
        sequence: 1,
        parent_sequence: None,
    };

    let result = consumer.apply(entry_no_principal);
    assert!(result.is_err());
    assert_matches!(result, Err(EventError::MissingAuthor));

    let entry_no_role = ConfigEntry {
        entry_type: EntryType::Commit,
        scope: Scope { node_id: "compute-042".into(), vcluster_id: "ml-training".into() },
        identity: Identity { principal: "admin@example.com".into(), role: "".into() },
        state_delta: Some(StateDelta { keys: vec![("key".into(), "value".into())] }),
        timestamp: Utc::now(),
        sequence: 2,
        parent_sequence: None,
    };

    let result = consumer.apply(entry_no_role);
    assert!(result.is_err());
    assert_matches!(result, Err(EventError::MissingAuthor));
}

/// Contract: event-catalog.md § Agent Runtime Events — DriftEvent
/// Spec: enum mismatch caught at deserialization boundary
/// If this test didn't exist: unknown drift dimensions could crash the evaluator or be silently ignored.
#[test]
fn drift_event_with_unknown_dimension_rejected() {
    let pipeline = stub_drift_pipeline();

    let raw_event = RawDriftEvent {
        timestamp: Utc::now(),
        source: "inotify".into(),
        dimension: "UnknownDimension".into(), // not in DriftDimension enum
        key: "/etc/hosts".into(),
        detail: "modified".into(),
    };

    let result = pipeline.parse_and_send(raw_event);
    assert!(result.is_err());
    assert_matches!(result, Err(EventError::UnknownDriftDimension(_)));
}

// ---------------------------------------------------------------------------
// Enrollment Events (cross-check with enrollment contracts)
// ---------------------------------------------------------------------------

/// Contract: event-catalog.md § Journal Events — NodeEnrolled
/// Spec: ADR-008 — admin registered a node in enrollment registry
/// If this test didn't exist: node registration might not produce a journal entry.
#[test]
fn node_enrolled_event_produced_on_register() {
    let producer = stub_event_producer();
    let journal_consumer = stub_event_consumer("journal");

    let enrolled_event = producer.emit_node_enrolled(
        "compute-042",
        Identity { principal: "admin@example.com".into(), role: "pact-platform-admin".into() },
        HardwareIdentity {
            mac_addresses: vec!["aa:bb:cc:dd:ee:01".into()],
            bmc_serial: "SN12345".into(),
            tpm_ek_hash: None,
        },
    );

    assert_eq!(enrolled_event.entry_type, EntryType::NodeEnrolled);
    assert_eq!(enrolled_event.scope.node_id, "compute-042");

    let result = journal_consumer.apply(enrolled_event);
    assert!(result.is_ok());
}

/// Contract: event-catalog.md § Journal Events — CertSigned
/// Spec: ADR-008 — CSR signed by journal intermediate CA
/// If this test didn't exist: cert signing might not be recorded in Raft state.
#[test]
fn cert_signed_event_produced_on_enroll() {
    let producer = stub_event_producer();
    let journal_consumer = stub_event_consumer("journal");

    let cert_event = producer.emit_cert_signed(
        "compute-042",
        CertDetail {
            cert_serial: "SERIAL-001".into(),
            not_before: Utc::now(),
            not_after: Utc::now() + Duration::days(90),
            issuer: "pact-intermediate-ca".into(),
        },
    );

    assert_eq!(cert_event.entry_type, EntryType::CertSigned);
    assert_eq!(cert_event.scope.node_id, "compute-042");

    let result = journal_consumer.apply(cert_event.clone());
    assert!(result.is_ok());

    // Recorded in Raft state
    let raft_cert = journal_consumer.get_cert_record("compute-042");
    assert_eq!(raft_cert.cert_serial, "SERIAL-001");
}

/// Contract: event-catalog.md § Journal Events — CertRevoked
/// Spec: ADR-008 — certificate revoked on decommission, added to Vault CRL
/// If this test didn't exist: decommissioned nodes could retain valid certs.
#[test]
fn cert_revoked_event_produced_on_decommission() {
    let producer = stub_event_producer();
    let journal_consumer = stub_event_consumer("journal");
    let vault_consumer = stub_event_consumer("vault-crl");

    let revoke_event = producer.emit_cert_revoked(
        "compute-042",
        CertRevokeDetail {
            cert_serial: "SERIAL-001".into(),
            reason: RevocationReason::Decommission,
        },
    );

    assert_eq!(revoke_event.entry_type, EntryType::CertRevoked);

    // Journal records revocation
    let journal_result = journal_consumer.apply(revoke_event.clone());
    assert!(journal_result.is_ok());

    // Vault CRL updated
    let crl_result = vault_consumer.apply(revoke_event);
    assert!(crl_result.is_ok());
    assert!(vault_consumer.is_serial_revoked("SERIAL-001"));
}
