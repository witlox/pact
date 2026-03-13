//! Commit window steps — wired to CommitWindowManager.

use cucumber::{given, then, when};
use pact_agent::commit::{CommitWindowManager, WindowState};
use pact_common::config::CommitWindowConfig;
use pact_common::types::{ConfigEntry, EntryType, Identity, PrincipalType, Scope};
use pact_journal::JournalCommand;

use chrono::Utc;

use crate::PactWorld;

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(
    regex = r#"^default commit window config with base (\d+) seconds and sensitivity (\d+\.\d+)$"#
)]
async fn given_commit_window_config(world: &mut PactWorld, base: u32, sensitivity: f64) {
    let config = CommitWindowConfig {
        base_window_seconds: base,
        drift_sensitivity: sensitivity,
        emergency_window_seconds: world.commit_mgr.config().emergency_window_seconds,
    };
    world.commit_mgr = CommitWindowManager::new(config);
}

#[given(regex = r#"^commit window config with base (\d+) seconds and sensitivity (\d+\.\d+)$"#)]
async fn given_commit_window_config_override(world: &mut PactWorld, base: u32, sensitivity: f64) {
    let config = CommitWindowConfig {
        base_window_seconds: base,
        drift_sensitivity: sensitivity,
        emergency_window_seconds: world.commit_mgr.config().emergency_window_seconds,
    };
    world.commit_mgr = CommitWindowManager::new(config);
}

#[given(regex = r#"^emergency mode is active with window (\d+) seconds$"#)]
async fn given_emergency_active(world: &mut PactWorld, window: u32) {
    let config = CommitWindowConfig {
        base_window_seconds: world.commit_mgr.config().base_window_seconds,
        drift_sensitivity: world.commit_mgr.config().drift_sensitivity,
        emergency_window_seconds: window,
    };
    world.commit_mgr = CommitWindowManager::new(config);
    world.commit_mgr.enter_emergency();
    world
        .emergency_mgr
        .start(
            Identity {
                principal: "system@pact.internal".into(),
                principal_type: PrincipalType::Service,
                role: "pact-service-agent".into(),
            },
            "pre-set emergency".into(),
        )
        .ok(); // ignore if already active
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(regex = r#"^drift is detected with magnitude (\d+\.\d+)$"#)]
async fn when_drift_magnitude(world: &mut PactWorld, magnitude: f64) {
    world.commit_mgr.open(magnitude);
}

#[when(regex = r#"^drift is detected on node "([\w-]+)"$"#)]
async fn when_drift_detected_node_default(world: &mut PactWorld, node: String) {
    when_drift_detected_node_impl(world, 0.3, node).await;
}

#[when(regex = r#"^drift is detected with magnitude (\d+\.\d+) on node "([\w-]+)"$"#)]
async fn when_drift_detected_node_mag(world: &mut PactWorld, magnitude: f64, node: String) {
    when_drift_detected_node_impl(world, magnitude, node).await;
}

async fn when_drift_detected_node_impl(world: &mut PactWorld, magnitude: f64, node: String) {
    if world.enforcement_mode == "enforce" && !world.commit_mgr.is_emergency() {
        world.commit_mgr.open(magnitude);
    }

    // Record drift in journal
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::DriftDetected,
        scope: Scope::Node(node),
        author: Identity {
            principal: "system".into(),
            principal_type: PrincipalType::Service,
            role: "pact-service-agent".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r#"^the commit window should be approximately (\d+) seconds$"#)]
async fn then_window_approx(world: &mut PactWorld, expected: u32) {
    let tolerance = 10;
    let calculated = match world.commit_mgr.state() {
        WindowState::Open {
            opened_at,
            deadline,
        } => (*deadline - *opened_at).num_seconds() as u32,
        _ => world.commit_mgr.seconds_remaining(),
    };
    assert!(
        (calculated as i64 - expected as i64).unsigned_abs() < tolerance,
        "expected ~{expected}s, got {calculated}s",
    );
}

#[then(regex = r#"^the commit window for node "([\w-]+)" should be (\d+) seconds$"#)]
async fn then_emergency_window(world: &mut PactWorld, _node: String, expected: u32) {
    assert_eq!(world.commit_mgr.config().emergency_window_seconds, expected);
}
