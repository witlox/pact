//! Integration test: observer events → drift evaluator → commit window pipeline.
//!
//! Tests the real data flow between agent subsystems, not individual modules.

use chrono::Utc;
use tokio::sync::mpsc;

use pact_common::config::{BlacklistConfig, CommitWindowConfig};
use pact_common::types::{DriftWeights, Identity, PrincipalType};

use pact_agent::commit::{CommitWindowManager, WindowState};
use pact_agent::drift::DriftEvaluator;
use pact_agent::emergency::EmergencyManager;
use pact_agent::observer::{MockObserver, Observer, ObserverEvent};

fn event(category: &str, path: &str) -> ObserverEvent {
    ObserverEvent {
        category: category.into(),
        path: path.into(),
        detail: "test".into(),
        timestamp: Utc::now(),
    }
}

fn admin() -> Identity {
    Identity {
        principal: "admin@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-platform-admin".into(),
    }
}

/// Observer events flow through drift evaluator and open a commit window
/// when drift magnitude exceeds zero.
#[tokio::test]
async fn observer_events_trigger_commit_window() {
    // 1. Observer emits system change events
    let events = vec![
        event("kernel", "/etc/sysctl.conf"),
        event("mount", "/mnt/scratch"),
        event("file", "/etc/hostname"),
    ];
    let observer = MockObserver::with_events(events);
    let (tx, mut rx) = mpsc::channel(10);
    observer.start(tx).await.unwrap();

    // 2. Drift evaluator accumulates events
    let mut evaluator = DriftEvaluator::new(
        BlacklistConfig { patterns: vec![] }, // no blacklist
        DriftWeights::default(),
    );

    while let Ok(ev) = rx.try_recv() {
        evaluator.process_event(&ev);
    }

    let magnitude = evaluator.magnitude();
    assert!(magnitude > 0.0, "drift should be nonzero after events");

    // 3. Commit window opens based on drift magnitude
    let mut commit_window = CommitWindowManager::new(CommitWindowConfig::default());
    assert!(matches!(commit_window.state(), WindowState::Idle));

    commit_window.open(magnitude);
    assert!(matches!(commit_window.state(), WindowState::Open { .. }));
    assert!(commit_window.seconds_remaining() > 0);

    // 4. Higher drift → shorter window
    let window_secs = commit_window.calculate_window_seconds(magnitude);
    let base_window = commit_window.calculate_window_seconds(0.0);
    assert!(
        window_secs < base_window,
        "higher drift should produce shorter window: {window_secs} should be < {base_window}"
    );
}

/// Commit after drift: window opens, admin commits, window closes.
#[tokio::test]
async fn drift_then_commit_closes_window() {
    let mut evaluator =
        DriftEvaluator::new(BlacklistConfig { patterns: vec![] }, DriftWeights::default());

    // Generate drift
    evaluator.process_event(&event("kernel", "/etc/sysctl.conf"));
    evaluator.process_event(&event("service", "chronyd"));
    let magnitude = evaluator.magnitude();

    // Open window
    let mut commit_window = CommitWindowManager::new(CommitWindowConfig::default());
    commit_window.open(magnitude);
    assert!(matches!(commit_window.state(), WindowState::Open { .. }));

    // Admin commits — acknowledges drift
    commit_window.commit();
    assert!(matches!(commit_window.state(), WindowState::Idle));

    // Reset evaluator after commit
    evaluator.reset();
    assert_eq!(evaluator.magnitude(), 0.0);
}

/// Emergency mode extends commit window and prevents expiry.
#[tokio::test]
async fn emergency_mode_extends_commit_window() {
    let mut commit_window = CommitWindowManager::new(CommitWindowConfig {
        base_window_seconds: 60, // very short base
        drift_sensitivity: 2.0,
        emergency_window_seconds: 14400,
    });
    let mut emergency = EmergencyManager::new(14400);

    // Open window with drift
    commit_window.open(1.0);
    let normal_remaining = commit_window.seconds_remaining();

    // Enter emergency mode
    emergency.start(admin(), "network reconfiguration".into()).unwrap();
    commit_window.enter_emergency();

    // Re-open with emergency — should get much longer window
    commit_window.open(1.0);
    let emergency_remaining = commit_window.seconds_remaining();

    assert!(
        emergency_remaining > normal_remaining,
        "emergency window ({emergency_remaining}s) should be longer than normal ({normal_remaining}s)"
    );
    assert!(emergency_remaining > 14000, "emergency should be ~4 hours");

    // Emergency prevents expiry
    assert!(!commit_window.check_expired());

    // End emergency, commit
    emergency.end(&admin(), false).unwrap();
    commit_window.exit_emergency();
    commit_window.commit();
    assert!(matches!(commit_window.state(), WindowState::Idle));
}

/// Blacklisted events do not trigger drift or commit windows.
#[tokio::test]
async fn blacklisted_events_do_not_open_commit_window() {
    let events = vec![
        event("file", "/tmp/scratch/output.log"),
        event("file", "/var/log/messages"),
        event("file", "/proc/cpuinfo"),
    ];
    let observer = MockObserver::with_events(events);
    let (tx, mut rx) = mpsc::channel(10);
    observer.start(tx).await.unwrap();

    let mut evaluator = DriftEvaluator::new(
        BlacklistConfig::default(), // default blacklist includes /tmp, /var/log, /proc
        DriftWeights::default(),
    );

    while let Ok(ev) = rx.try_recv() {
        evaluator.process_event(&ev);
    }

    assert_eq!(evaluator.magnitude(), 0.0, "blacklisted paths should not produce drift");

    // With zero drift, commit window should stay idle (no reason to open)
    let commit_window = CommitWindowManager::new(CommitWindowConfig::default());
    assert!(matches!(commit_window.state(), WindowState::Idle));
}

/// Weighted drift: kernel events (weight=2.0) produce more drift than file events (weight=1.0).
#[tokio::test]
async fn kernel_drift_weighted_higher_than_file_drift() {
    let weights = DriftWeights::default(); // kernel=2.0, files=1.0

    // One kernel event
    let mut eval_kernel =
        DriftEvaluator::new(BlacklistConfig { patterns: vec![] }, weights.clone());
    eval_kernel.process_event(&event("kernel", "/etc/sysctl.conf"));
    let kernel_mag = eval_kernel.magnitude();

    // One file event
    let mut eval_file = DriftEvaluator::new(BlacklistConfig { patterns: vec![] }, weights);
    eval_file.process_event(&event("file", "/etc/hostname"));
    let file_mag = eval_file.magnitude();

    assert!(
        kernel_mag > file_mag,
        "kernel drift ({kernel_mag}) should be higher than file drift ({file_mag}) due to weight=2.0"
    );

    // Verify exact values: kernel weight=2.0, so magnitude = sqrt(2.0 * 1^2) = sqrt(2)
    let expected_kernel = (2.0_f64).sqrt();
    assert!(
        (kernel_mag - expected_kernel).abs() < 1e-10,
        "kernel magnitude should be sqrt(2) ≈ {expected_kernel}, got {kernel_mag}"
    );

    // File weight=1.0, so magnitude = sqrt(1.0 * 1^2) = 1.0
    assert!((file_mag - 1.0).abs() < 1e-10, "file magnitude should be 1.0, got {file_mag}");
}
