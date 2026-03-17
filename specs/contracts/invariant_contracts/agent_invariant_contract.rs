//! Contract tests for agent and drift invariants.
//!
//! These test that the enforcement mechanisms identified in enforcement-map.md
//! actually prevent invariant violations.
//!
//! Source: specs/invariants.md § Agent Invariants (A1-A10) and Drift Invariants (D1-D5)

// ---------------------------------------------------------------------------
// A1: At most one commit window per node
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A1
/// Spec: invariants.md § A1 — at most one active commit window at any time
/// If this test didn't exist: a node could accumulate multiple concurrent commit
/// windows, leading to ambiguous rollback targets.
#[test]
fn a1_at_most_one_commit_window() {
    let manager = stub_commit_window_manager();
    let drift1 = test_drift_event("files", 0.5);

    // First drift opens a window
    let result1 = manager.on_drift(drift1);
    assert!(result1.is_ok());
    assert!(manager.active_window().is_some(), "first drift should open a window");

    // Second drift on a different dimension must NOT open a second window
    let drift2 = test_drift_event("network", 0.3);
    let result2 = manager.on_drift(drift2);
    assert!(result2.is_ok());

    // Still exactly one window — Option<CommitWindow> enforces at-most-one
    assert_eq!(manager.window_count(), 1,
        "must have exactly one commit window, not two");
}

/// Contract: enforcement-map.md § A1
/// Spec: invariants.md § A1 — new drift extends existing window
/// If this test didn't exist: new drift could silently drop or create a parallel window.
#[test]
fn a1_new_drift_extends_existing_window() {
    let manager = stub_commit_window_manager();
    let drift1 = test_drift_event("files", 0.5);
    manager.on_drift(drift1).unwrap();

    let original_deadline = manager.active_window().unwrap().deadline;

    // Second drift should extend the existing window
    let drift2 = test_drift_event("kernel", 0.8);
    manager.on_drift(drift2).unwrap();

    let extended_deadline = manager.active_window().unwrap().deadline;
    assert!(extended_deadline >= original_deadline,
        "new drift must extend (or maintain) the existing window deadline");
    assert_eq!(manager.window_count(), 1,
        "still exactly one commit window after extension");
}

// ---------------------------------------------------------------------------
// A2: At most one emergency session per node
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A2
/// Spec: invariants.md § A2 — second emergency rejected with EmergencyActive
/// If this test didn't exist: overlapping emergency sessions could cause
/// conflicting audit trails and inconsistent rollback suspension.
#[test]
fn a2_at_most_one_emergency_session() {
    let session = stub_emergency_session();
    let caller = test_identity("admin@example.com", "pact-platform-admin");

    // First emergency succeeds
    let result1 = session.enter(&caller, "disk failure investigation");
    assert!(result1.is_ok());

    // Second emergency rejected
    let result2 = session.enter(&caller, "another issue");
    assert_matches!(result2, Err(PactError::EmergencyActive { .. }),
        "second emergency must be rejected while one is active");
}

// ---------------------------------------------------------------------------
// A3: Commit window formula
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A3
/// Spec: invariants.md § A3 — base/(1+mag*sens) > 0 for all valid inputs
/// If this test didn't exist: edge cases could produce zero or negative windows.
#[test]
fn a3_commit_window_formula_always_positive() {
    let evaluator = stub_drift_evaluator();

    // Sample a range of valid inputs
    let base_values = [1.0, 60.0, 300.0, 3600.0];
    let magnitudes = [0.001, 0.1, 0.5, 1.0, 5.0, 100.0];
    let sensitivities = [0.0, 0.1, 0.5, 1.0, 2.0, 10.0];

    for &base in &base_values {
        for &mag in &magnitudes {
            for &sens in &sensitivities {
                let window_secs = evaluator.compute_window(base, mag, sens);
                assert!(window_secs > 0.0,
                    "window must be positive: base={}, mag={}, sens={} → {}",
                    base, mag, sens, window_secs);
            }
        }
    }
}

/// Contract: enforcement-map.md § A3
/// Spec: invariants.md § A3 — boundary values for commit window formula
/// If this test didn't exist: boundary conditions could silently produce wrong results.
#[test]
fn a3_commit_window_formula_boundary_values() {
    let evaluator = stub_drift_evaluator();

    // magnitude=0 → window = base/(1+0) = base
    let window = evaluator.compute_window(300.0, 0.0, 1.0);
    assert!((window - 300.0).abs() < f64::EPSILON,
        "zero magnitude should return base window, got {}", window);

    // sensitivity=0 → window = base/(1+0) = base regardless of magnitude
    let window = evaluator.compute_window(300.0, 100.0, 0.0);
    assert!((window - 300.0).abs() < f64::EPSILON,
        "zero sensitivity should return base window, got {}", window);

    // Large magnitude + large sensitivity → small but positive window
    let window = evaluator.compute_window(300.0, 1000.0, 1000.0);
    assert!(window > 0.0, "large inputs must still produce positive window");
    assert!(window < 1.0, "large inputs should produce a very small window");
}

// ---------------------------------------------------------------------------
// A4: Auto-rollback on window expiry
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A4
/// Spec: invariants.md § A4 — expired window triggers rollback
/// If this test didn't exist: expired windows could leave the node in undeclared state.
#[test]
fn a4_auto_rollback_on_window_expiry() {
    let manager = stub_commit_window_manager();
    let drift = test_drift_event("files", 0.5);
    manager.on_drift(drift).unwrap();

    // Simulate time passing beyond the window deadline
    manager.advance_time_past_deadline();

    let tick_result = manager.tick();
    assert!(tick_result.rollback_triggered,
        "expired commit window must trigger auto-rollback");
    assert!(manager.active_window().is_none(),
        "window should be cleared after rollback");
}

/// Contract: enforcement-map.md § A4
/// Spec: invariants.md § A4 — emergency suspends auto-rollback
/// If this test didn't exist: emergency investigations could be interrupted
/// by auto-rollback, losing diagnostic state.
#[test]
fn a4_emergency_suspends_auto_rollback() {
    let manager = stub_commit_window_manager();
    let session = stub_emergency_session();
    let caller = test_identity("admin@example.com", "pact-platform-admin");

    // Open a commit window
    let drift = test_drift_event("files", 0.5);
    manager.on_drift(drift).unwrap();

    // Enter emergency mode
    session.enter(&caller, "investigating issue").unwrap();

    // Advance past deadline
    manager.advance_time_past_deadline();

    let tick_result = manager.tick_with_emergency(&session);
    assert!(!tick_result.rollback_triggered,
        "auto-rollback must NOT trigger during emergency mode");
}

// ---------------------------------------------------------------------------
// A5: Active consumer check before rollback
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A5
/// Spec: invariants.md § A5 — consumers holding resources blocks rollback
/// If this test didn't exist: rollback could unmount filesystems with open file
/// handles, causing data corruption or process crashes.
#[test]
fn a5_rollback_blocked_by_active_consumers() {
    let manager = stub_commit_window_manager();
    let drift = test_drift_event("mounts", 0.5);
    manager.on_drift(drift).unwrap();

    // Simulate active consumer holding a resource
    manager.register_consumer("lattice-node-agent", "/mnt/scratch");

    manager.advance_time_past_deadline();

    let tick_result = manager.tick();
    assert!(!tick_result.rollback_triggered,
        "rollback must not proceed while consumers hold resources");
    assert_matches!(tick_result.blocked_reason, Some(BlockedReason::ActiveConsumers { .. }),
        "reason must indicate active consumers");
}

// ---------------------------------------------------------------------------
// A6: Service dependency ordering
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A6
/// Spec: invariants.md § A6 — services sorted by order field
/// If this test didn't exist: services could start in arbitrary order, breaking
/// dependencies (e.g., lattice before time sync).
#[test]
fn a6_services_start_in_order() {
    let services = vec![
        test_service_decl("lattice-node-agent", 40),
        test_service_decl("chronyd", 10),
        test_service_decl("nvidia-persistenced", 20),
        test_service_decl("metrics-exporter", 30),
    ];

    let start_order = compute_start_order(&services);
    let names: Vec<&str> = start_order.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["chronyd", "nvidia-persistenced", "metrics-exporter", "lattice-node-agent"],
        "services must start in ascending order");
}

/// Contract: enforcement-map.md § A6
/// Spec: invariants.md § A6 — shutdown in reverse order
/// If this test didn't exist: shutdown could kill dependencies before dependents,
/// causing ungraceful termination.
#[test]
fn a6_services_stop_in_reverse_order() {
    let services = vec![
        test_service_decl("chronyd", 10),
        test_service_decl("nvidia-persistenced", 20),
        test_service_decl("metrics-exporter", 30),
        test_service_decl("lattice-node-agent", 40),
    ];

    let stop_order = compute_stop_order(&services);
    let names: Vec<&str> = stop_order.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["lattice-node-agent", "metrics-exporter", "nvidia-persistenced", "chronyd"],
        "services must stop in reverse (descending) order");
}

// ---------------------------------------------------------------------------
// A9: Cached config during partition
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A9
/// Spec: invariants.md § A9 — agent uses ConfigCache when journal unreachable
/// If this test didn't exist: a network partition could leave nodes with no
/// configuration, effectively bricking them.
#[test]
fn a9_cached_config_during_partition() {
    let cache = test_config_cache();

    // Populate cache with known-good config
    cache.store(test_overlay("ml-training", 42));

    // Simulate journal unreachable
    let journal = stub_unreachable_journal();

    let config = resolve_config(&journal, &cache);
    assert!(config.is_ok(), "agent must succeed with cached config during partition");
    assert_eq!(config.unwrap().version, 42,
        "cached config must reflect the last known-good version");
}

// ---------------------------------------------------------------------------
// A10: Emergency does not expand whitelist
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § A10
/// Spec: invariants.md § A10 — emergency doesn't add commands to whitelist
/// If this test didn't exist: emergency mode could be used as a privilege
/// escalation vector to run arbitrary commands.
#[test]
fn a10_emergency_does_not_expand_whitelist() {
    let session = stub_emergency_session();
    let whitelist = stub_whitelist();
    let caller = test_identity("admin@example.com", "pact-ops-ml-training");

    let whitelist_before: Vec<String> = whitelist.allowed_commands();

    session.enter(
        &test_identity("admin@example.com", "pact-platform-admin"),
        "investigating issue",
    ).unwrap();

    let whitelist_during: Vec<String> = whitelist.allowed_commands();
    assert_eq!(whitelist_before, whitelist_during,
        "emergency mode must NOT add commands to the whitelist");

    // Verify a non-whitelisted command is still rejected during emergency
    let result = whitelist.check("rm -rf /", &caller);
    assert_matches!(result, Err(PactError::CommandNotWhitelisted { .. }),
        "non-whitelisted command must still be rejected during emergency");
}

// ---------------------------------------------------------------------------
// D1: Blacklist exclusion
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § D1
/// Spec: invariants.md § D1 — default blacklist paths produce no drift
/// If this test didn't exist: changes in /tmp or /proc could trigger false
/// drift alerts on every node.
#[test]
fn d1_blacklist_exclusion_default_paths() {
    let evaluator = stub_drift_evaluator();

    let blacklisted_paths = [
        "/tmp/scratch/data.bin",
        "/var/log/syslog",
        "/proc/1/status",
        "/sys/class/net/eth0/speed",
        "/dev/null",
        "/run/user/1000/bus",
    ];

    for path in &blacklisted_paths {
        let drift = evaluator.evaluate_file_change(path);
        assert!(drift.is_none(),
            "blacklisted path {} must not produce drift", path);
    }
}

/// Contract: enforcement-map.md § D1
/// Spec: invariants.md § D1 — non-blacklisted path produces drift
/// If this test didn't exist: the blacklist could silently suppress all drift.
#[test]
fn d1_non_blacklisted_path_produces_drift() {
    let evaluator = stub_drift_evaluator();

    let result = evaluator.evaluate_file_change("/etc/hosts");
    assert!(result.is_some(),
        "/etc/hosts is not blacklisted and must produce drift");
}

// ---------------------------------------------------------------------------
// D2: Seven dimensions
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § D2
/// Spec: invariants.md § D2 — DriftDimension enum has exactly 7 variants
/// If this test didn't exist: adding or removing a dimension without updating
/// all consumers would cause silent bugs in magnitude calculation.
#[test]
fn d2_exactly_seven_dimensions() {
    let all_dimensions = DriftDimension::all_variants();
    assert_eq!(all_dimensions.len(), 7,
        "DriftDimension must have exactly 7 variants, found {}", all_dimensions.len());

    // Verify the exact set
    let expected = ["mounts", "files", "network", "services", "kernel", "packages", "gpu"];
    let names: Vec<&str> = all_dimensions.iter().map(|d| d.name()).collect();
    for exp in &expected {
        assert!(names.contains(exp),
            "missing expected drift dimension: {}", exp);
    }
}

// ---------------------------------------------------------------------------
// D3: Non-negative magnitudes
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § D3
/// Spec: invariants.md § D3 — weighted norm always >= 0
/// If this test didn't exist: negative drift magnitudes could invert commit
/// window calculations or suppress rollback.
#[test]
fn d3_magnitude_non_negative() {
    let evaluator = stub_drift_evaluator();

    // Test with various drift vectors
    let vectors = vec![
        test_drift_vector([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        test_drift_vector([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        test_drift_vector([0.5, 0.3, 0.1, 0.0, 0.8, 0.0, 0.2]),
        test_drift_vector([1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
    ];

    for vector in &vectors {
        let magnitude = evaluator.compute_magnitude(vector);
        assert!(magnitude >= 0.0,
            "drift magnitude must be non-negative, got {} for {:?}", magnitude, vector);
    }
}

/// Contract: enforcement-map.md § D3
/// Spec: invariants.md § D3 — zero drift produces zero magnitude
/// If this test didn't exist: zero vector could produce nonzero magnitude due
/// to floating point artifacts.
#[test]
fn d3_all_zero_magnitude_is_zero() {
    let evaluator = stub_drift_evaluator();
    let zero_vector = test_drift_vector([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);

    let magnitude = evaluator.compute_magnitude(&zero_vector);
    assert!((magnitude - 0.0).abs() < f64::EPSILON,
        "all-zero drift vector must produce exactly zero magnitude, got {}", magnitude);
}

// ---------------------------------------------------------------------------
// D4: Weight influence
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § D4
/// Spec: invariants.md § D4 — default kernel=2.0, gpu=2.0
/// If this test didn't exist: kernel/GPU drift could be under-weighted,
/// leading to dangerously long commit windows for critical changes.
#[test]
fn d4_kernel_and_gpu_weighted_double() {
    let evaluator = stub_drift_evaluator();

    // Drift only in kernel dimension
    let kernel_only = test_drift_vector([0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let kernel_mag = evaluator.compute_magnitude(&kernel_only);

    // Drift only in files dimension (weight=1.0)
    let files_only = test_drift_vector([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let files_mag = evaluator.compute_magnitude(&files_only);

    assert!((kernel_mag / files_mag - 2.0).abs() < 0.01,
        "kernel drift (weight=2.0) should produce 2x magnitude of files (weight=1.0), \
         got ratio {}", kernel_mag / files_mag);

    // Drift only in GPU dimension
    let gpu_only = test_drift_vector([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    let gpu_mag = evaluator.compute_magnitude(&gpu_only);

    assert!((gpu_mag / files_mag - 2.0).abs() < 0.01,
        "GPU drift (weight=2.0) should produce 2x magnitude of files (weight=1.0), \
         got ratio {}", gpu_mag / files_mag);
}

/// Contract: enforcement-map.md § D4
/// Spec: invariants.md § D4 — zero weight ignores dimension
/// If this test didn't exist: zero-weighted dimensions could still contribute
/// to magnitude via rounding or implementation error.
#[test]
fn d4_zero_weight_ignores_dimension() {
    let evaluator = stub_drift_evaluator_with_weights(
        DriftWeights { mounts: 0.0, files: 1.0, network: 1.0, services: 1.0, kernel: 1.0, packages: 1.0, gpu: 1.0 }
    );

    // Drift only in the zero-weighted mounts dimension
    let mounts_only = test_drift_vector([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let magnitude = evaluator.compute_magnitude(&mounts_only);

    assert!((magnitude - 0.0).abs() < f64::EPSILON,
        "dimension with weight=0.0 must be completely ignored, got magnitude {}", magnitude);
}

// ---------------------------------------------------------------------------
// D5: Observe-only mode
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § D5
/// Spec: invariants.md § D5 — observe-only: drift logged, no commit window
/// If this test didn't exist: observe-only mode could silently enforce policy
/// or trigger rollbacks during the bootstrap observation period.
#[test]
fn d5_observe_only_logs_without_enforcement() {
    let manager = stub_commit_window_manager();
    manager.set_enforcement_mode("observe");

    let drift = test_drift_event("files", 0.5);
    let result = manager.on_drift(drift);
    assert!(result.is_ok());

    // No commit window opened
    assert!(manager.active_window().is_none(),
        "observe-only mode must NOT open a commit window");

    // But drift was logged
    assert!(!manager.drift_log().is_empty(),
        "observe-only mode must still log the drift event");
}
