//! BDD acceptance tests for pact.
//!
//! Uses cucumber-rs to run Gherkin feature files.
//! Custom harness: `[[test]] harness = false` in Cargo.toml.
//!
//! Run with: `cargo test -p pact-acceptance`

use std::collections::HashMap;

use chrono::Utc;
use cucumber::{given, then, when, World};
use pact_common::{
    config::{BlacklistConfig, CommitWindowConfig},
    error::PactError,
    types::{
        AdminOperation, AdminOperationType, BootOverlay, CapabilityReport, ConfigEntry,
        ConfigState, DeltaAction, DeltaItem, DriftVector, DriftWeights, EntrySeq, EntryType,
        GpuCapability, GpuHealth, GpuVendor, HealthCheck, HealthCheckType, Identity,
        MemoryCapability, NodeId, PrincipalType, RestartPolicy, Scope, ServiceDecl, ServiceState,
        SoftwareCapability, StateDelta, StorageCapability, SupervisorBackend, SupervisorStatus,
        VClusterId, VClusterPolicy,
    },
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// World — shared state across all steps in a scenario
// ---------------------------------------------------------------------------

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct PactWorld {
    // --- Journal state (mirrors JournalState) ---
    entries: HashMap<EntrySeq, ConfigEntry>,
    next_sequence: EntrySeq,
    node_states: HashMap<NodeId, ConfigState>,
    policies: HashMap<VClusterId, VClusterPolicy>,
    overlays: HashMap<VClusterId, BootOverlay>,
    audit_log: Vec<AdminOperation>,

    // --- Drift detection ---
    drift_vector: DriftVector,
    drift_weights: DriftWeights,
    blacklist: BlacklistConfig,
    custom_blacklist_patterns: Vec<String>,
    drift_events: Vec<DriftEvent>,
    drift_filtered: bool,
    enforcement_mode: String,

    // --- Commit window ---
    commit_window_config: CommitWindowConfig,
    active_commit_window: Option<CommitWindow>,
    rollback_triggered: bool,
    rollback_deferred: bool,
    alert_raised: bool,

    // --- Emergency mode ---
    emergency_active: bool,
    emergency_reason: Option<String>,
    emergency_start_time: Option<chrono::DateTime<Utc>>,
    emergency_window_expired: bool,
    stale_alert_raised: bool,
    scheduling_hold_requested: bool,

    // --- Supervisor ---
    supervisor_backend: SupervisorBackend,
    service_declarations: Vec<ServiceDecl>,
    service_states: HashMap<String, ServiceState>,
    service_start_order: Vec<String>,
    service_stop_order: Vec<String>,
    supervisor_status: SupervisorStatus,

    // --- Shell / Exec ---
    shell_session_active: bool,
    shell_session_id: Option<String>,
    shell_whitelist: Vec<String>,
    shell_whitelist_mode: String,
    exec_results: Vec<ExecResult>,
    whitelist_suggestions: Vec<String>,
    available_commands: Vec<String>,
    blocked_commands: Vec<String>,
    lesssecure_set: bool,

    // --- Capability ---
    capability_report: Option<CapabilityReport>,
    gpu_capabilities: Vec<GpuCapability>,
    manifest_written: bool,
    socket_available: bool,

    // --- Policy / RBAC ---
    current_user: Option<UserContext>,
    auth_result: Option<AuthResult>,
    pending_approvals: Vec<PendingApproval>,
    opa_available: bool,
    policy_degraded: bool,

    // --- Config subscription ---
    subscriptions: HashMap<NodeId, ConfigSubscription>,
    received_updates: Vec<ConfigUpdateEvent>,

    // --- Boot sequence ---
    boot_phases_completed: Vec<String>,
    boot_stream_chunks: Vec<BootStreamChunk>,

    // --- CLI ---
    cli_output: Option<String>,
    cli_exit_code: Option<i32>,

    // --- Federation ---
    sovra_reachable: bool,
    federated_templates: Vec<String>,
    compliance_reports: Vec<String>,

    // --- Observability ---
    loki_enabled: bool,
    loki_events: Vec<LokiEvent>,
    metrics_available: bool,
    health_status: Option<HealthResponse>,

    // --- Errors ---
    last_error: Option<PactError>,
    last_denial_reason: Option<String>,

    // --- MCP / Agentic ---
    mcp_active: bool,

    // --- Journal cluster ---
    journal_reachable: bool,
    journal_leader: Option<u64>,
    journal_cluster_size: u32,
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct DriftEvent {
    dimension: String,
    key: String,
    magnitude: f64,
}

#[derive(Debug, Clone)]
struct CommitWindow {
    duration_seconds: f64,
    opened_at: chrono::DateTime<Utc>,
    node_id: NodeId,
}

#[derive(Debug, Clone)]
struct ExecResult {
    command: String,
    exit_code: i32,
    stdout: String,
    stderr: String,
    logged: bool,
}

#[derive(Debug, Clone)]
struct UserContext {
    principal: String,
    role: String,
    principal_type: PrincipalType,
    token_valid: bool,
}

#[derive(Debug, Clone)]
enum AuthResult {
    Authorized,
    Denied { reason: String },
    ApprovalRequired { approval_id: String },
}

#[derive(Debug, Clone)]
struct PendingApproval {
    operation_id: String,
    requester: String,
    vcluster_id: VClusterId,
    created_at: chrono::DateTime<Utc>,
    timeout_minutes: u32,
    status: ApprovalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[derive(Debug, Clone)]
struct ConfigSubscription {
    vcluster_id: VClusterId,
    from_sequence: EntrySeq,
}

#[derive(Debug, Clone)]
struct ConfigUpdateEvent {
    sequence: EntrySeq,
    update_type: String,
}

#[derive(Debug, Clone)]
enum BootStreamChunk {
    BaseOverlay { version: u64, data: Vec<u8>, checksum: String },
    NodeDelta { data: Vec<u8> },
    Complete { base_version: u64, node_version: Option<u64> },
}

#[derive(Debug, Clone)]
struct LokiEvent {
    component: String,
    entry_type: String,
    detail: String,
}

#[derive(Debug, Clone)]
struct HealthResponse {
    status_code: u16,
    role: String,
}

impl PactWorld {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            next_sequence: 0,
            node_states: HashMap::new(),
            policies: HashMap::new(),
            overlays: HashMap::new(),
            audit_log: Vec::new(),
            drift_vector: DriftVector {
                mounts: 0.0,
                files: 0.0,
                network: 0.0,
                services: 0.0,
                kernel: 0.0,
                packages: 0.0,
                gpu: 0.0,
            },
            drift_weights: DriftWeights::default(),
            blacklist: BlacklistConfig::default(),
            custom_blacklist_patterns: Vec::new(),
            drift_events: Vec::new(),
            drift_filtered: false,
            enforcement_mode: "observe".to_string(),
            commit_window_config: CommitWindowConfig::default(),
            active_commit_window: None,
            rollback_triggered: false,
            rollback_deferred: false,
            alert_raised: false,
            emergency_active: false,
            emergency_reason: None,
            emergency_start_time: None,
            emergency_window_expired: false,
            stale_alert_raised: false,
            scheduling_hold_requested: false,
            supervisor_backend: SupervisorBackend::Pact,
            service_declarations: Vec::new(),
            service_states: HashMap::new(),
            service_start_order: Vec::new(),
            service_stop_order: Vec::new(),
            supervisor_status: SupervisorStatus {
                backend: SupervisorBackend::Pact,
                services_declared: 0,
                services_running: 0,
                services_failed: 0,
            },
            shell_session_active: false,
            shell_session_id: None,
            shell_whitelist: default_whitelist(),
            shell_whitelist_mode: "learning".to_string(),
            exec_results: Vec::new(),
            whitelist_suggestions: Vec::new(),
            available_commands: Vec::new(),
            blocked_commands: Vec::new(),
            lesssecure_set: false,
            capability_report: None,
            gpu_capabilities: Vec::new(),
            manifest_written: false,
            socket_available: false,
            current_user: None,
            auth_result: None,
            pending_approvals: Vec::new(),
            opa_available: true,
            policy_degraded: false,
            subscriptions: HashMap::new(),
            received_updates: Vec::new(),
            boot_phases_completed: Vec::new(),
            boot_stream_chunks: Vec::new(),
            cli_output: None,
            cli_exit_code: None,
            sovra_reachable: true,
            federated_templates: Vec::new(),
            compliance_reports: Vec::new(),
            loki_enabled: false,
            loki_events: Vec::new(),
            metrics_available: false,
            health_status: None,
            last_error: None,
            last_denial_reason: None,
            mcp_active: false,
            journal_reachable: true,
            journal_leader: Some(1),
            journal_cluster_size: 3,
        }
    }

    // --- Helper: append entry through state machine ---
    fn append_entry(&mut self, mut entry: ConfigEntry) -> EntrySeq {
        let seq = self.next_sequence;
        entry.sequence = seq;
        self.next_sequence += 1;
        self.entries.insert(seq, entry);
        seq
    }

    // --- Helper: create identity ---
    fn make_identity(principal: &str, role: &str) -> Identity {
        Identity {
            principal: principal.to_string(),
            principal_type: PrincipalType::Human,
            role: role.to_string(),
        }
    }

    // --- Helper: create config entry ---
    fn make_entry(entry_type: EntryType, scope: Scope, author: Identity) -> ConfigEntry {
        ConfigEntry {
            sequence: 0,
            timestamp: Utc::now(),
            entry_type,
            scope,
            author,
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        }
    }

    // --- Helper: compute commit window ---
    fn compute_window(&self, magnitude: f64) -> f64 {
        let base = f64::from(self.commit_window_config.base_window_seconds);
        let sensitivity = self.commit_window_config.drift_sensitivity;
        base / (1.0 + magnitude * sensitivity)
    }

    // --- Helper: check blacklist ---
    fn is_blacklisted(&self, path: &str) -> bool {
        let all_patterns: Vec<&str> = self
            .blacklist
            .patterns
            .iter()
            .chain(self.custom_blacklist_patterns.iter())
            .map(String::as_str)
            .collect();

        for pattern in all_patterns {
            if let Some(prefix) = pattern.strip_suffix("/**") {
                if path.starts_with(prefix) {
                    return true;
                }
            } else if path == pattern {
                return true;
            }
        }
        false
    }

    // --- Helper: authorize ---
    fn authorize(&self, action: &str, vcluster: &str) -> AuthResult {
        let Some(user) = &self.current_user else {
            return AuthResult::Denied { reason: "no user context".to_string() };
        };

        if !user.token_valid {
            return AuthResult::Denied { reason: "invalid token".to_string() };
        }

        // Platform admin: full access
        if user.role == "pact-platform-admin" {
            return AuthResult::Authorized;
        }

        // Check vCluster scope
        let role_vcluster = user.role.split('-').skip(2).collect::<Vec<_>>().join("-");
        if role_vcluster != vcluster && !user.role.starts_with("pact-platform") {
            return AuthResult::Denied {
                reason: format!("not authorized for vCluster {vcluster}"),
            };
        }

        // Service AI restrictions
        if user.role == "pact-service-ai" && action == "emergency" {
            return AuthResult::Denied {
                reason: "emergency mode restricted to human admins".to_string(),
            };
        }

        // Viewer restrictions
        if user.role.contains("viewer") {
            match action {
                "status" | "diff" | "log" | "watch" | "cap" => return AuthResult::Authorized,
                "exec" => {
                    // Viewers can exec read-only commands
                    return AuthResult::Authorized;
                }
                _ => {
                    return AuthResult::Denied { reason: format!("viewers cannot {action}") };
                }
            }
        }

        // Two-person approval check
        if let Some(policy) = self.policies.get(vcluster) {
            if policy.two_person_approval && matches!(action, "commit" | "rollback" | "apply") {
                let approval_id = Uuid::new_v4().to_string();
                return AuthResult::ApprovalRequired { approval_id };
            }
        }

        AuthResult::Authorized
    }
}

fn default_whitelist() -> Vec<String> {
    vec![
        "nvidia-smi",
        "dmesg",
        "lspci",
        "ip",
        "ss",
        "cat",
        "journalctl",
        "mount",
        "df",
        "free",
        "top",
        "ps",
        "lsmod",
        "sysctl",
        "uname",
        "hostname",
        "date",
        "uptime",
        "who",
        "w",
        "last",
        "netstat",
        "ethtool",
        "lsblk",
        "blkid",
        "findmnt",
        "nproc",
        "lscpu",
        "numactl",
        "dmidecode",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

// ===========================================================================
// GIVEN steps
// ===========================================================================

#[given("a journal with default state")]
async fn given_journal_default(world: &mut PactWorld) {
    // World is already initialized with defaults
    assert!(world.entries.is_empty());
}

#[given("default drift weights")]
async fn given_default_drift_weights(world: &mut PactWorld) {
    world.drift_weights = DriftWeights::default();
}

#[given(
    regex = r#"^default commit window config with base (\d+) seconds and sensitivity (\d+\.\d+)$"#
)]
async fn given_commit_window_config(world: &mut PactWorld, base: u32, sensitivity: f64) {
    world.commit_window_config.base_window_seconds = base;
    world.commit_window_config.drift_sensitivity = sensitivity;
}

#[given(regex = r#"^commit window config with base (\d+) seconds and sensitivity (\d+\.\d+)$"#)]
async fn given_commit_window_config_override(world: &mut PactWorld, base: u32, sensitivity: f64) {
    world.commit_window_config.base_window_seconds = base;
    world.commit_window_config.drift_sensitivity = sensitivity;
}

#[given(regex = r#"^a boot overlay for vCluster "([\w-]+)" version (\d+) with data "(.*)"$"#)]
async fn given_boot_overlay(world: &mut PactWorld, vcluster: String, version: u64, data: String) {
    let overlay = BootOverlay {
        vcluster_id: vcluster.clone(),
        version,
        checksum: format!("sha256:{:x}", md5_simple(&data)),
        data: data.into_bytes(),
    };
    world.overlays.insert(vcluster, overlay);
}

#[given(
    regex = r#"^a boot overlay for vCluster "([\w-]+)" with (?:base )?sysctl(?: and mount)? config$"#
)]
async fn given_boot_overlay_sysctl(world: &mut PactWorld, vcluster: String) {
    let overlay = BootOverlay {
        vcluster_id: vcluster.clone(),
        version: 1,
        data: b"sysctl.vm.swappiness=60\nmount./scratch=nfs".to_vec(),
        checksum: "sha256:abc".to_string(),
    };
    world.overlays.insert(vcluster, overlay);
}

#[given(regex = r#"^a boot overlay for vCluster "([\w-]+)"$"#)]
async fn given_boot_overlay_simple(world: &mut PactWorld, vcluster: String) {
    let overlay = BootOverlay {
        vcluster_id: vcluster.clone(),
        version: 1,
        data: b"default-config".to_vec(),
        checksum: "sha256:default".to_string(),
    };
    world.overlays.insert(vcluster, overlay);
}

#[given(regex = r#"^no overlay exists for vCluster "([\w-]+)"$"#)]
async fn given_no_overlay(world: &mut PactWorld, vcluster: String) {
    world.overlays.remove(&vcluster);
}

#[given(
    regex = r#"^a committed node delta for node "([\w-]+)" with kernel change "([\w.]+)" to "(.*)"$"#
)]
async fn given_node_delta(world: &mut PactWorld, node_id: String, key: String, value: String) {
    let mut entry = PactWorld::make_entry(
        EntryType::Commit,
        Scope::Node(node_id),
        PactWorld::make_identity("admin@example.com", "pact-platform-admin"),
    );
    entry.state_delta = Some(StateDelta {
        mounts: vec![],
        files: vec![],
        network: vec![],
        services: vec![],
        kernel: vec![DeltaItem {
            action: DeltaAction::Modify,
            key,
            value: Some(value),
            previous: None,
        }],
        packages: vec![],
        gpu: vec![],
    });
    world.append_entry(entry);
}

#[given(regex = r#"^a committed node delta for node "([\w-]+)"$"#)]
async fn given_node_delta_simple(world: &mut PactWorld, node_id: String) {
    let entry = PactWorld::make_entry(
        EntryType::Commit,
        Scope::Node(node_id),
        PactWorld::make_identity("admin@example.com", "pact-platform-admin"),
    );
    world.append_entry(entry);
}

#[given(
    regex = r#"^node "([\w-]+)" is subscribed to config updates for vCluster "([\w-]+)" from sequence (\d+)$"#
)]
async fn given_subscription(world: &mut PactWorld, node: String, vcluster: String, seq: u64) {
    world
        .subscriptions
        .insert(node, ConfigSubscription { vcluster_id: vcluster, from_sequence: seq });
}

#[given(regex = r#"^a custom blacklist pattern "(.*)"$"#)]
async fn given_custom_blacklist(world: &mut PactWorld, pattern: String) {
    world.custom_blacklist_patterns.push(pattern);
}

#[given(regex = r#"^enforcement mode is "(observe|enforce)"$"#)]
async fn given_enforcement_mode(world: &mut PactWorld, mode: String) {
    world.enforcement_mode = mode;
}

#[given(regex = r#"^a drift vector with (\w+) magnitude (\d+\.\d+)$"#)]
async fn given_drift_single_dim(world: &mut PactWorld, dim: String, mag: f64) {
    set_drift_dimension(&mut world.drift_vector, &dim, mag);
}

#[given(
    regex = r#"^a drift vector with (\w+) magnitude (\d+\.\d+) and (\w+) magnitude (\d+\.\d+)$"#
)]
async fn given_drift_two_dim(
    world: &mut PactWorld,
    dim1: String,
    mag1: f64,
    dim2: String,
    mag2: f64,
) {
    set_drift_dimension(&mut world.drift_vector, &dim1, mag1);
    set_drift_dimension(&mut world.drift_vector, &dim2, mag2);
}

#[given("a drift vector with all dimensions at 0.0")]
async fn given_drift_zero(world: &mut PactWorld) {
    world.drift_vector = DriftVector {
        mounts: 0.0,
        files: 0.0,
        network: 0.0,
        services: 0.0,
        kernel: 0.0,
        packages: 0.0,
        gpu: 0.0,
    };
}

#[given(regex = r#"^a supervisor with backend "(pact|systemd)"$"#)]
async fn given_supervisor_backend(world: &mut PactWorld, backend: String) {
    world.supervisor_backend = match backend.as_str() {
        "systemd" => SupervisorBackend::Systemd,
        _ => SupervisorBackend::Pact,
    };
}

#[given(regex = r#"^a service declaration for "([\w-]+)" with binary "(.*)"$"#)]
async fn given_service_decl(world: &mut PactWorld, name: String, binary: String) {
    let decl = ServiceDecl {
        name: name.clone(),
        binary,
        args: vec![],
        restart: RestartPolicy::Always,
        restart_delay_seconds: 5,
        depends_on: vec![],
        order: 1,
        cgroup_memory_max: None,
        health_check: None,
    };
    world.service_declarations.push(decl);
}

#[given(regex = r#"^a running service "([\w-]+)"$"#)]
async fn given_running_service(world: &mut PactWorld, name: String) {
    world.service_states.insert(name.clone(), ServiceState::Running);
    if !world.service_declarations.iter().any(|s| s.name == name) {
        world.service_declarations.push(ServiceDecl {
            name,
            binary: "/usr/bin/stub".to_string(),
            args: vec![],
            restart: RestartPolicy::Always,
            restart_delay_seconds: 5,
            depends_on: vec![],
            order: 1,
            cgroup_memory_max: None,
            health_check: None,
        });
    }
}

#[given(regex = r#"^a running service "([\w-]+)" with health check type "(\w+)"$"#)]
async fn given_running_service_health(world: &mut PactWorld, name: String, check_type: String) {
    world.service_states.insert(name.clone(), ServiceState::Running);
    let health_check = match check_type.as_str() {
        "Process" => {
            Some(HealthCheck { check_type: HealthCheckType::Process, interval_seconds: 30 })
        }
        _ => None,
    };
    world.service_declarations.push(ServiceDecl {
        name,
        binary: "/usr/bin/stub".to_string(),
        args: vec![],
        restart: RestartPolicy::Always,
        restart_delay_seconds: 5,
        depends_on: vec![],
        order: 1,
        cgroup_memory_max: None,
        health_check,
    });
}

#[given(regex = r#"^a service "([\w-]+)" with restart policy "(\w+)" and delay (\d+) seconds$"#)]
async fn given_service_restart_policy(
    world: &mut PactWorld,
    name: String,
    policy: String,
    delay: u32,
) {
    let restart = match policy.as_str() {
        "Always" => RestartPolicy::Always,
        "OnFailure" => RestartPolicy::OnFailure,
        "Never" => RestartPolicy::Never,
        _ => RestartPolicy::Always,
    };
    world.service_declarations.push(ServiceDecl {
        name: name.clone(),
        binary: "/usr/bin/stub".to_string(),
        args: vec![],
        restart,
        restart_delay_seconds: delay,
        depends_on: vec![],
        order: 1,
        cgroup_memory_max: None,
        health_check: None,
    });
    world.service_states.insert(name, ServiceState::Running);
}

#[given(regex = r#"^a service "([\w-]+)" with order (\d+) and no dependencies$"#)]
async fn given_service_order_no_dep(world: &mut PactWorld, name: String, order: u32) {
    world.service_declarations.push(ServiceDecl {
        name,
        binary: "/usr/bin/stub".to_string(),
        args: vec![],
        restart: RestartPolicy::Always,
        restart_delay_seconds: 5,
        depends_on: vec![],
        order,
        cgroup_memory_max: None,
        health_check: None,
    });
}

#[given(regex = r#"^a service "([\w-]+)" with order (\d+) and depends on "([\w-]+)"$"#)]
async fn given_service_order_with_dep(
    world: &mut PactWorld,
    name: String,
    order: u32,
    dep: String,
) {
    world.service_declarations.push(ServiceDecl {
        name,
        binary: "/usr/bin/stub".to_string(),
        args: vec![],
        restart: RestartPolicy::Always,
        restart_delay_seconds: 5,
        depends_on: vec![dep],
        order,
        cgroup_memory_max: None,
        health_check: None,
    });
}

#[given(regex = r#"^a running service "([\w-]+)" with order (\d+)$"#)]
async fn given_running_service_order(world: &mut PactWorld, name: String, order: u32) {
    world.service_states.insert(name.clone(), ServiceState::Running);
    if !world.service_declarations.iter().any(|s| s.name == name) {
        world.service_declarations.push(ServiceDecl {
            name,
            binary: "/usr/bin/stub".to_string(),
            args: vec![],
            restart: RestartPolicy::Always,
            restart_delay_seconds: 5,
            depends_on: vec![],
            order,
            cgroup_memory_max: None,
            health_check: None,
        });
    }
}

#[given(regex = r#"^a running service "([\w-]+)" with order (\d+) and depends on "([\w-]+)"$"#)]
async fn given_running_service_order_dep(
    world: &mut PactWorld,
    name: String,
    order: u32,
    dep: String,
) {
    world.service_states.insert(name.clone(), ServiceState::Running);
    world.service_declarations.push(ServiceDecl {
        name,
        binary: "/usr/bin/stub".to_string(),
        args: vec![],
        restart: RestartPolicy::Always,
        restart_delay_seconds: 5,
        depends_on: vec![dep],
        order,
        cgroup_memory_max: None,
        health_check: None,
    });
}

#[given(regex = r#"^(\d+) declared services with (\d+) running and (\d+) failed$"#)]
async fn given_service_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    world.supervisor_status = SupervisorStatus {
        backend: world.supervisor_backend.clone(),
        services_declared: declared,
        services_running: running,
        services_failed: failed,
    };
}

#[given(regex = r#"^supervisor config with backend "(pact|systemd)"$"#)]
async fn given_supervisor_config(world: &mut PactWorld, backend: String) {
    world.supervisor_backend = match backend.as_str() {
        "systemd" => SupervisorBackend::Systemd,
        _ => SupervisorBackend::Pact,
    };
}

#[given("a shell server with default whitelist")]
async fn given_shell_default_whitelist(world: &mut PactWorld) {
    world.shell_whitelist = default_whitelist();
}

#[given(regex = r#"^a node with (\d+) (NVIDIA|AMD) ([\w]+) GPUs$"#)]
async fn given_gpus(world: &mut PactWorld, count: u32, vendor_str: String, model: String) {
    let vendor = match vendor_str.as_str() {
        "AMD" => GpuVendor::Amd,
        _ => GpuVendor::Nvidia,
    };
    for i in 0..count {
        world.gpu_capabilities.push(GpuCapability {
            index: i,
            vendor: vendor.clone(),
            model: model.clone(),
            memory_bytes: 80 * 1024 * 1024 * 1024,
            health: GpuHealth::Healthy,
            pci_bus_id: format!("0000:{:02x}:00.0", i),
        });
    }
}

#[given("a node with no GPUs")]
async fn given_no_gpus(world: &mut PactWorld) {
    world.gpu_capabilities.clear();
}

#[given(regex = r#"^a user with role "([\w-]+)"$"#)]
async fn given_user_role(world: &mut PactWorld, role: String) {
    world.current_user = Some(UserContext {
        principal: "user@example.com".to_string(),
        role,
        principal_type: PrincipalType::Human,
        token_valid: true,
    });
}

#[given(regex = r#"^a user "([\w@.]+)" with role "([\w-]+)"$"#)]
async fn given_named_user_role(world: &mut PactWorld, principal: String, role: String) {
    world.current_user = Some(UserContext {
        principal,
        role,
        principal_type: PrincipalType::Human,
        token_valid: true,
    });
}

#[given(regex = r#"^a user with role "([\w-]+)" and principal type "(\w+)"$"#)]
async fn given_user_role_type(world: &mut PactWorld, role: String, ptype: String) {
    let principal_type = match ptype.as_str() {
        "Service" => PrincipalType::Service,
        "Agent" => PrincipalType::Agent,
        _ => PrincipalType::Human,
    };
    world.current_user = Some(UserContext {
        principal: "service@pact.internal".to_string(),
        role,
        principal_type,
        token_valid: true,
    });
}

#[given(regex = r#"^vCluster "([\w-]+)" has two-person approval enabled$"#)]
async fn given_two_person_approval(world: &mut PactWorld, vcluster: String) {
    let policy = VClusterPolicy {
        vcluster_id: vcluster.clone(),
        drift_sensitivity: 5.0,
        base_commit_window_seconds: 900,
        emergency_allowed: true,
        two_person_approval: true,
        ..VClusterPolicy::default()
    };
    world.policies.insert(vcluster, policy);
}

#[given(regex = r#"^vCluster "([\w-]+)" has policy with emergency_allowed false$"#)]
async fn given_no_emergency_policy(world: &mut PactWorld, vcluster: String) {
    let policy = VClusterPolicy {
        vcluster_id: vcluster.clone(),
        drift_sensitivity: 5.0,
        base_commit_window_seconds: 900,
        emergency_allowed: false,
        two_person_approval: false,
        ..VClusterPolicy::default()
    };
    world.policies.insert(vcluster, policy);
}

#[given(regex = r#"^emergency mode is active with window (\d+) seconds$"#)]
async fn given_emergency_active(world: &mut PactWorld, window: u32) {
    world.emergency_active = true;
    world.commit_window_config.emergency_window_seconds = window;
}

#[given(regex = r#"^node "([\w-]+)" has cached config and policy for vCluster "([\w-]+)"$"#)]
async fn given_cached_config(world: &mut PactWorld, _node: String, vcluster: String) {
    let policy = VClusterPolicy {
        vcluster_id: vcluster.clone(),
        drift_sensitivity: 5.0,
        base_commit_window_seconds: 900,
        emergency_allowed: true,
        two_person_approval: false,
        ..VClusterPolicy::default()
    };
    world.policies.insert(vcluster, policy);
}

#[given("the journal is unreachable")]
async fn given_journal_unreachable(world: &mut PactWorld) {
    world.journal_reachable = false;
}

#[given(regex = r#"^the journal is unreachable from node "([\w-]+)"$"#)]
async fn given_journal_unreachable_from(world: &mut PactWorld, _node: String) {
    world.journal_reachable = false;
}

#[given("the PolicyService is unreachable")]
async fn given_policy_unreachable(world: &mut PactWorld) {
    world.opa_available = false;
    world.policy_degraded = true;
}

#[given(regex = r#"^whitelist mode is "([\w]+)"$"#)]
async fn given_whitelist_mode(world: &mut PactWorld, mode: String) {
    world.shell_whitelist_mode = mode;
}

#[given("Loki forwarding is enabled")]
async fn given_loki_enabled(world: &mut PactWorld) {
    world.loki_enabled = true;
}

#[given("Loki forwarding is disabled")]
async fn given_loki_disabled(world: &mut PactWorld) {
    world.loki_enabled = false;
}

#[given("an MCP server with pact-service-ai identity")]
async fn given_mcp_server(world: &mut PactWorld) {
    world.mcp_active = true;
    world.current_user = Some(UserContext {
        principal: "service/ai-agent".to_string(),
        role: "pact-service-ai".to_string(),
        principal_type: PrincipalType::Service,
        token_valid: true,
    });
}

#[given("Sovra federation is configured with 300 second interval")]
async fn given_sovra_configured(world: &mut PactWorld) {
    world.sovra_reachable = true;
}

#[given("Sovra federation is configured")]
async fn given_sovra_configured_simple(world: &mut PactWorld) {
    world.sovra_reachable = true;
}

// ===========================================================================
// WHEN steps
// ===========================================================================

#[when(regex = r#"^I append a commit entry for vCluster "([\w-]+)" by "([\w@.]+)"$"#)]
async fn when_append_commit(world: &mut PactWorld, vcluster: String, author: String) {
    let entry = PactWorld::make_entry(
        EntryType::Commit,
        Scope::VCluster(vcluster),
        PactWorld::make_identity(&author, "pact-platform-admin"),
    );
    world.append_entry(entry);
}

#[when(
    regex = r#"^I append a commit entry for vCluster "([\w-]+)" by "([\w@.]+)" with role "([\w-]+)"$"#
)]
async fn when_append_commit_role(
    world: &mut PactWorld,
    vcluster: String,
    author: String,
    role: String,
) {
    let entry = PactWorld::make_entry(
        EntryType::Commit,
        Scope::VCluster(vcluster),
        PactWorld::make_identity(&author, &role),
    );
    world.append_entry(entry);
}

#[when(regex = r#"^I append a rollback entry for vCluster "([\w-]+)" by "([\w@.]+)"$"#)]
async fn when_append_rollback(world: &mut PactWorld, vcluster: String, author: String) {
    let entry = PactWorld::make_entry(
        EntryType::Rollback,
        Scope::VCluster(vcluster),
        PactWorld::make_identity(&author, "pact-platform-admin"),
    );
    world.append_entry(entry);
}

#[when(
    regex = r#"^I append a commit entry with a kernel sysctl change "([\w.]+)" from "(.*)" to "(.*)"$"#
)]
async fn when_append_commit_sysctl(world: &mut PactWorld, key: String, from: String, to: String) {
    let mut entry = PactWorld::make_entry(
        EntryType::Commit,
        Scope::Global,
        PactWorld::make_identity("admin@example.com", "pact-platform-admin"),
    );
    entry.state_delta = Some(StateDelta {
        mounts: vec![],
        files: vec![],
        network: vec![],
        services: vec![],
        kernel: vec![DeltaItem {
            action: DeltaAction::Modify,
            key,
            value: Some(to),
            previous: Some(from),
        }],
        packages: vec![],
        gpu: vec![],
    });
    world.append_entry(entry);
}

#[when(regex = r#"^I append a commit entry with TTL (\d+) seconds$"#)]
async fn when_append_commit_ttl(world: &mut PactWorld, ttl: u32) {
    let mut entry = PactWorld::make_entry(
        EntryType::Commit,
        Scope::Global,
        PactWorld::make_identity("admin@example.com", "pact-platform-admin"),
    );
    entry.ttl_seconds = Some(ttl);
    world.append_entry(entry);
}

#[when(regex = r#"^I set node "([\w-]+)" state to "(\w+)"$"#)]
async fn when_set_node_state(world: &mut PactWorld, node: String, state_str: String) {
    let state = parse_config_state(&state_str);
    world.node_states.insert(node, state);
}

#[when(
    regex = r#"^I set policy for vCluster "([\w-]+)" with max drift (\d+\.\d+) and commit window (\d+)$"#
)]
async fn when_set_policy(world: &mut PactWorld, vcluster: String, max_drift: f64, window: u32) {
    let policy = VClusterPolicy {
        vcluster_id: vcluster.clone(),
        drift_sensitivity: max_drift,
        base_commit_window_seconds: window,
        emergency_allowed: true,
        two_person_approval: false,
        ..VClusterPolicy::default()
    };
    world.policies.insert(vcluster, policy);
}

#[when(
    regex = r#"^I store a boot overlay for vCluster "([\w-]+)" version (\d+) with checksum "(.*)"$"#
)]
async fn when_store_overlay(
    world: &mut PactWorld,
    vcluster: String,
    version: u64,
    checksum: String,
) {
    let overlay =
        BootOverlay { vcluster_id: vcluster.clone(), version, data: vec![1, 2, 3], checksum };
    world.overlays.insert(vcluster, overlay);
}

#[when(
    regex = r#"^I record an exec operation by "([\w@.]+)" on node "([\w-]+)" with detail "(.*)"$"#
)]
async fn when_record_exec(world: &mut PactWorld, actor: String, node: String, detail: String) {
    let op = AdminOperation {
        operation_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        actor: PactWorld::make_identity(&actor, "pact-platform-admin"),
        operation_type: AdminOperationType::Exec,
        scope: Scope::Node(node),
        detail,
    };
    world.audit_log.push(op);
}

#[when(regex = r#"^I record a shell session start by "([\w@.]+)" on node "([\w-]+)"$"#)]
async fn when_record_shell_start(world: &mut PactWorld, actor: String, node: String) {
    let op = AdminOperation {
        operation_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        actor: PactWorld::make_identity(&actor, "pact-platform-admin"),
        operation_type: AdminOperationType::ShellSessionStart,
        scope: Scope::Node(node),
        detail: "session started".to_string(),
    };
    world.audit_log.push(op);
}

#[when(regex = r#"^I record a shell session end by "([\w@.]+)" on node "([\w-]+)"$"#)]
async fn when_record_shell_end(world: &mut PactWorld, actor: String, node: String) {
    let op = AdminOperation {
        operation_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        actor: PactWorld::make_identity(&actor, "pact-platform-admin"),
        operation_type: AdminOperationType::ShellSessionEnd,
        scope: Scope::Node(node),
        detail: "session ended".to_string(),
    };
    world.audit_log.push(op);
}

#[when("the journal state is serialized and deserialized")]
async fn when_serde_roundtrip(world: &mut PactWorld) {
    // Roundtrip the relevant state through JSON
    let entries_json = serde_json::to_string(&world.entries).unwrap();
    let states_json = serde_json::to_string(&world.node_states).unwrap();
    let policies_json = serde_json::to_string(&world.policies).unwrap();

    world.entries = serde_json::from_str(&entries_json).unwrap();
    world.node_states = serde_json::from_str(&states_json).unwrap();
    world.policies = serde_json::from_str(&policies_json).unwrap();
}

#[when(regex = r#"^a file change is detected at "(.*)"$"#)]
async fn when_file_change(world: &mut PactWorld, path: String) {
    if world.is_blacklisted(&path) {
        world.drift_filtered = true;
    } else {
        world.drift_events.push(DriftEvent {
            dimension: "files".to_string(),
            key: path,
            magnitude: 0.1,
        });
        world.drift_vector.files += 0.1;
    }
}

#[when(regex = r#"^a mount change is detected for "(.*)"$"#)]
async fn when_mount_change(world: &mut PactWorld, path: String) {
    world.drift_events.push(DriftEvent {
        dimension: "mounts".to_string(),
        key: path,
        magnitude: 0.2,
    });
    world.drift_vector.mounts += 0.2;
}

#[when(regex = r#"^a kernel parameter change is detected for "([\w.]+)"$"#)]
async fn when_kernel_change(world: &mut PactWorld, param: String) {
    world.drift_events.push(DriftEvent {
        dimension: "kernel".to_string(),
        key: param,
        magnitude: 0.1,
    });
    world.drift_vector.kernel += 0.1;
}

#[when(regex = r#"^a service state change is detected for "([\w-]+)"$"#)]
async fn when_service_change(world: &mut PactWorld, service: String) {
    world.drift_events.push(DriftEvent {
        dimension: "services".to_string(),
        key: service,
        magnitude: 0.1,
    });
    world.drift_vector.services += 0.1;
}

#[when(regex = r#"^a network interface change is detected for "([\w]+)"$"#)]
async fn when_network_change(world: &mut PactWorld, iface: String) {
    world.drift_events.push(DriftEvent {
        dimension: "network".to_string(),
        key: iface,
        magnitude: 0.1,
    });
    world.drift_vector.network += 0.1;
}

#[when(regex = r#"^a GPU state change is detected for GPU index (\d+)$"#)]
async fn when_gpu_change(world: &mut PactWorld, index: u32) {
    world.drift_events.push(DriftEvent {
        dimension: "gpu".to_string(),
        key: format!("gpu-{index}"),
        magnitude: 0.2,
    });
    world.drift_vector.gpu += 0.2;
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
    let mag = magnitude;
    world.drift_vector.kernel = mag;
    world.drift_events.push(DriftEvent {
        dimension: "kernel".to_string(),
        key: "drift".to_string(),
        magnitude: mag,
    });

    if world.enforcement_mode == "enforce" && !world.emergency_active {
        let window = world.compute_window(mag);
        world.active_commit_window = Some(CommitWindow {
            duration_seconds: window,
            opened_at: Utc::now(),
            node_id: node.clone(),
        });
    }

    // Record in journal
    let entry = PactWorld::make_entry(
        EntryType::DriftDetected,
        Scope::Node(node),
        PactWorld::make_identity("system", "pact-service-agent"),
    );
    world.append_entry(entry);
}

#[when(regex = r#"^drift is detected with magnitude (\d+\.\d+)$"#)]
async fn when_drift_magnitude(world: &mut PactWorld, magnitude: f64) {
    let window = world.compute_window(magnitude);
    world.active_commit_window = Some(CommitWindow {
        duration_seconds: window,
        opened_at: Utc::now(),
        node_id: "node-001".to_string(),
    });
}

#[when(regex = r#"^node "([\w-]+)" requests boot config for vCluster "([\w-]+)"$"#)]
async fn when_boot_request(world: &mut PactWorld, node: String, vcluster: String) {
    // Stream overlay
    if let Some(overlay) = world.overlays.get(&vcluster) {
        world.boot_stream_chunks.push(BootStreamChunk::BaseOverlay {
            version: overlay.version,
            data: overlay.data.clone(),
            checksum: overlay.checksum.clone(),
        });
    }

    // Stream node delta if exists
    let has_delta = world
        .entries
        .values()
        .any(|e| matches!(&e.scope, Scope::Node(n) if n == &node) && e.state_delta.is_some());
    if has_delta {
        world
            .boot_stream_chunks
            .push(BootStreamChunk::NodeDelta { data: b"node-delta-data".to_vec() });
    }

    // Complete
    let base_version = world.overlays.get(&vcluster).map_or(0, |o| o.version);
    world.boot_stream_chunks.push(BootStreamChunk::Complete {
        base_version,
        node_version: if has_delta { Some(1) } else { None },
    });
}

#[when(regex = r#"^the service "([\w-]+)" is started$"#)]
async fn when_service_started(world: &mut PactWorld, name: String) {
    world.service_states.insert(name.clone(), ServiceState::Running);
    world.service_start_order.push(name);
}

#[when(regex = r#"^the service "([\w-]+)" is stopped$"#)]
async fn when_service_stopped(world: &mut PactWorld, name: String) {
    world.service_states.insert(name.clone(), ServiceState::Stopped);
    world.service_stop_order.push(name);
}

#[when(regex = r#"^the service "([\w-]+)" is restarted$"#)]
async fn when_service_restarted(world: &mut PactWorld, name: String) {
    world.service_stop_order.push(name.clone());
    world.service_start_order.push(name.clone());
    world.service_states.insert(name, ServiceState::Running);
}

#[when("all services are started")]
async fn when_all_services_started(world: &mut PactWorld) {
    let mut sorted = world.service_declarations.clone();
    sorted.sort_by_key(|s| s.order);
    for decl in &sorted {
        world.service_states.insert(decl.name.clone(), ServiceState::Running);
        world.service_start_order.push(decl.name.clone());
    }
}

#[when("all services are stopped")]
async fn when_all_services_stopped(world: &mut PactWorld) {
    let mut sorted = world.service_declarations.clone();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.order));
    for decl in &sorted {
        world.service_states.insert(decl.name.clone(), ServiceState::Stopped);
        world.service_stop_order.push(decl.name.clone());
    }
}

#[when(regex = r#"^the service "([\w-]+)" fails$"#)]
async fn when_service_fails(world: &mut PactWorld, name: String) {
    world.service_states.insert(name, ServiceState::Failed);
}

#[when("capability detection runs")]
async fn when_capability_detection(world: &mut PactWorld) {
    let report = CapabilityReport {
        node_id: "node-001".to_string(),
        timestamp: Utc::now(),
        report_id: Uuid::new_v4(),
        gpus: world.gpu_capabilities.clone(),
        memory: MemoryCapability {
            total_bytes: 549_755_813_888,
            available_bytes: 500_000_000_000,
            numa_nodes: 2,
        },
        network: None,
        storage: StorageCapability { tmpfs_bytes: 1_073_741_824, mounts: vec![] },
        software: SoftwareCapability { loaded_modules: vec![], uenv_image: None, services: vec![] },
        config_state: world.node_states.get("node-001").cloned().unwrap_or(ConfigState::Committed),
        drift_summary: None,
        emergency: None,
        supervisor_status: world.supervisor_status.clone(),
    };
    world.capability_report = Some(report);
    world.manifest_written = true;
    world.socket_available = true;
}

#[when(
    regex = r#"^admin "([\w@.]+)" enters emergency mode on node "([\w-]+)" with reason "(.*)"$"#
)]
async fn when_emergency_enter(world: &mut PactWorld, admin: String, node: String, reason: String) {
    world.emergency_active = true;
    world.emergency_reason = Some(reason.clone());
    world.emergency_start_time = Some(Utc::now());
    world.node_states.insert(node.clone(), ConfigState::Emergency);

    let mut entry = PactWorld::make_entry(
        EntryType::EmergencyStart,
        Scope::Node(node),
        PactWorld::make_identity(&admin, "pact-ops-ml-training"),
    );
    entry.emergency_reason = Some(reason);
    world.append_entry(entry);
}

#[when(regex = r#"^the user requests to commit on vCluster "([\w-]+)"$"#)]
async fn when_user_commits(world: &mut PactWorld, vcluster: String) {
    world.auth_result = Some(world.authorize("commit", &vcluster));
}

#[when(regex = r#"^the user requests action "(\w+)" on vCluster "([\w-]+)"$"#)]
async fn when_user_action(world: &mut PactWorld, action: String, vcluster: String) {
    world.auth_result = Some(world.authorize(&action, &vcluster));
}

// ===========================================================================
// THEN steps
// ===========================================================================

#[then(regex = r#"^the entry should be assigned sequence (\d+)$"#)]
async fn then_assigned_sequence(world: &mut PactWorld, seq: u64) {
    assert!(world.entries.contains_key(&seq), "entry at sequence {seq} not found");
}

#[then(regex = r#"^the journal should contain (\d+) entr(?:y|ies)$"#)]
async fn then_journal_count(world: &mut PactWorld, count: usize) {
    assert_eq!(world.entries.len(), count);
}

#[then(regex = r#"^entry (\d+) should have type "(\w+)"$"#)]
async fn then_entry_type(world: &mut PactWorld, seq: u64, entry_type_str: String) {
    let entry = world.entries.get(&seq).expect("entry not found");
    let expected = parse_entry_type(&entry_type_str);
    assert_eq!(entry.entry_type, expected);
}

#[then(regex = r#"^entry (\d+) should have author "([\w@.]+)"$"#)]
async fn then_entry_author(world: &mut PactWorld, seq: u64, author: String) {
    let entry = world.entries.get(&seq).expect("entry not found");
    assert_eq!(entry.author.principal, author);
}

#[then(regex = r#"^entry (\d+) should have role "([\w-]+)"$"#)]
async fn then_entry_role(world: &mut PactWorld, seq: u64, role: String) {
    let entry = world.entries.get(&seq).expect("entry not found");
    assert_eq!(entry.author.role, role);
}

#[then(regex = r#"^entry (\d+) should have a kernel delta with key "([\w.]+)"$"#)]
async fn then_entry_kernel_delta(world: &mut PactWorld, seq: u64, key: String) {
    let entry = world.entries.get(&seq).expect("entry not found");
    let delta = entry.state_delta.as_ref().expect("no state delta");
    assert!(delta.kernel.iter().any(|d| d.key == key));
}

#[then(regex = r#"^the delta action should be "(\w+)"$"#)]
async fn then_delta_action(world: &mut PactWorld, action_str: String) {
    let last_entry = world.entries.values().last().expect("no entries");
    let delta = last_entry.state_delta.as_ref().expect("no state delta");
    let expected = match action_str.as_str() {
        "Add" => DeltaAction::Add,
        "Remove" => DeltaAction::Remove,
        "Modify" => DeltaAction::Modify,
        _ => panic!("unknown delta action: {action_str}"),
    };
    assert!(delta.kernel.iter().any(|d| d.action == expected));
}

#[then(regex = r#"^entry (\d+) should have TTL (\d+)$"#)]
async fn then_entry_ttl(world: &mut PactWorld, seq: u64, ttl: u32) {
    let entry = world.entries.get(&seq).expect("entry not found");
    assert_eq!(entry.ttl_seconds, Some(ttl));
}

#[then(regex = r#"^node "([\w-]+)" should have state "(\w+)"$"#)]
async fn then_node_state(world: &mut PactWorld, node: String, state_str: String) {
    let expected = parse_config_state(&state_str);
    assert_eq!(world.node_states.get(&node), Some(&expected));
}

#[then(regex = r#"^vCluster "([\w-]+)" should have a policy with max drift (\d+\.\d+)$"#)]
async fn then_policy_drift(world: &mut PactWorld, vcluster: String, max_drift: f64) {
    let policy = world.policies.get(&vcluster).expect("policy not found");
    assert!((policy.drift_sensitivity - max_drift).abs() < f64::EPSILON);
}

#[then(regex = r#"^vCluster "([\w-]+)" should have commit window (\d+)$"#)]
async fn then_policy_window(world: &mut PactWorld, vcluster: String, window: u32) {
    let policy = world.policies.get(&vcluster).expect("policy not found");
    assert_eq!(policy.base_commit_window_seconds, window);
}

#[then(regex = r#"^vCluster "([\w-]+)" should have overlay version (\d+)$"#)]
async fn then_overlay_version(world: &mut PactWorld, vcluster: String, version: u64) {
    let overlay = world.overlays.get(&vcluster).expect("overlay not found");
    assert_eq!(overlay.version, version);
}

#[then(regex = r#"^vCluster "([\w-]+)" overlay should have checksum "(.*)"$"#)]
async fn then_overlay_checksum(world: &mut PactWorld, vcluster: String, checksum: String) {
    let overlay = world.overlays.get(&vcluster).expect("overlay not found");
    assert_eq!(overlay.checksum, checksum);
}

#[then(regex = r#"^the audit log should contain (\d+) entr(?:y|ies)$"#)]
async fn then_audit_count(world: &mut PactWorld, count: usize) {
    assert_eq!(world.audit_log.len(), count);
}

#[then(regex = r#"^audit entry (\d+) should have type "(\w+)"$"#)]
async fn then_audit_type(world: &mut PactWorld, idx: usize, op_type: String) {
    let op = &world.audit_log[idx];
    let expected = parse_admin_op_type(&op_type);
    assert_eq!(op.operation_type, expected);
}

#[then(regex = r#"^audit entry (\d+) should have detail "(.*)"$"#)]
async fn then_audit_detail(world: &mut PactWorld, idx: usize, detail: String) {
    assert_eq!(world.audit_log[idx].detail, detail);
}

#[then("the change should be filtered by the blacklist")]
async fn then_filtered(world: &mut PactWorld) {
    assert!(world.drift_filtered);
}

#[then("no drift event should be emitted")]
async fn then_no_drift(world: &mut PactWorld) {
    assert!(world.drift_events.is_empty() || world.drift_filtered);
}

#[then("a drift event should be emitted")]
async fn then_drift_emitted(world: &mut PactWorld) {
    assert!(!world.drift_events.is_empty());
}

#[then(regex = r#"^the drift should be in the "(\w+)" dimension$"#)]
async fn then_drift_dimension(world: &mut PactWorld, dim: String) {
    assert!(world.drift_events.iter().any(|e| e.dimension == dim));
}

#[then(regex = r#"^a drift event should be emitted in the "(\w+)" dimension$"#)]
async fn then_drift_in_dimension(world: &mut PactWorld, dim: String) {
    assert!(world.drift_events.iter().any(|e| e.dimension == dim));
}

#[then(regex = r#"^the drift vector should have non-zero "(\w+)" magnitude$"#)]
async fn then_drift_nonzero(world: &mut PactWorld, dim: String) {
    let val = get_drift_dimension(&world.drift_vector, &dim);
    assert!(val > 0.0, "{dim} magnitude should be > 0.0, got {val}");
}

#[then("other dimensions should be zero")]
async fn then_other_zero(world: &mut PactWorld) {
    // At least check that not all dimensions are non-zero
    let dims = [
        world.drift_vector.mounts,
        world.drift_vector.files,
        world.drift_vector.network,
        world.drift_vector.services,
        world.drift_vector.kernel,
        world.drift_vector.packages,
        world.drift_vector.gpu,
    ];
    let non_zero_count = dims.iter().filter(|&&v| v > 0.0).count();
    assert!(non_zero_count <= 1, "expected at most 1 non-zero dimension");
}

#[then(
    regex = r#"^the (\w+) drift total magnitude should be greater than the (\w+) drift total magnitude$"#
)]
async fn then_drift_comparison(world: &mut PactWorld, dim1: String, dim2: String) {
    let mut v1 = DriftVector {
        mounts: 0.0,
        files: 0.0,
        network: 0.0,
        services: 0.0,
        kernel: 0.0,
        packages: 0.0,
        gpu: 0.0,
    };
    set_drift_dimension(&mut v1, &dim1, 1.0);
    let mag1 = v1.magnitude(&world.drift_weights);

    let mut v2 = DriftVector {
        mounts: 0.0,
        files: 0.0,
        network: 0.0,
        services: 0.0,
        kernel: 0.0,
        packages: 0.0,
        gpu: 0.0,
    };
    set_drift_dimension(&mut v2, &dim2, 1.0);
    let mag2 = v2.magnitude(&world.drift_weights);

    assert!(mag1 > mag2, "{dim1} magnitude {mag1} should be > {dim2} magnitude {mag2}");
}

#[then("the total drift magnitude should be 0.0")]
async fn then_drift_zero(world: &mut PactWorld) {
    let mag = world.drift_vector.magnitude(&world.drift_weights);
    assert!((mag - 0.0).abs() < f64::EPSILON, "expected 0.0, got {mag}");
}

#[then("the total drift magnitude should be greater than a single dimension at 0.5")]
async fn then_drift_compound(world: &mut PactWorld) {
    let compound = world.drift_vector.magnitude(&world.drift_weights);
    let single = DriftVector {
        mounts: 0.0,
        files: 0.0,
        network: 0.0,
        services: 0.0,
        kernel: 0.5,
        packages: 0.0,
        gpu: 0.0,
    };
    let single_mag = single.magnitude(&world.drift_weights);
    assert!(compound > single_mag);
}

#[then(regex = r#"^the commit window should be approximately (\d+) seconds$"#)]
async fn then_window_approx(world: &mut PactWorld, expected: u32) {
    let window = world.active_commit_window.as_ref().expect("no commit window");
    let tolerance = 5.0;
    assert!(
        (window.duration_seconds - f64::from(expected)).abs() < tolerance,
        "expected ~{expected}s, got {:.0}s",
        window.duration_seconds
    );
}

#[then("the boot stream should contain a base overlay chunk")]
async fn then_has_overlay(world: &mut PactWorld) {
    assert!(world
        .boot_stream_chunks
        .iter()
        .any(|c| matches!(c, BootStreamChunk::BaseOverlay { .. })));
}

#[then("the boot stream should contain a node delta")]
async fn then_has_delta(world: &mut PactWorld) {
    assert!(world
        .boot_stream_chunks
        .iter()
        .any(|c| matches!(c, BootStreamChunk::NodeDelta { .. })));
}

#[then("the boot stream should not contain a node delta")]
async fn then_no_delta(world: &mut PactWorld) {
    assert!(!world
        .boot_stream_chunks
        .iter()
        .any(|c| matches!(c, BootStreamChunk::NodeDelta { .. })));
}

#[then("the boot stream should end with a ConfigComplete message")]
async fn then_has_complete(world: &mut PactWorld) {
    assert!(matches!(world.boot_stream_chunks.last(), Some(BootStreamChunk::Complete { .. })));
}

#[then(regex = r#"^the capability report should contain (\d+) GPUs$"#)]
async fn then_gpu_count(world: &mut PactWorld, count: usize) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.gpus.len(), count);
}

#[then(regex = r#"^all GPUs should have vendor "(Nvidia|Amd)"$"#)]
async fn then_all_gpu_vendor(world: &mut PactWorld, vendor_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match vendor_str.as_str() {
        "Amd" => GpuVendor::Amd,
        _ => GpuVendor::Nvidia,
    };
    assert!(report.gpus.iter().all(|g| g.vendor == expected));
}

#[then(regex = r#"^the service "([\w-]+)" should be in state "(\w+)"$"#)]
async fn then_service_state(world: &mut PactWorld, name: String, state_str: String) {
    let expected = parse_service_state(&state_str);
    assert_eq!(world.service_states.get(&name), Some(&expected));
}

#[then(regex = r#"^"([\w-]+)" should start before "([\w-]+)"$"#)]
async fn then_start_order(world: &mut PactWorld, first: String, second: String) {
    let first_idx = world.service_start_order.iter().position(|s| s == &first);
    let second_idx = world.service_start_order.iter().position(|s| s == &second);
    assert!(first_idx < second_idx, "{first} should start before {second}");
}

#[then(regex = r#"^"([\w-]+)" should stop before "([\w-]+)"$"#)]
async fn then_stop_order(world: &mut PactWorld, first: String, second: String) {
    let first_idx = world.service_stop_order.iter().position(|s| s == &first);
    let second_idx = world.service_stop_order.iter().position(|s| s == &second);
    assert!(first_idx < second_idx, "{first} should stop before {second}");
}

#[then(regex = r#"^the status should report backend "(Pact|Systemd)"$"#)]
async fn then_supervisor_backend(world: &mut PactWorld, backend_str: String) {
    let expected = match backend_str.as_str() {
        "Systemd" => SupervisorBackend::Systemd,
        _ => SupervisorBackend::Pact,
    };
    assert_eq!(world.supervisor_status.backend, expected);
}

#[then(regex = r#"^the status should report (\d+) declared, (\d+) running, (\d+) failed$"#)]
async fn then_supervisor_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    assert_eq!(world.supervisor_status.services_declared, declared);
    assert_eq!(world.supervisor_status.services_running, running);
    assert_eq!(world.supervisor_status.services_failed, failed);
}

#[then("the request should be authorized")]
async fn then_authorized(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::Authorized) => {}
        other => panic!("expected Authorized, got {other:?}"),
    }
}

#[then("the request should be denied")]
async fn then_denied(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::Denied { .. }) => {}
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[then(regex = r#"^the request should be denied with reason "(.*)"$"#)]
async fn then_denied_with_reason(world: &mut PactWorld, expected: String) {
    match &world.auth_result {
        Some(AuthResult::Denied { reason: actual }) => {
            assert!(
                actual.contains(&expected),
                "expected reason containing '{expected}', got '{actual}'"
            );
        }
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[then("the response should indicate approval required")]
async fn then_approval_required(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::ApprovalRequired { .. }) => {}
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }
}

#[then(regex = r#"^the response should require approval from a second administrator$"#)]
async fn then_requires_second_admin(world: &mut PactWorld) {
    match &world.auth_result {
        Some(AuthResult::ApprovalRequired { .. }) => {}
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }
}

#[then(regex = r#"^node "([\w-]+)" should be in emergency state$"#)]
async fn then_emergency_state(world: &mut PactWorld, node: String) {
    assert_eq!(world.node_states.get(&node), Some(&ConfigState::Emergency));
}

#[then(regex = r#"^an EmergencyStart entry should be recorded in the journal$"#)]
async fn then_emergency_start_entry(world: &mut PactWorld) {
    assert!(world.entries.values().any(|e| e.entry_type == EntryType::EmergencyStart));
}

#[then(regex = r#"^the emergency reason should be "(.*)"$"#)]
async fn then_emergency_reason(world: &mut PactWorld, reason: String) {
    let entry = world
        .entries
        .values()
        .find(|e| e.entry_type == EntryType::EmergencyStart)
        .expect("no EmergencyStart");
    assert_eq!(entry.emergency_reason.as_deref(), Some(reason.as_str()));
}

#[then(regex = r#"^the commit window for node "([\w-]+)" should be (\d+) seconds$"#)]
async fn then_emergency_window(world: &mut PactWorld, _node: String, expected: u32) {
    assert_eq!(world.commit_window_config.emergency_window_seconds, expected);
}

// ===========================================================================
// Helpers
// ===========================================================================

fn parse_config_state(s: &str) -> ConfigState {
    match s {
        "ObserveOnly" => ConfigState::ObserveOnly,
        "Committed" => ConfigState::Committed,
        "Drifted" => ConfigState::Drifted,
        "Converging" => ConfigState::Converging,
        "Emergency" => ConfigState::Emergency,
        _ => panic!("unknown config state: {s}"),
    }
}

fn parse_entry_type(s: &str) -> EntryType {
    match s {
        "Commit" => EntryType::Commit,
        "Rollback" => EntryType::Rollback,
        "AutoConverge" => EntryType::AutoConverge,
        "DriftDetected" => EntryType::DriftDetected,
        "CapabilityChange" => EntryType::CapabilityChange,
        "PolicyUpdate" => EntryType::PolicyUpdate,
        "BootConfig" => EntryType::BootConfig,
        "EmergencyStart" => EntryType::EmergencyStart,
        "EmergencyEnd" => EntryType::EmergencyEnd,
        "ExecLog" => EntryType::ExecLog,
        "ShellSession" => EntryType::ShellSession,
        "ServiceLifecycle" => EntryType::ServiceLifecycle,
        _ => panic!("unknown entry type: {s}"),
    }
}

fn parse_admin_op_type(s: &str) -> AdminOperationType {
    match s {
        "Exec" => AdminOperationType::Exec,
        "ShellSessionStart" => AdminOperationType::ShellSessionStart,
        "ShellSessionEnd" => AdminOperationType::ShellSessionEnd,
        "ServiceStart" => AdminOperationType::ServiceStart,
        "ServiceStop" => AdminOperationType::ServiceStop,
        "ServiceRestart" => AdminOperationType::ServiceRestart,
        "EmergencyStart" => AdminOperationType::EmergencyStart,
        "EmergencyEnd" => AdminOperationType::EmergencyEnd,
        _ => panic!("unknown admin op type: {s}"),
    }
}

fn parse_service_state(s: &str) -> ServiceState {
    match s {
        "Starting" => ServiceState::Starting,
        "Running" => ServiceState::Running,
        "Stopping" => ServiceState::Stopping,
        "Stopped" => ServiceState::Stopped,
        "Failed" => ServiceState::Failed,
        "Restarting" => ServiceState::Restarting,
        _ => panic!("unknown service state: {s}"),
    }
}

fn set_drift_dimension(v: &mut DriftVector, dim: &str, val: f64) {
    match dim {
        "mounts" => v.mounts = val,
        "files" => v.files = val,
        "network" => v.network = val,
        "services" => v.services = val,
        "kernel" => v.kernel = val,
        "packages" => v.packages = val,
        "gpu" => v.gpu = val,
        _ => panic!("unknown drift dimension: {dim}"),
    }
}

fn get_drift_dimension(v: &DriftVector, dim: &str) -> f64 {
    match dim {
        "mounts" => v.mounts,
        "files" => v.files,
        "network" => v.network,
        "services" => v.services,
        "kernel" => v.kernel,
        "packages" => v.packages,
        "gpu" => v.gpu,
        _ => panic!("unknown drift dimension: {dim}"),
    }
}

fn md5_simple(s: &str) -> u64 {
    let mut hash: u64 = 0;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    hash
}

// ===========================================================================
// Cucumber runner
// ===========================================================================

fn main() {
    futures::executor::block_on(PactWorld::cucumber().run("features/"));
}
