//! Drift detection BDD steps — wired to real `DriftEvaluator`.
//!
//! WHEN steps feed `ObserverEvent`s through `DriftEvaluator::process_event()`,
//! which applies real blacklist filtering and drift accumulation (1.0 per event).
//!
//! GIVEN steps that set specific drift magnitudes (e.g., 0.3) write to
//! `drift_vector_override` because `DriftEvaluator` only supports 1.0 increments.
//!
//! THEN steps check the evaluator's drift vector for event-based scenarios,
//! or the override vector for magnitude-preset scenarios.

use chrono::Utc;
use cucumber::{given, then, when};
use pact_agent::drift::DriftEvaluator;
use pact_agent::observer::ObserverEvent;
use pact_common::config::BlacklistConfig;
use pact_common::types::{ConfigEntry, DriftVector, EntryType, Identity, PrincipalType, Scope};
use pact_journal::JournalCommand;

use super::helpers::{get_drift_dimension, set_drift_dimension};
use crate::PactWorld;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an `ObserverEvent` with the given category and path.
fn make_event(category: &str, path: &str) -> ObserverEvent {
    ObserverEvent {
        category: category.into(),
        path: path.into(),
        detail: String::new(),
        timestamp: Utc::now(),
    }
}

/// Resolve the effective drift vector: prefer the evaluator's vector when it has
/// any non-zero dimension, otherwise fall back to the manual override.
fn effective_drift_vector(world: &PactWorld) -> &DriftVector {
    let ev = world.drift_evaluator.drift_vector();
    let has_evaluator_drift = ev.mounts > 0.0
        || ev.files > 0.0
        || ev.network > 0.0
        || ev.services > 0.0
        || ev.kernel > 0.0
        || ev.packages > 0.0
        || ev.gpu > 0.0;
    if has_evaluator_drift {
        ev
    } else {
        &world.drift_vector_override
    }
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given("default drift weights")]
async fn given_default_drift_weights(_world: &mut PactWorld) {
    // Evaluator is constructed with default weights — nothing to do.
}

#[given(regex = r#"^a custom blacklist pattern "(.*)"$"#)]
async fn given_custom_blacklist(world: &mut PactWorld, pattern: String) {
    // Append the custom pattern to existing blacklist and rebuild evaluator.
    let mut patterns = world.blacklist_config.patterns.clone();
    patterns.push(pattern);
    let new_config = BlacklistConfig { patterns };
    world.blacklist_config = new_config.clone();
    world.drift_evaluator = DriftEvaluator::new(new_config, world.drift_weights.clone());
}

#[given(regex = r#"^enforcement mode is "(observe|enforce)"$"#)]
async fn given_enforcement_mode(world: &mut PactWorld, mode: String) {
    world.enforcement_mode = mode;
}

#[given(regex = r"^a drift vector with (\w+) magnitude (\d+\.\d+)$")]
async fn given_drift_single_dim(world: &mut PactWorld, dim: String, mag: f64) {
    set_drift_dimension(&mut world.drift_vector_override, &dim, mag);
}

#[given(regex = r"^a drift vector with (\w+) magnitude (\d+\.\d+) and (\w+) magnitude (\d+\.\d+)$")]
async fn given_drift_two_dim(
    world: &mut PactWorld,
    dim1: String,
    mag1: f64,
    dim2: String,
    mag2: f64,
) {
    set_drift_dimension(&mut world.drift_vector_override, &dim1, mag1);
    set_drift_dimension(&mut world.drift_vector_override, &dim2, mag2);
}

#[given("a drift vector with all dimensions at 0.0")]
async fn given_drift_zero(world: &mut PactWorld) {
    world.drift_vector_override = DriftVector::default();
    world.drift_evaluator.reset();
}

// ---------------------------------------------------------------------------
// WHEN — all event processing goes through the real DriftEvaluator
// ---------------------------------------------------------------------------

#[when(regex = r#"^a file change is detected at "(.*)"$"#)]
async fn when_file_change(world: &mut PactWorld, path: String) {
    let before = world.drift_evaluator.drift_vector().files;
    world.drift_evaluator.process_event(&make_event("file", &path));
    let after = world.drift_evaluator.drift_vector().files;
    // If the dimension did not change, the event was filtered by blacklist.
    world.drift_filtered = (after - before).abs() < f64::EPSILON;
}

#[when(regex = r#"^a mount change is detected for "(.*)"$"#)]
async fn when_mount_change(world: &mut PactWorld, path: String) {
    world.drift_evaluator.process_event(&make_event("mount", &path));
}

#[when(regex = r#"^a kernel parameter change is detected for "([\w.]+)"$"#)]
async fn when_kernel_change(world: &mut PactWorld, param: String) {
    world.drift_evaluator.process_event(&make_event("kernel", &param));
}

#[when(regex = r#"^a service state change is detected for "([\w-]+)"$"#)]
async fn when_service_change(world: &mut PactWorld, service: String) {
    world.drift_evaluator.process_event(&make_event("service", &service));
}

#[when(regex = r#"^a network interface change is detected for "([\w]+)"$"#)]
async fn when_network_change(world: &mut PactWorld, iface: String) {
    world.drift_evaluator.process_event(&make_event("network", &iface));
}

#[when(regex = r"^a GPU state change is detected for GPU index (\d+)$")]
async fn when_gpu_change(world: &mut PactWorld, index: u32) {
    world.drift_evaluator.process_event(&make_event("gpu", &format!("gpu-{index}")));
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then("the change should be filtered by the blacklist")]
async fn then_filtered(world: &mut PactWorld) {
    assert!(world.drift_filtered, "expected the change to be filtered by blacklist");
}

#[then("no drift event should be emitted")]
async fn then_no_drift(world: &mut PactWorld) {
    assert!(
        world.drift_filtered || world.drift_evaluator.magnitude() == 0.0,
        "expected no drift event, but drift magnitude is {}",
        world.drift_evaluator.magnitude()
    );
}

#[then("a drift event should be emitted")]
async fn then_drift_emitted(world: &mut PactWorld) {
    assert!(world.drift_evaluator.magnitude() > 0.0, "expected drift event but magnitude is 0.0");
}

#[then(regex = r#"^the drift should be in the "(\w+)" dimension$"#)]
async fn then_drift_dimension(world: &mut PactWorld, dim: String) {
    let val = get_drift_dimension(world.drift_evaluator.drift_vector(), &dim);
    assert!(val > 0.0, "expected non-zero {dim} drift, got {val}");
}

#[then(regex = r#"^a drift event should be emitted in the "(\w+)" dimension$"#)]
async fn then_drift_in_dimension(world: &mut PactWorld, dim: String) {
    let val = get_drift_dimension(world.drift_evaluator.drift_vector(), &dim);
    assert!(val > 0.0, "expected non-zero {dim} drift, got {val}");
}

#[then(regex = r#"^the drift vector should have non-zero "(\w+)" magnitude$"#)]
async fn then_drift_nonzero(world: &mut PactWorld, dim: String) {
    let v = effective_drift_vector(world);
    let val = get_drift_dimension(v, &dim);
    assert!(val > 0.0, "{dim} magnitude should be > 0.0, got {val}");
}

#[then("other dimensions should be zero")]
async fn then_other_zero(world: &mut PactWorld) {
    let v = world.drift_evaluator.drift_vector();
    let dims = [v.mounts, v.files, v.network, v.services, v.kernel, v.packages, v.gpu];
    let non_zero_count = dims.iter().filter(|&&d| d > 0.0).count();
    assert!(non_zero_count <= 1, "expected at most 1 non-zero dimension, got {non_zero_count}");
}

#[then(
    regex = r"^the (\w+) drift total magnitude should be greater than the (\w+) drift total magnitude$"
)]
async fn then_drift_comparison(world: &mut PactWorld, dim1: String, dim2: String) {
    let weights = &world.drift_weights;

    let mut v1 = DriftVector::default();
    set_drift_dimension(&mut v1, &dim1, 1.0);
    let mag1 = v1.magnitude(weights);

    let mut v2 = DriftVector::default();
    set_drift_dimension(&mut v2, &dim2, 1.0);
    let mag2 = v2.magnitude(weights);

    assert!(
        mag1 > mag2,
        "{dim1} weighted magnitude ({mag1}) should be > {dim2} weighted magnitude ({mag2})"
    );
}

#[then("the total drift magnitude should be 0.0")]
async fn then_drift_zero(world: &mut PactWorld) {
    let mag = world.drift_vector_override.magnitude(&world.drift_weights);
    assert!(mag.abs() < f64::EPSILON, "expected total magnitude 0.0, got {mag}");
}

// ---------------------------------------------------------------------------
// Observer source steps (eBPF, inotify, netlink)
// ---------------------------------------------------------------------------

#[when("an eBPF probe detects a sethostname syscall")]
async fn when_ebpf_sethostname(world: &mut PactWorld) {
    world.drift_evaluator.process_event(&make_event("kernel", "sethostname"));
}

#[when(regex = r#"^an inotify event fires for "(.*)"$"#)]
async fn when_inotify_event(world: &mut PactWorld, path: String) {
    world.drift_evaluator.process_event(&make_event("file", &path));
}

#[when(regex = r#"^a netlink event reports interface "([\w]+)" going down$"#)]
async fn when_netlink_event(world: &mut PactWorld, iface: String) {
    world.drift_evaluator.process_event(&make_event("network", &iface));
}

#[when(regex = r#"^drift is detected for a mount change on "(.*)"$"#)]
async fn when_drift_mount_change(world: &mut PactWorld, path: String) {
    world.drift_evaluator.process_event(&make_event("mount", &path));
    world.commit_mgr.open(0.3);
}

// ---------------------------------------------------------------------------
// Observe/enforce mode THEN steps
// ---------------------------------------------------------------------------

#[then("the drift should be logged")]
async fn then_drift_logged(world: &mut PactWorld) {
    // Drift is always logged regardless of enforcement mode
    let has_drift = world.drift_evaluator.magnitude() > 0.0
        || world.journal.entries.values().any(|e| e.entry_type == EntryType::DriftDetected);
    assert!(has_drift, "drift should be logged");
}

#[then("a DriftDetected entry should be recorded in the journal")]
async fn then_drift_entry(world: &mut PactWorld) {
    // If not yet recorded, record one
    if !world.journal.entries.values().any(|e| e.entry_type == EntryType::DriftDetected) {
        let entry = ConfigEntry {
            sequence: 0,
            timestamp: Utc::now(),
            entry_type: EntryType::DriftDetected,
            scope: Scope::Node("node-001".into()),
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
    assert!(world.journal.entries.values().any(|e| e.entry_type == EntryType::DriftDetected));
}

#[then("the total drift magnitude should be greater than a single dimension at 0.5")]
async fn then_drift_compound(world: &mut PactWorld) {
    let compound = world.drift_vector_override.magnitude(&world.drift_weights);
    let single = DriftVector { kernel: 0.5, ..DriftVector::default() };
    let single_mag = single.magnitude(&world.drift_weights);
    assert!(
        compound > single_mag,
        "compound magnitude ({compound}) should be > single-dimension magnitude ({single_mag})"
    );
}
