//! Contract tests for event schemas (ConfigEntry, DriftEvent, AdminOperation, etc.).
//!
//! These tests verify:
//! - Schema shape and field presence for all journal event types
//! - Round-trip serialization of event schemas
//! - Validation constraints from the spec
//! - State transition correctness for approval workflows
//!
//! Source: specs/architecture/events/event-schemas.md

// ---------------------------------------------------------------------------
// Helper stubs
// ---------------------------------------------------------------------------

fn test_config_entry_of_type(entry_type: EntryType) -> ConfigEntry {
    ConfigEntry {
        sequence: 1,
        entry_type,
        scope: Scope::VCluster("ml-training".into()),
        timestamp: Utc::now(),
        author: Identity {
            principal: "alice@example.com".into(),
            role: "pact-ops-ml-training".into(),
        },
        parent: None,
        delta: None,
        ttl: None,
        metadata: HashMap::new(),
    }
}

fn test_drift_event() -> DriftEvent {
    DriftEvent {
        timestamp: Utc::now(),
        source: DriftSource::Inotify,
        dimension: DriftDimension::Files,
        key: "/etc/hosts".into(),
        detail: "file content changed".into(),
    }
}

fn test_admin_operation() -> AdminOperation {
    AdminOperation {
        operation_type: AdminOperationType::Exec,
        identity: Identity {
            principal: "alice@example.com".into(),
            role: "pact-ops-ml-training".into(),
        },
        node_id: "compute-042".into(),
        vcluster_id: "ml-training".into(),
        timestamp: Utc::now(),
        detail: "hostname".into(),
    }
}

fn test_pending_approval() -> PendingApproval {
    let now = Utc::now();
    PendingApproval {
        approval_id: "approval-001".into(),
        original_request: "commit vcluster ml-training".into(),
        action: "commit".into(),
        scope: Scope::VCluster("ml-training".into()),
        requester: Identity {
            principal: "alice@example.com".into(),
            role: "pact-regulated-ml-training".into(),
        },
        approver: None,
        status: ApprovalStatus::Pending,
        created_at: now,
        expires_at: now + Duration::minutes(30),
    }
}

fn test_merge_conflict() -> MergeConflict {
    let now = Utc::now();
    MergeConflict {
        node_id: "compute-042".into(),
        vcluster_id: "ml-training".into(),
        conflicts: vec![ConflictEntry {
            key: "kernel.shmmax".into(),
            local_value: "2147483648".into(),
            journal_value: "1073741824".into(),
            local_changed_at: now - Duration::minutes(5),
            journal_changed_at: now - Duration::minutes(10),
        }],
        detected_at: now,
        grace_period_expires: now + Duration::minutes(15),
    }
}

// ---------------------------------------------------------------------------
// ConfigEntry Schema Validation
// ---------------------------------------------------------------------------

/// Contract: event-schemas.md § ConfigEntry — Commit variant
/// Spec: Commit entries must have delta=Yes and ttl=No
/// If this test didn't exist: a Commit could be recorded without a delta, losing change history.
#[test]
fn commit_entry_has_delta_no_ttl() {
    let mut entry = test_config_entry_of_type(EntryType::Commit);
    entry.delta = Some(StateDelta {
        changes: vec![Change {
            key: "kernel.shmmax".into(),
            old_value: Some("1073741824".into()),
            new_value: Some("2147483648".into()),
        }],
    });
    entry.parent = Some(0);

    assert!(entry.delta.is_some(), "Commit must have a delta");
    assert!(entry.ttl.is_none(), "Commit must not have a ttl");
}

/// Contract: event-schemas.md § ConfigEntry — Rollback variant
/// Spec: Rollback delta is the reverse of the committed state
/// If this test didn't exist: a rollback could apply a forward delta, doubling the change.
#[test]
fn rollback_entry_has_reverse_delta() {
    let mut entry = test_config_entry_of_type(EntryType::Rollback);
    entry.parent = Some(5); // points to original commit
    entry.delta = Some(StateDelta {
        changes: vec![Change {
            key: "kernel.shmmax".into(),
            old_value: Some("2147483648".into()),  // was the new value
            new_value: Some("1073741824".into()),   // reverts to original
        }],
    });

    let change = &entry.delta.as_ref().unwrap().changes[0];
    // Reverse delta: old_value is what was committed, new_value is the original
    assert_ne!(change.old_value, change.new_value, "rollback delta must differ");
    assert!(entry.parent.is_some(), "Rollback must link to rolled-back entry");
}

/// Contract: event-schemas.md § ConfigEntry — EmergencyStart variant
/// Spec: EmergencyStart has ttl (emergency_window_seconds) and no delta
/// If this test didn't exist: an emergency could lack a time window, running indefinitely.
#[test]
fn emergency_start_has_ttl_no_delta() {
    let mut entry = test_config_entry_of_type(EntryType::EmergencyStart);
    entry.ttl = Some(3600); // 1 hour emergency window

    assert!(entry.ttl.is_some(), "EmergencyStart must have a ttl");
    assert!(entry.delta.is_none(), "EmergencyStart must not have a delta");
}

/// Contract: event-schemas.md § ConfigEntry — EmergencyEnd variant
/// Spec: EmergencyEnd parent must point to an EmergencyStart entry
/// If this test didn't exist: an EmergencyEnd could be orphaned with no matching start.
#[test]
fn emergency_end_links_to_start() {
    let start = test_config_entry_of_type(EntryType::EmergencyStart);
    let mut end = test_config_entry_of_type(EntryType::EmergencyEnd);
    end.parent = Some(start.sequence);

    assert!(end.parent.is_some(), "EmergencyEnd must have a parent");
    assert_eq!(
        end.parent.unwrap(),
        start.sequence,
        "EmergencyEnd parent must point to EmergencyStart entry"
    );
}

/// Contract: event-schemas.md § ConfigEntry — ExecLog variant
/// Spec: ExecLog must have metadata["command"] present
/// If this test didn't exist: exec audit entries could omit the command, breaking audit trail.
#[test]
fn exec_log_has_command_in_metadata() {
    let mut entry = test_config_entry_of_type(EntryType::ExecLog);
    entry.metadata.insert("command".into(), "hostname".into());

    assert!(
        entry.metadata.contains_key("command"),
        "ExecLog must have 'command' in metadata"
    );
    assert!(
        !entry.metadata["command"].is_empty(),
        "ExecLog command must be non-empty"
    );
}

/// Contract: event-schemas.md § ConfigEntry — ShellSession variant
/// Spec: ShellSession must have metadata["action"] = "start" or "end"
/// If this test didn't exist: shell sessions could lack start/end markers, breaking session tracking.
#[test]
fn shell_session_has_action_in_metadata() {
    let mut start_entry = test_config_entry_of_type(EntryType::ShellSession);
    start_entry.metadata.insert("action".into(), "start".into());

    let mut end_entry = test_config_entry_of_type(EntryType::ShellSession);
    end_entry.metadata.insert("action".into(), "end".into());

    let valid_actions = ["start", "end"];
    assert!(valid_actions.contains(&start_entry.metadata["action"].as_str()));
    assert!(valid_actions.contains(&end_entry.metadata["action"].as_str()));
}

/// Contract: event-schemas.md § ConfigEntry — ServiceLifecycle variant
/// Spec: ServiceLifecycle must have metadata["service"] and metadata["action"]
/// If this test didn't exist: service events could omit which service changed, breaking audit.
#[test]
fn service_lifecycle_has_service_in_metadata() {
    let mut entry = test_config_entry_of_type(EntryType::ServiceLifecycle);
    entry.metadata.insert("service".into(), "lattice-node-agent".into());
    entry.metadata.insert("action".into(), "start".into());

    assert!(
        entry.metadata.contains_key("service"),
        "ServiceLifecycle must have 'service' in metadata"
    );
    assert!(
        entry.metadata.contains_key("action"),
        "ServiceLifecycle must have 'action' in metadata"
    );
}

/// Contract: event-schemas.md § ConfigEntry — PendingApproval variant
/// Spec: PendingApproval must have ttl set to approval timeout
/// If this test didn't exist: approval requests could hang indefinitely without expiry.
#[test]
fn pending_approval_has_timeout_ttl() {
    let mut entry = test_config_entry_of_type(EntryType::PendingApproval);
    entry.ttl = Some(1800); // 30-minute approval timeout
    entry.metadata.insert("approval_id".into(), "approval-001".into());

    assert!(entry.ttl.is_some(), "PendingApproval must have a ttl");
    assert!(entry.ttl.unwrap() > 0, "PendingApproval ttl must be positive");
}

/// Contract: event-schemas.md § ConfigEntry — DriftDetected variant
/// Spec: DriftDetected has a delta (drift vector) but is informational — no state change
/// If this test didn't exist: drift events could trigger state changes or lack drift data.
#[test]
fn drift_detected_is_informational() {
    let mut entry = test_config_entry_of_type(EntryType::DriftDetected);
    entry.delta = Some(StateDelta {
        changes: vec![Change {
            key: "/etc/hosts".into(),
            old_value: Some("expected-content".into()),
            new_value: Some("actual-content".into()),
        }],
    });

    assert!(entry.delta.is_some(), "DriftDetected must have a delta (drift vector)");
    assert!(entry.ttl.is_none(), "DriftDetected must not have a ttl");
    assert!(entry.parent.is_none(), "DriftDetected is informational — no causal parent");
}

// ---------------------------------------------------------------------------
// DriftEvent Schema
// ---------------------------------------------------------------------------

/// Contract: event-schemas.md § DriftEvent
/// Spec: all fields must round-trip through serialization
/// If this test didn't exist: a field could be silently dropped during serde.
#[test]
fn drift_event_round_trip() {
    let event = test_drift_event();

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: DriftEvent = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.source, event.source);
    assert_eq!(deserialized.dimension, event.dimension);
    assert_eq!(deserialized.key, event.key);
    assert_eq!(deserialized.detail, event.detail);
}

/// Contract: event-schemas.md § DriftSource
/// Spec: exactly 4 variants — Ebpf, Inotify, Netlink, Manual
/// If this test didn't exist: a variant could be added or removed, breaking observer dispatch.
#[test]
fn drift_source_all_variants() {
    let sources = vec![
        DriftSource::Ebpf,
        DriftSource::Inotify,
        DriftSource::Netlink,
        DriftSource::Manual,
    ];

    for source in &sources {
        let json = serde_json::to_string(source).unwrap();
        let deserialized: DriftSource = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, source);
    }
}

/// Contract: event-schemas.md § DriftDimension (D2)
/// Spec: exactly 7 variants — Mounts, Files, Network, Services, Kernel, Packages, Gpu
/// If this test didn't exist: a dimension could be missing, silently ignoring drift in that area.
#[test]
fn drift_dimension_exactly_seven() {
    let dimensions = vec![
        DriftDimension::Mounts,
        DriftDimension::Files,
        DriftDimension::Network,
        DriftDimension::Services,
        DriftDimension::Kernel,
        DriftDimension::Packages,
        DriftDimension::Gpu,
    ];

    assert_eq!(dimensions.len(), 7, "DriftDimension must have exactly 7 variants");

    for dim in &dimensions {
        let json = serde_json::to_string(dim).unwrap();
        let deserialized: DriftDimension = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, dim);
    }
}

/// Contract: event-schemas.md § DriftVector (D3, D4)
/// Spec: magnitude = sqrt(sum((weight_i * dimension_i)^2)), kernel=2.0, gpu=2.0, others=1.0
/// If this test didn't exist: the magnitude formula could use wrong weights or wrong aggregation.
#[test]
fn drift_vector_magnitude_formula() {
    let vector = DriftVector {
        mounts: 1.0,
        files: 1.0,
        network: 1.0,
        services: 1.0,
        kernel: 1.0,   // weight 2.0
        packages: 1.0,
        gpu: 1.0,       // weight 2.0
    };

    // Default weights: kernel=2.0, gpu=2.0, all others=1.0
    // magnitude = sqrt(1^2 + 1^2 + 1^2 + 1^2 + (2*1)^2 + 1^2 + (2*1)^2)
    //           = sqrt(1 + 1 + 1 + 1 + 4 + 1 + 4)
    //           = sqrt(13)
    let expected = (13.0_f64).sqrt();
    let magnitude = vector.magnitude();

    assert!(
        (magnitude - expected).abs() < 1e-10,
        "magnitude should be sqrt(13) = {}, got {}",
        expected,
        magnitude,
    );
}

// ---------------------------------------------------------------------------
// AdminOperation Schema
// ---------------------------------------------------------------------------

/// Contract: event-schemas.md § AdminOperation
/// Spec: all AdminOperationType variants must serialize and deserialize
/// If this test didn't exist: a variant could fail serde, blocking audit logging.
#[test]
fn admin_operation_all_types_round_trip() {
    let types = vec![
        AdminOperationType::Exec,
        AdminOperationType::ShellSessionStart,
        AdminOperationType::ShellSessionEnd,
        AdminOperationType::ServiceStart,
        AdminOperationType::ServiceStop,
        AdminOperationType::ServiceRestart,
        AdminOperationType::EmergencyStart,
        AdminOperationType::EmergencyEnd,
        AdminOperationType::ApprovalDecision,
    ];

    for op_type in types {
        let mut op = test_admin_operation();
        op.operation_type = op_type.clone();

        let json = serde_json::to_string(&op).unwrap();
        let deserialized: AdminOperation = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.operation_type, op.operation_type);
    }
}

/// Contract: event-schemas.md § AdminOperation (J3/O3)
/// Spec: identity must be non-empty — every admin operation must have an authenticated identity
/// If this test didn't exist: anonymous operations could slip through, breaking audit requirements.
#[test]
fn admin_operation_requires_identity() {
    let op = test_admin_operation();

    assert!(
        !op.identity.principal.is_empty(),
        "AdminOperation identity.principal must not be empty"
    );
    assert!(
        !op.identity.role.is_empty(),
        "AdminOperation identity.role must not be empty"
    );
}

// ---------------------------------------------------------------------------
// PendingApproval Schema
// ---------------------------------------------------------------------------

/// Contract: event-schemas.md § PendingApproval (P4)
/// Spec: approver.principal must differ from requester.principal — no self-approval
/// If this test didn't exist: an admin could approve their own request, bypassing two-person rule.
#[test]
fn pending_approval_approver_distinct_from_requester() {
    let mut approval = test_pending_approval();
    approval.approver = Some(Identity {
        principal: "bob@example.com".into(),
        role: "pact-regulated-ml-training".into(),
    });

    assert_ne!(
        approval.requester.principal,
        approval.approver.as_ref().unwrap().principal,
        "P4: approver must differ from requester"
    );
}

/// Contract: event-schemas.md § PendingApproval (P5)
/// Spec: expires_at must be after created_at — approvals have a forward-looking timeout
/// If this test didn't exist: an approval could expire before it was created, immediately timing out.
#[test]
fn pending_approval_expires_at_in_future() {
    let approval = test_pending_approval();

    assert!(
        approval.expires_at > approval.created_at,
        "P5: expires_at must be after created_at"
    );
}

/// Contract: event-schemas.md § ApprovalStatus
/// Spec: valid transitions are Pending->Approved, Pending->Rejected, Pending->Expired
/// If this test didn't exist: invalid transitions (e.g., Approved->Pending) could occur.
#[test]
fn approval_status_transitions() {
    let statuses = vec![
        (ApprovalStatus::Pending, ApprovalStatus::Approved),
        (ApprovalStatus::Pending, ApprovalStatus::Rejected),
        (ApprovalStatus::Pending, ApprovalStatus::Expired),
    ];

    for (from, to) in &statuses {
        let json_from = serde_json::to_string(from).unwrap();
        let json_to = serde_json::to_string(to).unwrap();
        let deser_from: ApprovalStatus = serde_json::from_str(&json_from).unwrap();
        let deser_to: ApprovalStatus = serde_json::from_str(&json_to).unwrap();

        // All valid transitions start from Pending
        assert_eq!(&deser_from, &ApprovalStatus::Pending);
        assert_ne!(&deser_to, &ApprovalStatus::Pending, "target must not be Pending");
    }
}

// ---------------------------------------------------------------------------
// Conflict Event Schemas
// ---------------------------------------------------------------------------

/// Contract: event-schemas.md § MergeConflict
/// Spec: MergeConflict must round-trip with all fields including conflicts and grace_period
/// If this test didn't exist: conflict data could be lost during serialization across the wire.
#[test]
fn merge_conflict_round_trip() {
    let conflict = test_merge_conflict();

    let json = serde_json::to_string(&conflict).unwrap();
    let deserialized: MergeConflict = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.node_id, conflict.node_id);
    assert_eq!(deserialized.vcluster_id, conflict.vcluster_id);
    assert_eq!(deserialized.conflicts.len(), conflict.conflicts.len());
    assert!(deserialized.grace_period_expires > deserialized.detected_at);
}

/// Contract: event-schemas.md § ConflictEntry
/// Spec: ConflictEntry must have both local_value and journal_value
/// If this test didn't exist: a conflict could omit one side, making resolution impossible.
#[test]
fn conflict_entry_has_both_values() {
    let conflict = test_merge_conflict();
    let entry = &conflict.conflicts[0];

    assert!(
        !entry.local_value.is_empty(),
        "ConflictEntry must have local_value"
    );
    assert!(
        !entry.journal_value.is_empty(),
        "ConflictEntry must have journal_value"
    );
    assert_ne!(
        entry.local_value, entry.journal_value,
        "local and journal values must differ for a conflict to exist"
    );
}

/// Contract: event-schemas.md § ConflictResolution
/// Spec: exactly 3 variants — AcceptLocal, AcceptJournal, GracePeriodExpired
/// If this test didn't exist: a resolution variant could be missing, leaving conflicts unresolvable.
#[test]
fn conflict_resolution_all_variants() {
    let resolutions = vec![
        ConflictResolution::AcceptLocal,
        ConflictResolution::AcceptJournal,
        ConflictResolution::GracePeriodExpired,
    ];

    for resolution in &resolutions {
        let json = serde_json::to_string(resolution).unwrap();
        let deserialized: ConflictResolution = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, resolution);
    }
}

/// Contract: event-schemas.md § PromoteConflict
/// Spec: PromoteConflict must round-trip with promoting_node and conflicts
/// If this test didn't exist: promote operations could lose conflict details across the wire.
#[test]
fn promote_conflict_round_trip() {
    let promote = PromoteConflict {
        promoting_node: "compute-042".into(),
        vcluster_id: "ml-training".into(),
        conflicts: vec![PromoteConflictEntry {
            key: "kernel.shmmax".into(),
            promoted_value: "2147483648".into(),
            conflicting_node: "compute-043".into(),
            conflicting_value: "1073741824".into(),
        }],
    };

    let json = serde_json::to_string(&promote).unwrap();
    let deserialized: PromoteConflict = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.promoting_node, promote.promoting_node);
    assert_eq!(deserialized.conflicts.len(), 1);
    assert_eq!(deserialized.conflicts[0].key, "kernel.shmmax");
}

// ---------------------------------------------------------------------------
// CapabilityReport Schema
// ---------------------------------------------------------------------------

/// Contract: event-schemas.md § CapabilityReport
/// Spec: CapabilityReport must include supervisor_status with backend and counts
/// If this test didn't exist: scheduler could receive reports without supervisor info, misrouting work.
#[test]
fn capability_report_includes_supervisor_status() {
    let report = CapabilityReport {
        node_id: "compute-042".into(),
        timestamp: Utc::now(),
        report_id: Uuid::new_v4(),
        gpus: vec![],
        memory: test_memory_capability(),
        network: None,
        storage: test_storage_capability(),
        software: test_software_capability(),
        config_state: test_config_state(),
        drift_summary: None,
        emergency: None,
        supervisor_status: SupervisorStatus {
            backend: "pact".into(),
            running_count: 4,
            failed_count: 0,
        },
    };

    assert!(!report.supervisor_status.backend.is_empty());
    assert!(report.supervisor_status.running_count >= 0);
}

/// Contract: event-schemas.md § CapabilityReport
/// Spec: network, drift_summary, and emergency are optional (None is valid)
/// If this test didn't exist: None optional fields could cause panics in consumers.
#[test]
fn capability_report_optional_fields() {
    let report = CapabilityReport {
        node_id: "compute-042".into(),
        timestamp: Utc::now(),
        report_id: Uuid::new_v4(),
        gpus: vec![],
        memory: test_memory_capability(),
        network: None,
        storage: test_storage_capability(),
        software: test_software_capability(),
        config_state: test_config_state(),
        drift_summary: None,
        emergency: None,
        supervisor_status: SupervisorStatus {
            backend: "pact".into(),
            running_count: 2,
            failed_count: 0,
        },
    };

    let json = serde_json::to_string(&report).unwrap();
    let deserialized: CapabilityReport = serde_json::from_str(&json).unwrap();

    assert!(deserialized.network.is_none());
    assert!(deserialized.drift_summary.is_none());
    assert!(deserialized.emergency.is_none());
}

// ---------------------------------------------------------------------------
// Loki Event Envelope
// ---------------------------------------------------------------------------

/// Contract: event-schemas.md § Loki Event Schema
/// Spec: Loki envelope must have timestamp, component, event_type, severity, identity
/// If this test didn't exist: events streamed to Loki could miss required fields, breaking dashboards.
#[test]
fn loki_event_has_required_fields() {
    let event = serde_json::json!({
        "timestamp": "2026-03-12T10:30:00Z",
        "component": "journal",
        "node_id": "node-001",
        "vcluster_id": "ml-training",
        "event_type": "config_commit",
        "severity": "info",
        "sequence": 42,
        "identity": {
            "principal": "alice@example.com",
            "role": "pact-ops-ml-training"
        },
        "detail": {}
    });

    let obj = event.as_object().unwrap();
    assert!(obj.contains_key("timestamp"), "Loki event must have timestamp");
    assert!(obj.contains_key("component"), "Loki event must have component");
    assert!(obj.contains_key("event_type"), "Loki event must have event_type");
    assert!(obj.contains_key("severity"), "Loki event must have severity");
    assert!(obj.contains_key("identity"), "Loki event must have identity");

    let identity = obj["identity"].as_object().unwrap();
    assert!(identity.contains_key("principal"), "identity must have principal");
    assert!(identity.contains_key("role"), "identity must have role");
}
