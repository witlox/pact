//! Contract tests for shared kernel data models (serialization + validation).
//!
//! These tests verify:
//! - Round-trip serialization of shared kernel types
//! - Validation constraints from the spec
//! - Enum variant completeness
//!
//! Source: specs/architecture/data-models/shared-kernel.md (excluding Node Enrollment)

// ---------------------------------------------------------------------------
// Helper stubs
// ---------------------------------------------------------------------------

fn test_identity(principal: &str, role: &str) -> Identity {
    Identity {
        principal: principal.into(),
        principal_type: PrincipalType::Human,
        role: role.into(),
    }
}

fn test_config_entry() -> ConfigEntry {
    ConfigEntry {
        sequence: 42,
        timestamp: Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::VCluster("ml-training".into()),
        author: test_identity("admin@example.com", "pact-platform-admin"),
        parent: Some(41),
        state_delta: Some(StateDelta {
            mounts: vec![],
            files: vec![DeltaItem {
                action: DeltaAction::Add,
                key: "/etc/pact.toml".into(),
                value: Some("contents".into()),
                previous: None,
            }],
            network: vec![],
            services: vec![],
            kernel: vec![],
            packages: vec![],
            gpu: vec![],
        }),
        policy_ref: Some("policy-001".into()),
        ttl_seconds: Some(900),
        emergency_reason: None,
    }
}

fn test_vcluster_policy() -> VClusterPolicy {
    VClusterPolicy {
        vcluster_id: "ml-training".into(),
        policy_id: "pol-001".into(),
        updated_at: Some(Utc::now()),
        drift_sensitivity: 2.0,
        base_commit_window_seconds: 900,
        emergency_window_seconds: 14400,
        auto_converge_categories: vec!["packages".into()],
        require_ack_categories: vec!["kernel".into(), "gpu".into()],
        enforcement_mode: "enforce".into(),
        role_bindings: vec![RoleBinding {
            role: "pact-ops-ml-training".into(),
            principals: vec!["admin@example.com".into()],
            allowed_actions: vec!["commit".into(), "exec".into()],
        }],
        regulated: true,
        two_person_approval: true,
        emergency_allowed: true,
        audit_retention_days: 2555,
        federation_template: Some("template-001".into()),
        supervisor_backend: "pact".into(),
        exec_whitelist: vec!["nvidia-smi".into()],
        shell_whitelist: vec!["bash".into()],
    }
}

fn test_capability_report() -> CapabilityReport {
    CapabilityReport {
        node_id: "compute-042".into(),
        timestamp: Utc::now(),
        report_id: Uuid::new_v4(),
        gpus: vec![GpuCapability {
            index: 0,
            vendor: GpuVendor::Nvidia,
            model: "A100".into(),
            memory_bytes: 80_000_000_000,
            health: GpuHealth::Healthy,
            pci_bus_id: "0000:3b:00.0".into(),
        }],
        memory: MemoryCapability {
            total_bytes: 512_000_000_000,
            available_bytes: 500_000_000_000,
            numa_nodes: 2,
        },
        network: Some(NetworkCapability {
            fabric_type: "InfiniBand".into(),
            bandwidth_bps: 200_000_000_000,
            latency_us: 1.5,
        }),
        storage: StorageCapability {
            tmpfs_bytes: 64_000_000_000,
            mounts: vec![MountPointInfo {
                path: "/scratch".into(),
                fs_type: "lustre".into(),
                source: "mds01:/scratch".into(),
                available: true,
            }],
        },
        software: SoftwareCapability {
            loaded_modules: vec!["nvidia".into(), "ib_core".into()],
            uenv_image: Some("ml-stack-v3".into()),
            services: vec![ServiceStatusInfo {
                name: "lattice-node-agent".into(),
                state: ServiceState::Running,
                pid: 1234,
                uptime_seconds: 3600,
                restart_count: 0,
            }],
        },
        config_state: ConfigState::Committed,
        drift_summary: None,
        emergency: None,
        supervisor_status: SupervisorStatus {
            backend: SupervisorBackend::Pact,
            services_declared: 4,
            services_running: 4,
            services_failed: 0,
        },
    }
}

// ===========================================================================
// Identity & Authorization
// ===========================================================================

/// Contract: shared-kernel.md § Identity
/// Spec: Identity with principal, principal_type, role must survive serde round-trip
/// If this test didn't exist: a field rename or serde attribute could silently drop data.
#[test]
fn identity_round_trip() {
    let identity = test_identity("admin@example.com", "pact-platform-admin");

    let json = serde_json::to_string(&identity).unwrap();
    let deserialized: Identity = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.principal, identity.principal);
    assert_eq!(deserialized.principal_type, identity.principal_type);
    assert_eq!(deserialized.role, identity.role);
}

/// Contract: shared-kernel.md § Identity
/// Spec: J3 — principal must be non-empty
/// If this test didn't exist: an empty principal could bypass authorization checks.
#[test]
fn identity_rejects_empty_principal() {
    let identity = Identity {
        principal: "".into(),
        principal_type: PrincipalType::Human,
        role: "pact-platform-admin".into(),
    };

    let result = validate_identity(&identity);
    assert!(result.is_err(), "empty principal should be rejected");
}

/// Contract: shared-kernel.md § Identity
/// Spec: J3 — role must be non-empty
/// If this test didn't exist: an empty role could bypass RBAC enforcement.
#[test]
fn identity_rejects_empty_role() {
    let identity = Identity {
        principal: "admin@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "".into(),
    };

    let result = validate_identity(&identity);
    assert!(result.is_err(), "empty role should be rejected");
}

/// Contract: shared-kernel.md § PrincipalType
/// Spec: all 3 variants must exist and round-trip
/// If this test didn't exist: a variant could be renamed or removed silently.
#[test]
fn principal_type_all_variants() {
    let variants = vec![
        PrincipalType::Human,
        PrincipalType::Agent,
        PrincipalType::Service,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: PrincipalType = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § RoleBinding
/// Spec: RoleBinding with role, principals, allowed_actions must survive serde
/// If this test didn't exist: policy enforcement could use stale or missing bindings.
#[test]
fn role_binding_round_trip() {
    let binding = RoleBinding {
        role: "pact-ops-ml-training".into(),
        principals: vec!["admin@example.com".into(), "ci-bot@service".into()],
        allowed_actions: vec!["commit".into(), "exec".into(), "status".into()],
    };

    let json = serde_json::to_string(&binding).unwrap();
    let deserialized: RoleBinding = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.role, binding.role);
    assert_eq!(deserialized.principals, binding.principals);
    assert_eq!(deserialized.allowed_actions, binding.allowed_actions);
}

// ===========================================================================
// Configuration State
// ===========================================================================

/// Contract: shared-kernel.md § ConfigState
/// Spec: all 5 variants must exist and round-trip
/// If this test didn't exist: a missing variant would break state machine transitions.
#[test]
fn config_state_all_variants() {
    let variants = vec![
        ConfigState::ObserveOnly,
        ConfigState::Committed,
        ConfigState::Drifted,
        ConfigState::Converging,
        ConfigState::Emergency,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: ConfigState = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § EntryType
/// Spec: all 20 EntryType variants must exist and round-trip
/// If this test didn't exist: a journal entry type could be missing, causing log gaps.
#[test]
fn entry_type_all_variants() {
    let variants = vec![
        EntryType::Commit,
        EntryType::Rollback,
        EntryType::AutoConverge,
        EntryType::DriftDetected,
        EntryType::CapabilityChange,
        EntryType::PolicyUpdate,
        EntryType::BootConfig,
        EntryType::EmergencyStart,
        EntryType::EmergencyEnd,
        EntryType::ExecLog,
        EntryType::ShellSession,
        EntryType::ServiceLifecycle,
        EntryType::PendingApproval,
        EntryType::NodeEnrolled,
        EntryType::NodeActivated,
        EntryType::NodeDeactivated,
        EntryType::NodeDecommissioned,
        EntryType::NodeAssigned,
        EntryType::NodeUnassigned,
        EntryType::CertSigned,
        EntryType::CertRevoked,
    ];

    assert_eq!(variants.len(), 21, "spec defines 21 EntryType variants");

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: EntryType = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § Scope
/// Spec: all 3 Scope variants must exist and round-trip (including inner data)
/// If this test didn't exist: scope-based filtering could silently misroute entries.
#[test]
fn scope_all_variants() {
    let variants = vec![
        Scope::Global,
        Scope::VCluster("ml-training".into()),
        Scope::Node("compute-042".into()),
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: Scope = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § ConfigEntry
/// Spec: full ConfigEntry with all fields populated must survive serde round-trip
/// If this test didn't exist: a field could be silently dropped from the immutable log.
#[test]
fn config_entry_round_trip() {
    let entry = test_config_entry();

    let json = serde_json::to_string(&entry).unwrap();
    let deserialized: ConfigEntry = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.sequence, entry.sequence);
    assert_eq!(deserialized.entry_type, entry.entry_type);
    assert_eq!(deserialized.scope, entry.scope);
    assert_eq!(deserialized.author.principal, entry.author.principal);
    assert_eq!(deserialized.parent, entry.parent);
    assert!(deserialized.state_delta.is_some());
    assert_eq!(deserialized.policy_ref, entry.policy_ref);
    assert_eq!(deserialized.ttl_seconds, entry.ttl_seconds);
    assert_eq!(deserialized.emergency_reason, entry.emergency_reason);
}

/// Contract: shared-kernel.md § ConfigEntry
/// Spec: optional fields (parent, state_delta, ttl) can be None
/// If this test didn't exist: None could serialize to null and fail to deserialize.
#[test]
fn config_entry_optional_fields_none() {
    let entry = ConfigEntry {
        sequence: 1,
        timestamp: Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::Global,
        author: test_identity("admin@example.com", "pact-platform-admin"),
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };

    let json = serde_json::to_string(&entry).unwrap();
    let deserialized: ConfigEntry = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.parent, None);
    assert!(deserialized.state_delta.is_none());
    assert_eq!(deserialized.ttl_seconds, None);
}

// ===========================================================================
// State Deltas & Drift
// ===========================================================================

/// Contract: shared-kernel.md § StateDelta
/// Spec: StateDelta with all DeltaActions (Add, Remove, Modify) must round-trip
/// If this test didn't exist: a delta action could serialize to wrong variant.
#[test]
fn state_delta_round_trip() {
    let delta = StateDelta {
        mounts: vec![DeltaItem {
            action: DeltaAction::Add,
            key: "/scratch".into(),
            value: Some("lustre".into()),
            previous: None,
        }],
        files: vec![DeltaItem {
            action: DeltaAction::Modify,
            key: "/etc/pact.toml".into(),
            value: Some("new-contents".into()),
            previous: Some("old-contents".into()),
        }],
        network: vec![],
        services: vec![DeltaItem {
            action: DeltaAction::Remove,
            key: "old-agent".into(),
            value: None,
            previous: Some("running".into()),
        }],
        kernel: vec![],
        packages: vec![],
        gpu: vec![],
    };

    let json = serde_json::to_string(&delta).unwrap();
    let deserialized: StateDelta = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.mounts.len(), 1);
    assert_eq!(deserialized.mounts[0].action, DeltaAction::Add);
    assert_eq!(deserialized.files.len(), 1);
    assert_eq!(deserialized.files[0].action, DeltaAction::Modify);
    assert_eq!(deserialized.files[0].previous, Some("old-contents".into()));
    assert_eq!(deserialized.services.len(), 1);
    assert_eq!(deserialized.services[0].action, DeltaAction::Remove);
}

/// Contract: shared-kernel.md § DriftVector
/// Spec: all 7 drift dimensions must survive serde round-trip
/// If this test didn't exist: a drift dimension could be dropped, skewing commit windows.
#[test]
fn drift_vector_round_trip() {
    let drift = DriftVector {
        mounts: 0.5,
        files: 1.2,
        network: 0.0,
        services: 0.8,
        kernel: 2.0,
        packages: 0.3,
        gpu: 1.5,
    };

    let json = serde_json::to_string(&drift).unwrap();
    let deserialized: DriftVector = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.mounts, drift.mounts);
    assert_eq!(deserialized.files, drift.files);
    assert_eq!(deserialized.network, drift.network);
    assert_eq!(deserialized.services, drift.services);
    assert_eq!(deserialized.kernel, drift.kernel);
    assert_eq!(deserialized.packages, drift.packages);
    assert_eq!(deserialized.gpu, drift.gpu);
}

/// Contract: shared-kernel.md § DriftVector
/// Spec: default DriftVector has all dimensions 0.0
/// If this test didn't exist: default could have nonzero values, triggering false drift.
#[test]
fn drift_vector_default_all_zero() {
    let drift = DriftVector::default();

    assert_eq!(drift.mounts, 0.0);
    assert_eq!(drift.files, 0.0);
    assert_eq!(drift.network, 0.0);
    assert_eq!(drift.services, 0.0);
    assert_eq!(drift.kernel, 0.0);
    assert_eq!(drift.packages, 0.0);
    assert_eq!(drift.gpu, 0.0);
}

/// Contract: shared-kernel.md § DriftWeights
/// Spec: default weights — kernel=2.0, gpu=2.0, all others=1.0
/// If this test didn't exist: wrong default weights would miscalculate commit windows.
#[test]
fn drift_weights_default_values() {
    let weights = DriftWeights::default();

    assert_eq!(weights.mounts, 1.0);
    assert_eq!(weights.files, 1.0);
    assert_eq!(weights.network, 1.0);
    assert_eq!(weights.services, 1.0);
    assert_eq!(weights.kernel, 2.0);
    assert_eq!(weights.packages, 1.0);
    assert_eq!(weights.gpu, 2.0);
}

// ===========================================================================
// VCluster Policy
// ===========================================================================

/// Contract: shared-kernel.md § VClusterPolicy
/// Spec: full 17-field VClusterPolicy must survive serde round-trip
/// If this test didn't exist: a policy field could be lost, bypassing enforcement.
#[test]
fn vcluster_policy_round_trip() {
    let policy = test_vcluster_policy();

    let json = serde_json::to_string(&policy).unwrap();
    let deserialized: VClusterPolicy = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.vcluster_id, policy.vcluster_id);
    assert_eq!(deserialized.policy_id, policy.policy_id);
    assert_eq!(deserialized.drift_sensitivity, policy.drift_sensitivity);
    assert_eq!(deserialized.base_commit_window_seconds, policy.base_commit_window_seconds);
    assert_eq!(deserialized.emergency_window_seconds, policy.emergency_window_seconds);
    assert_eq!(deserialized.auto_converge_categories, policy.auto_converge_categories);
    assert_eq!(deserialized.require_ack_categories, policy.require_ack_categories);
    assert_eq!(deserialized.enforcement_mode, policy.enforcement_mode);
    assert_eq!(deserialized.role_bindings.len(), 1);
    assert_eq!(deserialized.regulated, policy.regulated);
    assert_eq!(deserialized.two_person_approval, policy.two_person_approval);
    assert_eq!(deserialized.emergency_allowed, policy.emergency_allowed);
    assert_eq!(deserialized.audit_retention_days, policy.audit_retention_days);
    assert_eq!(deserialized.federation_template, policy.federation_template);
    assert_eq!(deserialized.supervisor_backend, policy.supervisor_backend);
    assert_eq!(deserialized.exec_whitelist, policy.exec_whitelist);
    assert_eq!(deserialized.shell_whitelist, policy.shell_whitelist);
}

/// Contract: shared-kernel.md § VClusterPolicy
/// Spec: ADR-002 — default impl must be permissive observe-only for bootstrap
/// If this test didn't exist: default policy could enforce on first boot, blocking nodes.
#[test]
fn vcluster_policy_default_is_observe_only() {
    let policy = VClusterPolicy::default();

    assert_eq!(policy.enforcement_mode, "observe");
    assert_eq!(policy.regulated, false);
    assert_eq!(policy.two_person_approval, false);
    assert_eq!(policy.emergency_allowed, true);
}

// ===========================================================================
// Boot Overlay
// ===========================================================================

/// Contract: shared-kernel.md § BootOverlay
/// Spec: BootOverlay with vcluster_id, version, data, checksum must round-trip
/// If this test didn't exist: overlay data could be corrupted during serde.
#[test]
fn boot_overlay_round_trip() {
    let data = vec![0x28, 0xB5, 0x2F, 0xFD, 0x01, 0x02, 0x03]; // zstd-like bytes
    let checksum = format!("sha256:{:x}", compute_hash(&data));

    let overlay = BootOverlay {
        vcluster_id: "ml-training".into(),
        version: 7,
        data: data.clone(),
        checksum: checksum.clone(),
    };

    let json = serde_json::to_string(&overlay).unwrap();
    let deserialized: BootOverlay = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.vcluster_id, overlay.vcluster_id);
    assert_eq!(deserialized.version, overlay.version);
    assert_eq!(deserialized.data, overlay.data);
    assert_eq!(deserialized.checksum, overlay.checksum);
}

/// Contract: shared-kernel.md § BootOverlay
/// Spec: J5 — checksum must equal hash(data)
/// If this test didn't exist: tampered overlays could be applied to nodes.
#[test]
fn boot_overlay_checksum_must_match_data() {
    let data = vec![0x01, 0x02, 0x03];
    let overlay = BootOverlay {
        vcluster_id: "ml-training".into(),
        version: 1,
        data,
        checksum: "sha256:0000000000000000".into(), // wrong checksum
    };

    let result = validate_boot_overlay(&overlay);
    assert!(result.is_err(), "mismatched checksum should be rejected");
}

// ===========================================================================
// Service Declaration & State
// ===========================================================================

/// Contract: shared-kernel.md § ServiceState
/// Spec: all 6 ServiceState variants must exist and round-trip
/// If this test didn't exist: a service state could be missing, breaking lifecycle tracking.
#[test]
fn service_state_all_variants() {
    let variants = vec![
        ServiceState::Starting,
        ServiceState::Running,
        ServiceState::Stopping,
        ServiceState::Stopped,
        ServiceState::Failed,
        ServiceState::Restarting,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: ServiceState = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § ServiceDecl
/// Spec: full ServiceDecl with all fields must survive serde round-trip
/// If this test didn't exist: a service field could be lost, breaking process supervision.
#[test]
fn service_decl_round_trip() {
    let decl = ServiceDecl {
        name: "lattice-node-agent".into(),
        binary: "/usr/bin/lattice-node-agent".into(),
        args: vec!["--config".into(), "/etc/lattice.toml".into()],
        restart: RestartPolicy::Always,
        restart_delay_seconds: 5,
        depends_on: vec!["chronyd".into()],
        order: 4,
        cgroup_memory_max: Some("8G".into()),
        health_check: Some(HealthCheck {
            check_type: HealthCheckType::Http { url: "http://localhost:9090/health".into() },
            interval_seconds: 30,
        }),
    };

    let json = serde_json::to_string(&decl).unwrap();
    let deserialized: ServiceDecl = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, decl.name);
    assert_eq!(deserialized.binary, decl.binary);
    assert_eq!(deserialized.args, decl.args);
    assert_eq!(deserialized.restart, decl.restart);
    assert_eq!(deserialized.restart_delay_seconds, decl.restart_delay_seconds);
    assert_eq!(deserialized.depends_on, decl.depends_on);
    assert_eq!(deserialized.order, decl.order);
    assert_eq!(deserialized.cgroup_memory_max, decl.cgroup_memory_max);
    assert!(deserialized.health_check.is_some());
}

/// Contract: shared-kernel.md § RestartPolicy
/// Spec: all 3 RestartPolicy variants must exist and round-trip
/// If this test didn't exist: a restart policy could be missing, defaulting to wrong behavior.
#[test]
fn restart_policy_all_variants() {
    let variants = vec![
        RestartPolicy::Always,
        RestartPolicy::OnFailure,
        RestartPolicy::Never,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: RestartPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § HealthCheckType
/// Spec: all 3 HealthCheckType variants (Process, Http, Tcp) must round-trip
/// If this test didn't exist: a health check type could fail to deserialize.
#[test]
fn health_check_type_all_variants() {
    let variants = vec![
        HealthCheckType::Process,
        HealthCheckType::Http { url: "http://localhost:9090/health".into() },
        HealthCheckType::Tcp { port: 8080 },
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: HealthCheckType = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

// ===========================================================================
// Capability Reporting
// ===========================================================================

/// Contract: shared-kernel.md § CapabilityReport
/// Spec: full CapabilityReport with all sub-structs must survive serde round-trip
/// If this test didn't exist: capability data could be lost, misscheduling workloads.
#[test]
fn capability_report_round_trip() {
    let report = test_capability_report();

    let json = serde_json::to_string(&report).unwrap();
    let deserialized: CapabilityReport = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.node_id, report.node_id);
    assert_eq!(deserialized.report_id, report.report_id);
    assert_eq!(deserialized.gpus.len(), 1);
    assert_eq!(deserialized.gpus[0].model, "A100");
    assert_eq!(deserialized.memory.total_bytes, report.memory.total_bytes);
    assert!(deserialized.network.is_some());
    assert_eq!(deserialized.storage.mounts.len(), 1);
    assert_eq!(deserialized.software.loaded_modules.len(), 2);
    assert_eq!(deserialized.config_state, ConfigState::Committed);
    assert!(deserialized.drift_summary.is_none());
    assert!(deserialized.emergency.is_none());
    assert_eq!(deserialized.supervisor_status.services_declared, 4);
}

/// Contract: shared-kernel.md § GpuCapability
/// Spec: GpuCapability with vendor, model, health must survive serde round-trip
/// If this test didn't exist: GPU info could be lost, misscheduling GPU workloads.
#[test]
fn gpu_capability_round_trip() {
    let gpu = GpuCapability {
        index: 0,
        vendor: GpuVendor::Nvidia,
        model: "H100".into(),
        memory_bytes: 80_000_000_000,
        health: GpuHealth::Healthy,
        pci_bus_id: "0000:3b:00.0".into(),
    };

    let json = serde_json::to_string(&gpu).unwrap();
    let deserialized: GpuCapability = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.index, gpu.index);
    assert_eq!(deserialized.vendor, gpu.vendor);
    assert_eq!(deserialized.model, gpu.model);
    assert_eq!(deserialized.memory_bytes, gpu.memory_bytes);
    assert_eq!(deserialized.health, gpu.health);
    assert_eq!(deserialized.pci_bus_id, gpu.pci_bus_id);
}

/// Contract: shared-kernel.md § GpuHealth
/// Spec: all 3 GpuHealth variants must exist and round-trip
/// If this test didn't exist: a GPU health state could be missing, hiding hardware issues.
#[test]
fn gpu_health_all_variants() {
    let variants = vec![
        GpuHealth::Healthy,
        GpuHealth::Degraded,
        GpuHealth::Failed,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: GpuHealth = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § SupervisorStatus
/// Spec: SupervisorStatus with backend, declared, running, failed counts must round-trip
/// If this test didn't exist: supervisor health reporting could lose data.
#[test]
fn supervisor_status_round_trip() {
    let status = SupervisorStatus {
        backend: SupervisorBackend::Pact,
        services_declared: 5,
        services_running: 4,
        services_failed: 1,
    };

    let json = serde_json::to_string(&status).unwrap();
    let deserialized: SupervisorStatus = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.backend, status.backend);
    assert_eq!(deserialized.services_declared, status.services_declared);
    assert_eq!(deserialized.services_running, status.services_running);
    assert_eq!(deserialized.services_failed, status.services_failed);
}

// ===========================================================================
// Admin Operations & Audit
// ===========================================================================

/// Contract: shared-kernel.md § AdminOperation
/// Spec: full AdminOperation with all fields must survive serde round-trip
/// If this test didn't exist: audit log entries could lose fields, breaking compliance.
#[test]
fn admin_operation_round_trip() {
    let op = AdminOperation {
        operation_id: "op-001".into(),
        timestamp: Utc::now(),
        actor: test_identity("admin@example.com", "pact-ops-ml-training"),
        operation_type: AdminOperationType::Exec,
        scope: Scope::Node("compute-042".into()),
        detail: "nvidia-smi -L".into(),
    };

    let json = serde_json::to_string(&op).unwrap();
    let deserialized: AdminOperation = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.operation_id, op.operation_id);
    assert_eq!(deserialized.actor.principal, op.actor.principal);
    assert_eq!(deserialized.operation_type, op.operation_type);
    assert_eq!(deserialized.scope, op.scope);
    assert_eq!(deserialized.detail, op.detail);
}

/// Contract: shared-kernel.md § AdminOperationType
/// Spec: all 13 AdminOperationType variants must exist and round-trip
/// If this test didn't exist: an admin action could be unrepresentable in the audit log.
#[test]
fn admin_operation_type_all_variants() {
    let variants = vec![
        AdminOperationType::Exec,
        AdminOperationType::ShellSessionStart,
        AdminOperationType::ShellSessionEnd,
        AdminOperationType::ServiceStart,
        AdminOperationType::ServiceStop,
        AdminOperationType::ServiceRestart,
        AdminOperationType::EmergencyStart,
        AdminOperationType::EmergencyEnd,
        AdminOperationType::ApprovalDecision,
        AdminOperationType::NodeEnroll,
        AdminOperationType::NodeDecommission,
        AdminOperationType::NodeAssign,
        AdminOperationType::NodeUnassign,
        AdminOperationType::NodeMove,
    ];

    assert_eq!(variants.len(), 14, "spec defines 14 AdminOperationType variants");

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: AdminOperationType = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}

/// Contract: shared-kernel.md § PendingApproval
/// Spec: PendingApproval with requester, optional approver must round-trip
/// If this test didn't exist: two-person approval state could be lost from the journal.
#[test]
fn pending_approval_round_trip() {
    let approval = PendingApproval {
        approval_id: "appr-001".into(),
        original_request: "entry-seq-42".into(),
        action: "commit".into(),
        scope: Scope::VCluster("regulated-cluster".into()),
        requester: test_identity("ops@example.com", "pact-regulated-cluster"),
        approver: Some(test_identity("senior@example.com", "pact-regulated-cluster")),
        status: ApprovalStatus::Approved,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(4),
    };

    let json = serde_json::to_string(&approval).unwrap();
    let deserialized: PendingApproval = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.approval_id, approval.approval_id);
    assert_eq!(deserialized.original_request, approval.original_request);
    assert_eq!(deserialized.action, approval.action);
    assert_eq!(deserialized.scope, approval.scope);
    assert_eq!(deserialized.requester.principal, approval.requester.principal);
    assert!(deserialized.approver.is_some());
    assert_eq!(deserialized.approver.unwrap().principal, "senior@example.com");
    assert_eq!(deserialized.status, approval.status);
}

/// Contract: shared-kernel.md § ApprovalStatus
/// Spec: all 4 ApprovalStatus variants must exist and round-trip
/// If this test didn't exist: an approval state could be unrepresentable, stalling workflows.
#[test]
fn approval_status_all_variants() {
    let variants = vec![
        ApprovalStatus::Pending,
        ApprovalStatus::Approved,
        ApprovalStatus::Rejected,
        ApprovalStatus::Expired,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: ApprovalStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, variant);
    }
}
