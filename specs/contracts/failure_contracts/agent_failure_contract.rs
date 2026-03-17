//! Contract tests for agent failure mode degradation.
//!
//! These test that failure modes F2, F3, F4, F5, F6 degrade as specified.
//!
//! Source: specs/failure-modes.md § F2, F3, F4, F5, F6

// ---------------------------------------------------------------------------
// F2: PolicyService unreachable (from agent perspective)
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F2
/// Spec: whitelist checks honored from local cache when PolicyService is unreachable
/// If this test didn't exist: agents could deny all operations during a journal outage,
/// blocking routine diagnostics on running nodes.
#[test]
fn f2_cached_whitelist_honored() {
    let cache = stub_policy_cache();

    let request = test_policy_request(
        Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        "status",
        Scope::VCluster("ml-training".into()),
    );

    let decision = cache.evaluate_degraded(&request);
    assert_matches!(decision, PolicyDecision::Allow { .. });
}

/// Contract: failure-modes.md § F2
/// Spec: two-person approval denied when PolicyService is unreachable (fail-closed)
/// If this test didn't exist: regulated operations could proceed without peer review
/// during a journal outage, bypassing compliance requirements.
#[test]
fn f2_two_person_denied_fail_closed() {
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

/// Contract: failure-modes.md § F2
/// Spec: platform admin authorized with cached role when PolicyService is unreachable
/// If this test didn't exist: platform admins would be locked out of emergency recovery
/// when the journal is down — exactly when they're needed most.
#[test]
fn f2_platform_admin_authorized_cached() {
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
// F3: Network partition (agent isolated from journal)
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F3
/// Spec: agent continues operating with cached config during partition
/// If this test didn't exist: a network partition could cause agent to stop
/// managing services, leaving the node unmanaged during the outage.
#[test]
fn f3_agent_continues_with_cached_config() {
    let agent = stub_agent_runtime();
    let cached_config = test_vcluster_config("ml-training");

    agent.load_cached_config(cached_config.clone());
    agent.simulate_journal_disconnect();

    let active_config = agent.effective_config().unwrap();
    assert_eq!(active_config.vcluster_id, "ml-training");
    assert!(agent.is_operational(), "agent must remain operational with cached config");
}

/// Contract: failure-modes.md § F3
/// Spec: drift detection continues locally during partition
/// If this test didn't exist: drift events during a partition would go undetected,
/// allowing uncontrolled state divergence without any local awareness.
#[test]
fn f3_drift_detection_continues_locally() {
    let agent = stub_agent_runtime();
    let observer = agent.state_observer();

    agent.simulate_journal_disconnect();

    // Simulate a drift event while partitioned
    observer.inject_event(test_drift_event("/etc/pact/agent.toml", DriftKind::FileModified));

    let drift_log = agent.local_drift_log();
    assert_eq!(drift_log.len(), 1);
    assert_eq!(drift_log[0].path, "/etc/pact/agent.toml");
}

/// Contract: failure-modes.md § F3
/// Spec: audit events logged locally for replay on reconnect
/// If this test didn't exist: operations during a partition would have no audit trail,
/// creating an unaccountable gap in the immutable log.
#[test]
fn f3_operations_logged_locally() {
    let agent = stub_agent_runtime();
    let shell = agent.shell_service();
    let caller = ops_identity("ml-training");

    agent.simulate_journal_disconnect();

    // Execute a command while partitioned
    let request = ExecRequest {
        command: "ps aux".into(),
        node_id: "compute-042".into(),
    };
    shell.exec(&caller, request).unwrap();

    let local_log = agent.local_audit_log();
    assert_eq!(local_log.len(), 1);
    assert_eq!(local_log[0].command, "ps aux");
    assert!(local_log[0].pending_replay, "event should be flagged for journal replay");
}

/// Contract: failure-modes.md § F3
/// Spec: config subscription resumes from from_sequence on reconnect
/// If this test didn't exist: a reconnecting agent would re-download the entire
/// config history, wasting bandwidth and delaying convergence.
#[test]
fn f3_config_subscription_resumes_from_sequence() {
    let journal = stub_journal_state();
    let identity = test_identity();

    journal.seed_entries(20);

    // Agent had received up to sequence 15 before partition
    let updates: Vec<_> = boot_config_service_subscribe(
        &journal, &identity, "ml-training", Some(15),
    ).take(5).collect();

    // Should only receive entries after sequence 15
    assert!(updates.iter().all(|u| u.sequence > 15));
}

// ---------------------------------------------------------------------------
// F4: Stale emergency
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F4
/// Spec: emergency exceeding configured window is flagged as stale
/// If this test didn't exist: an abandoned emergency session could keep auto-rollback
/// suspended indefinitely, leaving the node in uncommitted state forever.
#[test]
fn f4_stale_emergency_detected() {
    let mut cwm = stub_commit_window_manager();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    cwm.set_emergency_mode(true);

    // Advance time past emergency window
    let emergency_window = cwm.emergency_window_duration();
    cwm.advance_time(emergency_window + Duration::seconds(1));

    assert!(cwm.is_emergency_stale(), "emergency exceeding window must be flagged stale");
}

/// Contract: failure-modes.md § F4
/// Spec: force-ending a stale emergency requires pact-ops or platform-admin role
/// If this test didn't exist: a viewer or unauthorized user could force-end an
/// emergency session, potentially rolling back critical in-progress fixes.
#[test]
fn f4_force_end_requires_admin() {
    let mut cwm = stub_commit_window_manager();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    cwm.set_emergency_mode(true);

    // Viewer should not be able to force-end
    let viewer = Identity {
        principal: "viewer@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-viewer-ml-training".into(),
    };
    let result = cwm.force_end_emergency(&viewer);
    assert_matches!(result, Err(PactError::Unauthorized { .. }));

    // Ops admin should be able to force-end
    let ops = Identity {
        principal: "ops@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };
    let result = cwm.force_end_emergency(&ops);
    assert!(result.is_ok());
    assert!(!cwm.is_emergency_mode());
}

// ---------------------------------------------------------------------------
// F5: Rollback with active consumers
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F5
/// Spec: rollback blocked (not forced) when active consumers exist
/// If this test didn't exist: auto-rollback could forcibly unmount filesystems or
/// kill processes, crashing active workloads without warning.
#[test]
fn f5_rollback_fails_if_consumers_active() {
    let mut cwm = stub_commit_window_manager();
    let journal = stub_journal_client();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    cwm.register_consumer("lattice-job-12345");

    let result = cwm.rollback(&journal);
    assert_matches!(result, Err(PactError::ActiveConsumersExist { count: 1, .. }));

    // Window should still be open — not forcibly closed
    assert!(cwm.active_window.is_some());
}

/// Contract: failure-modes.md § F5
/// Spec: failed rollback attempt recorded in journal
/// If this test didn't exist: failed rollback attempts would be invisible,
/// making it impossible to diagnose why a node is stuck in drifted state.
#[test]
fn f5_failed_rollback_logged() {
    let mut cwm = stub_commit_window_manager();
    let journal = stub_journal_client();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    cwm.register_consumer("lattice-job-12345");

    let _ = cwm.rollback(&journal);

    let failed_entries = journal.failed_rollback_entries();
    assert_eq!(failed_entries.len(), 1);
    assert!(failed_entries[0].reason.contains("active consumers"));
}

// ---------------------------------------------------------------------------
// F6: Agent crash recovery
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F6
/// Spec: agent re-authenticates to journal after restart
/// If this test didn't exist: a restarted agent might attempt to use an expired
/// or invalid mTLS session, failing to reconnect silently.
#[test]
fn f6_agent_re_authenticates_on_restart() {
    let agent = stub_agent_runtime();

    // Simulate agent restart — previous mTLS session is gone
    agent.simulate_restart();

    let auth_result = agent.authenticate_to_journal();
    assert!(auth_result.is_ok(), "agent must re-authenticate after restart");
    assert!(agent.is_connected_to_journal());
}

/// Contract: failure-modes.md § F6
/// Spec: cached config used to reconcile services on restart
/// If this test didn't exist: a restarted agent could leave services in an
/// inconsistent state, with some running and others stopped, until the journal
/// delivers a fresh config.
#[test]
fn f6_cached_config_applied_on_restart() {
    let agent = stub_agent_runtime();
    let cached_config = test_vcluster_config("ml-training");

    agent.persist_config_cache(cached_config.clone());
    agent.simulate_restart();

    // Agent should load cached config and reconcile services
    let active_config = agent.effective_config().unwrap();
    assert_eq!(active_config.vcluster_id, "ml-training");

    let service_states = agent.service_states();
    assert!(service_states.iter().all(|s| s.state == ServiceState::Running || s.state == ServiceState::Stopped));
}
