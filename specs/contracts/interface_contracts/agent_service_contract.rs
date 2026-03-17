//! Contract tests for agent subsystem interfaces.
//!
//! These tests verify the internal integration surfaces within pact-agent:
//! - ServiceManager (process supervision)
//! - GpuBackend (hardware detection)
//! - StateObserver (drift event emission)
//! - DriftEvaluator (drift vector computation)
//! - CommitWindowManager (commit window lifecycle)
//! - ShellService (remote command execution)
//!
//! All tests reference their source contract and spec requirement.
//! Tests use stubs — they define WHAT the interface must do,
//! not HOW it does it internally.

// ---------------------------------------------------------------------------
// ServiceManager trait contracts
// ---------------------------------------------------------------------------

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: process_supervisor.feature scenario 16 — start logs lifecycle entry
/// If this test didn't exist: a service could start without an audit trail.
#[test]
fn start_logs_service_lifecycle_to_journal() {
    let manager = stub_service_manager();
    let journal = stub_journal_client();
    let service = test_service_decl("chronyd", 1);

    manager.start(&service).unwrap();

    let entries = journal.lifecycle_entries_for("chronyd");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].event_type, ServiceLifecycleEvent::Started);
    assert!(entries[0].timestamp > Utc::now() - Duration::seconds(1));
}

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: process_supervisor.feature scenario 12 — stop uses reverse dependency order
/// If this test didn't exist: dependent services could be killed before their dependees,
/// causing data loss or unclean shutdown.
#[test]
fn stop_uses_reverse_dependency_order() {
    let manager = stub_service_manager();
    let services = vec![
        test_service_decl("chronyd", 1),
        test_service_decl("nvidia-persistenced", 2),
        test_service_decl("lattice-node-agent", 3),
    ];

    // Start in order
    for s in &services {
        manager.start(s).unwrap();
    }

    // Stop all
    manager.stop_all().unwrap();

    let stop_order = manager.stop_order_log();
    assert_eq!(stop_order, vec!["lattice-node-agent", "nvidia-persistenced", "chronyd"]);
}

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: process_supervisor.feature scenarios 7-10 — RestartPolicy::Always
/// If this test didn't exist: a crashed critical service would stay down.
#[test]
fn restart_respects_restart_policy_always() {
    let manager = stub_service_manager();
    let mut service = test_service_decl("lattice-node-agent", 1);
    service.restart_policy = RestartPolicy::Always;

    manager.start(&service).unwrap();
    manager.simulate_exit(&service, ExitStatus::Code(1));

    let status = manager.status(&service).unwrap();
    assert_eq!(status.state, ServiceState::Running);
    assert_eq!(status.restart_count, 1);
}

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: process_supervisor.feature scenarios 7-10 — RestartPolicy::Never
/// If this test didn't exist: a one-shot service might respawn endlessly.
#[test]
fn restart_respects_restart_policy_never() {
    let manager = stub_service_manager();
    let mut service = test_service_decl("migration-job", 1);
    service.restart_policy = RestartPolicy::Never;

    manager.start(&service).unwrap();
    manager.simulate_exit(&service, ExitStatus::Code(0));

    let status = manager.status(&service).unwrap();
    assert_eq!(status.state, ServiceState::Stopped);
    assert_eq!(status.restart_count, 0);
}

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: process_supervisor.feature scenarios 7-10 — RestartPolicy::OnFailure
/// If this test didn't exist: a clean exit could trigger unnecessary restarts,
/// or a crash could leave a service permanently down.
#[test]
fn restart_respects_restart_policy_on_failure() {
    let manager = stub_service_manager();
    let mut service = test_service_decl("metrics-exporter", 1);
    service.restart_policy = RestartPolicy::OnFailure;

    // Clean exit — should NOT restart
    manager.start(&service).unwrap();
    manager.simulate_exit(&service, ExitStatus::Code(0));
    let status = manager.status(&service).unwrap();
    assert_eq!(status.state, ServiceState::Stopped);
    assert_eq!(status.restart_count, 0);

    // Failure exit — SHOULD restart
    manager.start(&service).unwrap();
    manager.simulate_exit(&service, ExitStatus::Code(137));
    let status = manager.status(&service).unwrap();
    assert_eq!(status.state, ServiceState::Running);
    assert_eq!(status.restart_count, 1);
}

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: ServiceInstance tracks pid, uptime, restart_count
/// If this test didn't exist: status could return incomplete process info.
#[test]
fn status_returns_service_instance_with_pid() {
    let manager = stub_service_manager();
    let service = test_service_decl("chronyd", 1);

    manager.start(&service).unwrap();

    let instance = manager.status(&service).unwrap();
    assert!(instance.pid > 0);
    assert!(instance.uptime >= Duration::zero());
    assert_eq!(instance.restart_count, 0);
    assert_eq!(instance.state, ServiceState::Running);
}

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: health check — process type checks PID existence
/// If this test didn't exist: a zombie PID could pass health checks.
#[test]
fn health_check_process_type() {
    let manager = stub_service_manager();
    let mut service = test_service_decl("chronyd", 1);
    service.health_check = HealthCheckType::Process;

    manager.start(&service).unwrap();
    assert!(manager.health(&service).unwrap());

    manager.simulate_exit(&service, ExitStatus::Code(1));
    // With RestartPolicy::Never to observe dead state
    service.restart_policy = RestartPolicy::Never;
    assert!(!manager.health(&service).unwrap());
}

/// Contract: agent-interfaces.md § ServiceManager
/// Spec: A6 — services start in dependency order (order field)
/// If this test didn't exist: services could race, breaking dependencies like
/// lattice-node-agent starting before time sync.
#[test]
fn start_respects_dependency_ordering() {
    let manager = stub_service_manager();
    let services = vec![
        test_service_decl("chronyd", 1),
        test_service_decl("nvidia-persistenced", 2),
        test_service_decl("lattice-node-agent", 3),
    ];

    manager.start_all(&services).unwrap();

    let start_order = manager.start_order_log();
    assert_eq!(start_order, vec!["chronyd", "nvidia-persistenced", "lattice-node-agent"]);
}

// ---------------------------------------------------------------------------
// GpuBackend trait contracts
// ---------------------------------------------------------------------------

/// Contract: agent-interfaces.md § GpuBackend
/// Spec: capability_reporting.feature — detect returns GPU capabilities
/// If this test didn't exist: GPU info could be silently wrong or missing fields.
#[test]
fn detect_returns_gpu_capabilities() {
    let backend = stub_gpu_backend(vec![
        GpuCapability {
            vendor: GpuVendor::Nvidia,
            model: "A100".into(),
            memory_bytes: 80 * 1024 * 1024 * 1024,
            health: GpuHealth::Healthy,
        },
    ]);

    let gpus = backend.detect().unwrap();
    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0].vendor, GpuVendor::Nvidia);
    assert_eq!(gpus[0].model, "A100");
    assert_eq!(gpus[0].memory_bytes, 80 * 1024 * 1024 * 1024);
    assert_eq!(gpus[0].health, GpuHealth::Healthy);
}

/// Contract: agent-interfaces.md § GpuBackend
/// Spec: capability_reporting.feature — graceful empty on non-GPU node
/// If this test didn't exist: detection on a non-GPU node might panic or error.
#[test]
fn detect_returns_empty_on_no_gpus() {
    let backend = stub_gpu_backend(vec![]);

    let gpus = backend.detect().unwrap();
    assert!(gpus.is_empty());
}

// ---------------------------------------------------------------------------
// StateObserver trait contracts
// ---------------------------------------------------------------------------

/// Contract: agent-interfaces.md § StateObserver
/// Spec: drift_detection.feature — events flow through mpsc channel
/// If this test didn't exist: observer output could be silently dropped.
#[test]
fn observer_emits_drift_events_through_channel() {
    let (tx, rx) = mpsc::channel(64);
    let observer = stub_state_observer(vec![
        test_drift_event("/etc/pact/agent.toml", DriftKind::FileModified),
    ]);

    observer.start(tx).unwrap();

    let event = rx.blocking_recv().unwrap();
    assert_eq!(event.path, "/etc/pact/agent.toml");
    assert_eq!(event.kind, DriftKind::FileModified);
}

/// Contract: agent-interfaces.md § StateObserver
/// Spec: D1 — blacklisted paths filtered before emission
/// If this test didn't exist: changes to /proc or /sys could flood the evaluator.
#[test]
fn observer_filters_blacklisted_paths() {
    let (tx, rx) = mpsc::channel(64);
    let observer = stub_state_observer_with_blacklist(
        vec![
            test_drift_event("/proc/stat", DriftKind::FileModified),
            test_drift_event("/etc/pact/agent.toml", DriftKind::FileModified),
        ],
        vec!["/proc/".into()],
    );

    observer.start(tx).unwrap();

    let event = rx.blocking_recv().unwrap();
    assert_eq!(event.path, "/etc/pact/agent.toml");
    // No second event — /proc/stat was filtered
    assert!(rx.try_recv().is_err());
}

/// Contract: agent-interfaces.md § StateObserver
/// Spec: multiple observers compose — concurrent observers, single consumer
/// If this test didn't exist: events from different observers could be lost or duplicated.
#[test]
fn multiple_observers_feed_same_evaluator() {
    let (tx, rx) = mpsc::channel(64);
    let inotify_observer = stub_state_observer(vec![
        test_drift_event("/etc/pact/agent.toml", DriftKind::FileModified),
    ]);
    let netlink_observer = stub_state_observer(vec![
        test_drift_event("eth0", DriftKind::InterfaceDown),
    ]);

    inotify_observer.start(tx.clone()).unwrap();
    netlink_observer.start(tx).unwrap();

    let mut events = vec![];
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    assert_eq!(events.len(), 2);
    let paths: Vec<&str> = events.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"/etc/pact/agent.toml"));
    assert!(paths.contains(&"eth0"));
}

// ---------------------------------------------------------------------------
// DriftEvaluator contracts
// ---------------------------------------------------------------------------

/// Contract: agent-interfaces.md § DriftEvaluator
/// Spec: D1 — blacklist match returns None
/// If this test didn't exist: blacklisted paths would generate drift vectors,
/// triggering spurious commit windows.
#[test]
fn evaluate_returns_none_for_blacklisted() {
    let evaluator = stub_drift_evaluator(
        vec!["/proc/".into(), "/sys/".into()],
        default_drift_weights(),
    );
    let event = test_drift_event("/proc/meminfo", DriftKind::FileModified);

    let result = evaluator.evaluate(&event);
    assert!(result.is_none());
}

/// Contract: agent-interfaces.md § DriftEvaluator
/// Spec: D1 — non-blacklisted event returns DriftVector
/// If this test didn't exist: valid drift events could be silently discarded.
#[test]
fn evaluate_returns_drift_vector_for_valid_event() {
    let evaluator = stub_drift_evaluator(
        vec!["/proc/".into()],
        default_drift_weights(),
    );
    let event = test_drift_event("/etc/pact/agent.toml", DriftKind::FileModified);

    let result = evaluator.evaluate(&event);
    assert!(result.is_some());
    let vector = result.unwrap();
    assert!(!vector.dimensions.is_empty());
}

/// Contract: agent-interfaces.md § DriftEvaluator
/// Spec: D3/D4 — magnitude uses weighted Euclidean norm: sqrt(sum(weight_i * dim_i)^2)
/// If this test didn't exist: magnitude formula could silently regress.
#[test]
fn magnitude_uses_weighted_euclidean_norm() {
    let evaluator = stub_drift_evaluator(vec![], default_drift_weights());
    let vector = DriftVector {
        dimensions: vec![
            DriftDimension { name: "config".into(), value: 3.0 },
            DriftDimension { name: "network".into(), value: 4.0 },
        ],
    };

    // weights: config=1.0, network=1.0 → sqrt((1*3)^2 + (1*4)^2) = sqrt(9+16) = 5.0
    let mag = evaluator.magnitude(&vector);
    assert!((mag - 5.0).abs() < f64::EPSILON);
}

/// Contract: agent-interfaces.md § DriftEvaluator
/// Spec: D3 — magnitude is always non-negative
/// If this test didn't exist: negative magnitudes could invert commit window timing.
#[test]
fn magnitude_non_negative() {
    let evaluator = stub_drift_evaluator(vec![], default_drift_weights());

    // Zero vector
    let zero = DriftVector { dimensions: vec![] };
    assert!(evaluator.magnitude(&zero) >= 0.0);

    // Negative dimension values still produce non-negative magnitude
    let negative = DriftVector {
        dimensions: vec![
            DriftDimension { name: "config".into(), value: -5.0 },
        ],
    };
    assert!(evaluator.magnitude(&negative) >= 0.0);
}

/// Contract: agent-interfaces.md § DriftEvaluator
/// Spec: D4 — weight=0 means dimension is ignored
/// If this test didn't exist: a zero-weighted dimension could still affect magnitude,
/// causing unexpected commit window timing.
#[test]
fn zero_weight_ignores_dimension() {
    let mut weights = default_drift_weights();
    weights.insert("network".into(), 0.0);

    let evaluator = stub_drift_evaluator(vec![], weights);
    let vector = DriftVector {
        dimensions: vec![
            DriftDimension { name: "config".into(), value: 3.0 },
            DriftDimension { name: "network".into(), value: 999.0 },
        ],
    };

    // Only config contributes: sqrt((1*3)^2) = 3.0
    let mag = evaluator.magnitude(&vector);
    assert!((mag - 3.0).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// CommitWindowManager contracts
// ---------------------------------------------------------------------------

/// Contract: agent-interfaces.md § CommitWindowManager
/// Spec: A1 — at most one active window
/// If this test didn't exist: concurrent commit windows could lead to conflicting
/// rollback decisions.
#[test]
fn open_creates_single_window() {
    let mut cwm = stub_commit_window_manager();
    let drift = test_drift_vector(1.0);

    let window = cwm.open(&drift, 1.0);
    assert!(window.is_active());
    assert!(cwm.active_window.is_some());
}

/// Contract: agent-interfaces.md § CommitWindowManager
/// Spec: A1 — new drift extends existing window, does not create new
/// If this test didn't exist: a second drift event could replace the first window,
/// losing the original state snapshot.
#[test]
fn open_extends_existing_window_on_new_drift() {
    let mut cwm = stub_commit_window_manager();
    let drift1 = test_drift_vector(1.0);
    let drift2 = test_drift_vector(2.0);

    let window1_id = cwm.open(&drift1, 1.0).id;
    let window2_id = cwm.open(&drift2, 2.0).id;

    assert_eq!(window1_id, window2_id, "should extend, not create new window");
    assert_eq!(cwm.window_count(), 1);
}

/// Contract: agent-interfaces.md § CommitWindowManager
/// Spec: commit closes window and records in journal
/// If this test didn't exist: committed state could linger as an open window
/// or be missing from the audit log.
#[test]
fn commit_closes_window_and_records_in_journal() {
    let mut cwm = stub_commit_window_manager();
    let journal = stub_journal_client();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    cwm.commit(&journal).unwrap();

    assert!(cwm.active_window.is_none());
    assert_eq!(journal.commit_entries().len(), 1);
}

/// Contract: agent-interfaces.md § CommitWindowManager
/// Spec: A5 — rollback blocked if active consumers exist
/// If this test didn't exist: rollback during active workloads could crash running jobs.
#[test]
fn rollback_checks_active_consumers() {
    let mut cwm = stub_commit_window_manager();
    let journal = stub_journal_client();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    cwm.register_consumer("lattice-job-12345");

    let result = cwm.rollback(&journal);
    assert_matches!(result, Err(PactError::ActiveConsumersExist { count: 1, .. }));
}

/// Contract: agent-interfaces.md § CommitWindowManager
/// Spec: A4 — expired window triggers auto-rollback
/// If this test didn't exist: an expired window could remain open indefinitely,
/// leaving the node in uncommitted state.
#[test]
fn tick_auto_rollback_on_expiry() {
    let mut cwm = stub_commit_window_manager();
    let journal = stub_journal_client();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    let expiry = cwm.active_window.as_ref().unwrap().expires_at;

    // Tick past expiry
    cwm.tick(expiry + Duration::seconds(1), &journal).unwrap();

    assert!(cwm.active_window.is_none());
    assert_eq!(journal.rollback_entries().len(), 1);
}

/// Contract: agent-interfaces.md § CommitWindowManager
/// Spec: A4 exception — emergency mode suspends auto-rollback
/// If this test didn't exist: auto-rollback during an emergency could revert
/// a critical fix while admin is still working.
#[test]
fn tick_emergency_suspends_auto_rollback() {
    let mut cwm = stub_commit_window_manager();
    let journal = stub_journal_client();
    let drift = test_drift_vector(1.0);

    cwm.open(&drift, 1.0);
    cwm.set_emergency_mode(true);
    let expiry = cwm.active_window.as_ref().unwrap().expires_at;

    // Tick past expiry — should NOT auto-rollback
    cwm.tick(expiry + Duration::seconds(1), &journal).unwrap();

    assert!(cwm.active_window.is_some(), "emergency mode should suspend auto-rollback");
    assert!(journal.rollback_entries().is_empty());
}

/// Contract: agent-interfaces.md § CommitWindowManager
/// Spec: A3 — window duration formula base/(1+mag*sens) always positive
/// If this test didn't exist: edge-case magnitude or sensitivity could produce
/// zero or negative window duration, making commit impossible.
#[test]
fn window_formula_always_positive() {
    let mut cwm = stub_commit_window_manager();

    // Test with extreme magnitudes
    for magnitude in [0.0, 0.001, 1.0, 100.0, f64::MAX / 2.0] {
        let drift = test_drift_vector(magnitude);
        let window = cwm.open(&drift, magnitude);
        assert!(
            window.duration_secs() > 0.0,
            "window duration must be positive for magnitude {}",
            magnitude,
        );
        cwm.force_close(); // Reset for next iteration
    }
}

/// Contract: agent-interfaces.md § StateObserver / CommitWindowManager
/// Spec: D5 — observe-only mode logs events but does not open a commit window
/// If this test didn't exist: observe-only bootstrap phase could accidentally
/// trigger rollbacks on a newly booted node.
#[test]
fn observe_only_mode_does_not_open_window() {
    let mut cwm = stub_commit_window_manager();
    cwm.set_observe_only(true);

    let drift = test_drift_vector(5.0);
    cwm.on_drift(&drift, 5.0);

    assert!(cwm.active_window.is_none());
    assert!(cwm.observed_drift_log().len() > 0, "drift should still be logged");
}

// ---------------------------------------------------------------------------
// ShellService contracts
// ---------------------------------------------------------------------------

/// Contract: agent-interfaces.md § ShellService
/// Spec: S1 — whitelist enforcement on exec
/// If this test didn't exist: arbitrary commands could be executed on nodes,
/// bypassing the restricted command model.
#[test]
fn exec_rejects_non_whitelisted_command() {
    let shell = stub_shell_service(vec!["ps".into(), "top".into(), "df".into()]);
    let caller = ops_identity("ml-training");

    let request = ExecRequest {
        command: "rm -rf /".into(),
        node_id: "compute-042".into(),
    };

    let result = shell.exec(&caller, request);
    assert_matches!(result, Err(PactError::CommandNotWhitelisted { command, .. }) if command == "rm");
}

/// Contract: agent-interfaces.md § ShellService
/// Spec: S2 — platform admin bypasses whitelist, still logged
/// If this test didn't exist: platform admins would be locked out of emergency
/// diagnostics, or their bypass wouldn't be auditable.
#[test]
fn exec_allows_platform_admin_bypass() {
    let shell = stub_shell_service(vec!["ps".into()]);
    let journal = stub_journal_client();
    let caller = platform_admin();

    let request = ExecRequest {
        command: "strace -p 1".into(),
        node_id: "compute-042".into(),
    };

    let result = shell.exec_with_journal(&caller, request, &journal);
    assert!(result.is_ok());

    // Must still be logged despite bypass
    let log = journal.exec_entries();
    assert_eq!(log.len(), 1);
    assert!(log[0].whitelist_bypassed);
}

/// Contract: agent-interfaces.md § ShellService
/// Spec: S3 — shell sessions use restricted bash (rbash)
/// If this test didn't exist: an interactive session could escape to unrestricted bash.
#[test]
fn shell_uses_restricted_bash() {
    let shell = stub_shell_service(vec![]);
    let caller = ops_identity("ml-training");

    let session = shell.open_session(&caller, "compute-042").unwrap();
    assert_eq!(session.shell_binary, "/bin/rbash");
    assert!(session.env.contains_key("PATH"));
    // PATH should be restricted to whitelist directory
    assert!(session.env["PATH"].starts_with("/usr/lib/pact/shell-bin"));
}

/// Contract: agent-interfaces.md § ShellService
/// Spec: S4 — every command logged to journal
/// If this test didn't exist: exec could silently bypass the audit trail.
#[test]
fn exec_logs_to_journal() {
    let shell = stub_shell_service(vec!["ps".into()]);
    let journal = stub_journal_client();
    let caller = ops_identity("ml-training");

    let request = ExecRequest {
        command: "ps aux".into(),
        node_id: "compute-042".into(),
    };

    shell.exec_with_journal(&caller, request, &journal).unwrap();

    let log = journal.exec_entries();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].command, "ps aux");
    assert_eq!(log[0].caller, "user@example.com");
    assert_eq!(log[0].node_id, "compute-042");
    assert!(log[0].timestamp > Utc::now() - Duration::seconds(1));
}

/// Contract: agent-interfaces.md § ShellService
/// Spec: S5 — state-changing commands trigger commit window
/// If this test didn't exist: state changes via exec would bypass drift detection
/// and commit window lifecycle.
#[test]
fn state_changing_exec_opens_commit_window() {
    let shell = stub_shell_service(vec!["systemctl".into()]);
    let journal = stub_journal_client();
    let cwm = stub_commit_window_manager();
    let caller = ops_identity("ml-training");

    let request = ExecRequest {
        command: "systemctl restart chronyd".into(),
        node_id: "compute-042".into(),
    };

    shell.exec_with_drift(&caller, request, &journal, &cwm).unwrap();

    // Drift observer detects the state change, commit window should open
    assert!(cwm.active_window.is_some());
}
