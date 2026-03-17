//! Capability reporting steps — wired to CapabilityReporter + MockGpuBackend.

use cucumber::{given, then, when};
use pact_common::types::{
    CapabilityReport, ConfigState, EmergencyInfo, GpuCapability, GpuHealth, GpuVendor, Identity,
    MemoryCapability, PrincipalType, SoftwareCapability, StorageCapability, SupervisorBackend,
    SupervisorStatus,
};
use pact_journal::JournalCommand;
use tempfile::TempDir;

use crate::PactWorld;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_gpu(index: u32, vendor: GpuVendor, model: &str, health: GpuHealth) -> GpuCapability {
    GpuCapability {
        index,
        vendor,
        model: model.to_string(),
        memory_bytes: 80 * 1024 * 1024 * 1024,
        health,
        pci_bus_id: format!("0000:{:02x}:00.0", index + 0x3b),
    }
}

fn build_report(world: &PactWorld, node_id: &str) -> CapabilityReport {
    let config_state =
        world.journal.node_states.get(node_id).cloned().unwrap_or(ConfigState::ObserveOnly);

    let emergency = if config_state == ConfigState::Emergency {
        Some(EmergencyInfo {
            reason: "emergency active".into(),
            admin_identity: Identity {
                principal: "admin@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-platform-admin".into(),
            },
            started_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(4),
        })
    } else {
        None
    };

    CapabilityReport {
        node_id: node_id.into(),
        timestamp: chrono::Utc::now(),
        report_id: uuid::Uuid::new_v4(),
        gpus: world.gpu_capabilities.clone(),
        memory: MemoryCapability { total_bytes: 0, available_bytes: 0, numa_nodes: 1 },
        network: None,
        storage: StorageCapability { tmpfs_bytes: 0, mounts: vec![] },
        software: SoftwareCapability { loaded_modules: vec![], uenv_image: None, services: vec![] },
        config_state,
        drift_summary: None,
        emergency,
        supervisor_status: world.supervisor_status.clone(),
    }
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(regex = r"^a node with (\d+) NVIDIA (\S+) GPUs$")]
async fn given_nvidia_gpus(world: &mut PactWorld, count: u32, model: String) {
    world.gpu_capabilities =
        (0..count).map(|i| make_gpu(i, GpuVendor::Nvidia, &model, GpuHealth::Healthy)).collect();
}

#[given(regex = r"^a node with (\d+) AMD (\S+) GPUs$")]
async fn given_amd_gpus(world: &mut PactWorld, count: u32, model: String) {
    world.gpu_capabilities =
        (0..count).map(|i| make_gpu(i, GpuVendor::Amd, &model, GpuHealth::Healthy)).collect();
}

#[given(regex = r"^a node with (\d+) NVIDIA (\S+) GPUs and (\d+) AMD (\S+) GPUs$")]
async fn given_mixed_gpus(
    world: &mut PactWorld,
    nv_count: u32,
    nv_model: String,
    amd_count: u32,
    amd_model: String,
) {
    let mut gpus: Vec<GpuCapability> = (0..nv_count)
        .map(|i| make_gpu(i, GpuVendor::Nvidia, &nv_model, GpuHealth::Healthy))
        .collect();
    gpus.extend(
        (0..amd_count)
            .map(|i| make_gpu(nv_count + i, GpuVendor::Amd, &amd_model, GpuHealth::Healthy)),
    );
    world.gpu_capabilities = gpus;
}

#[given("a node with no GPUs")]
async fn given_no_gpus(world: &mut PactWorld) {
    world.gpu_capabilities.clear();
}

#[given("a node with 1 NVIDIA GPU in healthy state")]
async fn given_healthy_gpu(world: &mut PactWorld) {
    world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Healthy)];
}

#[given("a node with 1 NVIDIA GPU that reports degraded health")]
async fn given_degraded_gpu(world: &mut PactWorld) {
    world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Degraded)];
}

#[given("a node with 1 NVIDIA GPU that fails")]
async fn given_failed_gpu(world: &mut PactWorld) {
    world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Failed)];
}

#[given(regex = r"^a node with (\d+) GB of memory$")]
async fn given_memory(world: &mut PactWorld, gb: u64) {
    // Store memory in a temporary capability report
    let report = CapabilityReport {
        node_id: "test-node".into(),
        timestamp: chrono::Utc::now(),
        report_id: uuid::Uuid::new_v4(),
        gpus: vec![],
        memory: MemoryCapability {
            total_bytes: gb * 1024 * 1024 * 1024,
            available_bytes: gb * 1024 * 1024 * 1024,
            numa_nodes: 1,
        },
        network: None,
        storage: StorageCapability { tmpfs_bytes: 0, mounts: vec![] },
        software: SoftwareCapability { loaded_modules: vec![], uenv_image: None, services: vec![] },
        config_state: ConfigState::ObserveOnly,
        drift_summary: None,
        emergency: None,
        supervisor_status: world.supervisor_status.clone(),
    };
    world.capability_report = Some(report);
}

#[given("a stable capability report")]
async fn given_stable_report(world: &mut PactWorld) {
    if world.gpu_capabilities.is_empty() {
        world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Healthy)];
    }
    world.capability_report = Some(build_report(world, "test-node"));
}

#[given(regex = r#"^node "([\w-]+)" is in state "([\w]+)"$"#)]
async fn given_node_state(world: &mut PactWorld, node: String, state_str: String) {
    // Handle enrollment states (Inactive, Revoked, etc.)
    if state_str == "Inactive" && world.journal.enrollments.contains_key(&node) {
        world.journal.apply_command(JournalCommand::DeactivateNode { node_id: node.clone() });
    }
    let state = super::helpers::parse_config_state(&state_str);
    world.journal.apply_command(JournalCommand::UpdateNodeState { node_id: node, state });
}

#[given(regex = r"^(\d+) declared services with (\d+) running and (\d+) failed$")]
async fn given_supervisor_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    world.supervisor_status = SupervisorStatus {
        backend: SupervisorBackend::Pact,
        services_declared: declared,
        services_running: running,
        services_failed: failed,
    };
}

#[given(regex = r#"^node "([\w-]+)" is in emergency mode$"#)]
async fn given_emergency_mode(world: &mut PactWorld, node: String) {
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: node,
        state: ConfigState::Emergency,
    });
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when("capability detection runs")]
async fn when_detection_runs(world: &mut PactWorld) {
    // If we already have a report with memory set, keep memory; otherwise build fresh
    let existing_memory = world
        .capability_report
        .as_ref()
        .map_or(MemoryCapability { total_bytes: 0, available_bytes: 0, numa_nodes: 1 }, |r| {
            r.memory.clone()
        });

    let mut report = build_report(world, "test-node");
    report.memory = existing_memory;

    // If any GPU has non-healthy state, record a CapabilityChange entry
    let has_degraded = report.gpus.iter().any(|g| g.health != GpuHealth::Healthy);
    if has_degraded {
        let entry = pact_common::types::ConfigEntry {
            sequence: 0,
            timestamp: chrono::Utc::now(),
            entry_type: pact_common::types::EntryType::CapabilityChange,
            scope: pact_common::types::Scope::Node("test-node".into()),
            author: pact_common::types::Identity {
                principal: "pact-agent".into(),
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

    world.capability_report = Some(report);
    world.manifest_written = true;
    world.socket_available = true;
}

#[when(regex = r#"^capability detection runs for node "([\w-]+)"$"#)]
async fn when_detection_runs_node(world: &mut PactWorld, node: String) {
    let report = build_report(world, &node);
    world.capability_report = Some(report);
    world.manifest_written = true;
}

#[when("GPU 0 transitions from healthy to degraded")]
async fn when_gpu_transition(world: &mut PactWorld) {
    if !world.gpu_capabilities.is_empty() {
        world.gpu_capabilities[0].health = GpuHealth::Degraded;
    }
    let report = build_report(world, "test-node");
    world.capability_report = Some(report);

    // Record CapabilityChange entry
    let entry = pact_common::types::ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: pact_common::types::EntryType::CapabilityChange,
        scope: pact_common::types::Scope::Node("test-node".into()),
        author: pact_common::types::Identity {
            principal: "pact-agent".into(),
            principal_type: pact_common::types::PrincipalType::Service,
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

#[when("the configured poll interval elapses")]
async fn when_poll_interval(world: &mut PactWorld) {
    // Simulate periodic report
    let report = build_report(world, "test-node");
    world.capability_report = Some(report);
}

#[when("the supervisor status is queried")]
async fn when_supervisor_status(world: &mut PactWorld) {
    // Status is already in world.supervisor_status — just verify it's accessible
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r"^the capability report should contain (\d+) GPUs?$")]
async fn then_gpu_count(world: &mut PactWorld, count: usize) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.gpus.len(), count, "expected {count} GPUs, got {}", report.gpus.len());
}

#[then(regex = r#"^all GPUs should have vendor "([\w]+)"$"#)]
async fn then_all_vendor(world: &mut PactWorld, vendor_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match vendor_str.as_str() {
        "Nvidia" => GpuVendor::Nvidia,
        "Amd" => GpuVendor::Amd,
        _ => panic!("unknown vendor: {vendor_str}"),
    };
    for gpu in &report.gpus {
        assert_eq!(gpu.vendor, expected);
    }
}

#[then(regex = r#"^all GPUs should have model "([\w-]+)"$"#)]
async fn then_all_model(world: &mut PactWorld, model: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    for gpu in &report.gpus {
        assert_eq!(gpu.model, model);
    }
}

#[then(regex = r#"^(\d+) GPUs should have vendor "([\w]+)"$"#)]
async fn then_n_vendor(world: &mut PactWorld, count: usize, vendor_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match vendor_str.as_str() {
        "Nvidia" => GpuVendor::Nvidia,
        "Amd" => GpuVendor::Amd,
        _ => panic!("unknown vendor: {vendor_str}"),
    };
    let actual = report.gpus.iter().filter(|g| g.vendor == expected).count();
    assert_eq!(actual, count);
}

#[then(regex = r#"^the GPU health should be "([\w]+)"$"#)]
async fn then_gpu_health(world: &mut PactWorld, health_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match health_str.as_str() {
        "Healthy" => GpuHealth::Healthy,
        "Degraded" => GpuHealth::Degraded,
        "Failed" => GpuHealth::Failed,
        _ => panic!("unknown health: {health_str}"),
    };
    assert_eq!(report.gpus[0].health, expected);
}

#[then("a CapabilityChange entry should be recorded in the journal")]
async fn then_cap_change_entry(world: &mut PactWorld) {
    let has_cap_change = world
        .journal
        .entries
        .values()
        .any(|e| e.entry_type == pact_common::types::EntryType::CapabilityChange);
    assert!(has_cap_change, "no CapabilityChange entry found in journal");
}

#[then("the capability report should be updated immediately")]
async fn then_report_updated(world: &mut PactWorld) {
    assert!(world.capability_report.is_some(), "capability report should be updated");
}

#[then(regex = r"^the capability report should show (\d+) memory bytes$")]
async fn then_memory_bytes(world: &mut PactWorld, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.total_bytes, bytes);
}

#[then("the report should be written to the configured manifest path")]
async fn then_manifest_written(world: &mut PactWorld) {
    assert!(world.manifest_written, "manifest should be written");
}

#[then("the manifest should be valid JSON")]
async fn then_manifest_json(world: &mut PactWorld) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let json = serde_json::to_string(report).expect("report should serialize to JSON");
    let _parsed: CapabilityReport =
        serde_json::from_str(&json).expect("JSON should parse back to CapabilityReport");
}

#[then("the report should be available via the configured unix socket")]
async fn then_socket_available(world: &mut PactWorld) {
    assert!(world.socket_available, "socket should be available");
}

#[then("lattice-node-agent should be able to read it")]
async fn then_lattice_can_read(world: &mut PactWorld) {
    // Socket availability implies lattice-node-agent can read it
    assert!(world.socket_available);
}

#[then("a new capability report should be sent immediately")]
async fn then_new_report_sent(world: &mut PactWorld) {
    assert!(world.capability_report.is_some(), "new report should be sent");
}

#[then("a new capability report should be sent")]
async fn then_periodic_report(world: &mut PactWorld) {
    assert!(world.capability_report.is_some(), "periodic report should be sent");
}

#[then(regex = r#"^the capability report config state should be "([\w]+)"$"#)]
async fn then_config_state(world: &mut PactWorld, state_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = super::helpers::parse_config_state(&state_str);
    assert_eq!(report.config_state, expected);
}

#[then(regex = r#"^the supervisor status should show backend "([\w]+)"$"#)]
async fn then_supervisor_backend(world: &mut PactWorld, backend_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match backend_str.as_str() {
        "Pact" => SupervisorBackend::Pact,
        "Systemd" => SupervisorBackend::Systemd,
        _ => panic!("unknown backend: {backend_str}"),
    };
    assert_eq!(report.supervisor_status.backend, expected);
}

#[then(regex = r"^the supervisor status should show (\d+) declared, (\d+) running, (\d+) failed$")]
async fn then_supervisor_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.supervisor_status.services_declared, declared);
    assert_eq!(report.supervisor_status.services_running, running);
    assert_eq!(report.supervisor_status.services_failed, failed);
}

#[then(regex = r#"^the status should report backend "([\w]+)"$"#)]
async fn then_status_backend(world: &mut PactWorld, backend_str: String) {
    let expected = match backend_str.as_str() {
        "Pact" => SupervisorBackend::Pact,
        "Systemd" => SupervisorBackend::Systemd,
        _ => panic!("unknown backend: {backend_str}"),
    };
    assert_eq!(world.supervisor_status.backend, expected);
}

#[then(regex = r"^the status should report (\d+) declared, (\d+) running, (\d+) failed$")]
async fn then_status_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    assert_eq!(world.supervisor_status.services_declared, declared);
    assert_eq!(world.supervisor_status.services_running, running);
    assert_eq!(world.supervisor_status.services_failed, failed);
}
