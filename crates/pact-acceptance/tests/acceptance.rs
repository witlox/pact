//! BDD acceptance tests for pact.
//!
//! Uses cucumber-rs to run Gherkin feature files.
//! Custom harness: `[[test]] harness = false` in Cargo.toml.
//!
//! Run with: `cargo test -p pact-acceptance`

// Cucumber step definitions often have parameters extracted by macros that aren't
// always used in stub implementations. Accept that in this test crate.
#![allow(
    unused_variables,
    unused_imports,
    dead_code,
    unused_mut,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::uninlined_format_args
)]

use std::collections::HashMap;
use std::fmt;

use cucumber::World;
use pact_agent::commit::CommitWindowManager;
use pact_agent::drift::DriftEvaluator;
use pact_agent::emergency::EmergencyManager;
use pact_common::{
    config::{BlacklistConfig, CommitWindowConfig},
    error::PactError,
    types::{
        CapabilityReport, DriftVector, DriftWeights, EntrySeq, GpuCapability, Identity, NodeId,
        ServiceDecl, ServiceState, SupervisorBackend, SupervisorStatus, VClusterId,
    },
};
use pact_journal::JournalState;
use pact_policy::rules::DefaultPolicyEngine;

mod steps;

// ---------------------------------------------------------------------------
// World — shared state across all steps in a scenario
// ---------------------------------------------------------------------------

#[derive(World)]
#[world(init = Self::new)]
pub struct PactWorld {
    // === Wired to real crate code ===
    /// Journal state machine — real `JournalState::apply_command()`.
    pub journal: JournalState,

    /// Drift evaluator — real blacklist filtering + event processing.
    pub drift_evaluator: DriftEvaluator,

    /// Commit window manager — real window calculation + lifecycle.
    pub commit_mgr: CommitWindowManager,

    /// Emergency mode manager — real start/end lifecycle.
    pub emergency_mgr: EmergencyManager,

    /// Policy engine — real RBAC + two-person approval workflow.
    pub policy_engine: DefaultPolicyEngine,

    /// Current authenticated identity (replaces old UserContext).
    pub current_identity: Option<Identity>,

    /// Last RBAC/policy evaluation result.
    pub auth_result: Option<AuthResult>,

    // === Drift test helpers ===
    /// Override drift vector for GIVEN steps that set specific magnitudes.
    pub drift_vector_override: DriftVector,

    /// Drift weights (kept separately for GIVEN/THEN magnitude comparisons).
    pub drift_weights: DriftWeights,

    /// Blacklist config (kept for rebuilding evaluator).
    pub blacklist_config: BlacklistConfig,

    /// Whether the last drift event was filtered by the blacklist.
    pub drift_filtered: bool,

    /// Enforcement mode: "observe" or "enforce".
    pub enforcement_mode: String,

    // === In-memory stubs (platform/infra dependent) ===

    // --- Supervisor ---
    pub supervisor_backend: SupervisorBackend,
    pub service_declarations: Vec<ServiceDecl>,
    pub service_states: HashMap<String, ServiceState>,
    pub service_start_order: Vec<String>,
    pub service_stop_order: Vec<String>,
    pub supervisor_status: SupervisorStatus,

    // --- Shell / Exec ---
    pub shell_session_active: bool,
    pub shell_session_id: Option<String>,
    pub shell_whitelist: Vec<String>,
    pub shell_whitelist_mode: String,
    pub exec_results: Vec<ExecResult>,
    pub whitelist_suggestions: Vec<String>,
    pub available_commands: Vec<String>,
    pub blocked_commands: Vec<String>,
    pub lesssecure_set: bool,

    // --- Capability ---
    pub capability_report: Option<CapabilityReport>,
    pub gpu_capabilities: Vec<GpuCapability>,
    pub cpu_capability: Option<pact_common::types::CpuCapability>,
    pub memory_capability: Option<pact_common::types::MemoryCapability>,
    pub network_interfaces: Option<Vec<pact_common::types::NetworkInterface>>,
    pub storage_capability: Option<pact_common::types::StorageCapability>,
    pub software_capability: Option<pact_common::types::SoftwareCapability>,
    pub manifest_written: bool,
    pub socket_available: bool,

    // --- Config subscription ---
    pub subscriptions: HashMap<NodeId, ConfigSubscription>,
    pub received_updates: Vec<ConfigUpdateEvent>,

    // --- Boot sequence ---
    pub boot_phases_completed: Vec<String>,
    pub boot_stream_chunks: Vec<BootStreamChunk>,

    // --- CLI ---
    pub cli_output: Option<String>,
    pub cli_exit_code: Option<i32>,

    // --- Federation ---
    pub sovra_reachable: bool,
    pub federated_templates: Vec<String>,
    pub compliance_reports: Vec<String>,

    // --- Observability ---
    pub loki_enabled: bool,
    pub loki_events: Vec<LokiEvent>,
    pub metrics_available: bool,
    pub health_status: Option<HealthResponse>,

    // --- Errors ---
    pub last_error: Option<PactError>,
    pub last_denial_reason: Option<String>,

    // --- MCP / Agentic ---
    pub mcp_active: bool,

    // --- Journal cluster (partition simulation) ---
    pub journal_reachable: bool,
    pub journal_leader: Option<u64>,
    pub journal_cluster_size: u32,

    // --- Policy flags ---
    pub opa_available: bool,
    pub policy_degraded: bool,

    // --- Rollback/alert flags ---
    pub rollback_triggered: bool,
    pub rollback_deferred: bool,
    pub alert_raised: bool,
    pub active_consumer_count: usize,

    // --- Merge conflict ---
    pub conflict_mgr: pact_agent::conflict::ConflictManager,
    pub conflict_local_value: Option<String>,
    pub conflict_journal_value: Option<String>,

    // --- Resource isolation ---
    pub cgroup_manager: Option<Box<dyn hpc_node::CgroupManager>>,
    /// Tracks cgroup scopes created (scope_path -> service_name).
    pub cgroup_scopes: HashMap<String, String>,
    /// Tracks which services have simulated cgroup creation failure.
    pub cgroup_fail_services: Vec<String>,
    /// Tracks audit events emitted during resource isolation tests.
    pub audit_events: Vec<AuditEventRecord>,
    /// Whether an emergency session is active (for resource isolation tests).
    pub emergency_session_active: bool,
    /// Identity for the emergency session.
    pub emergency_session_identity: Option<String>,
    /// Tracks frozen slices.
    pub frozen_slices: Vec<String>,
    /// Tracks namespace sets per allocation.
    pub namespace_sets: HashMap<String, Vec<String>>,
    /// Tracks whether a systemd scope was created (for systemd backend tests).
    pub systemd_scope_created: Option<String>,
    /// Tracks whether direct cgroup entries were created (systemd backend).
    pub direct_cgroup_entries_created: bool,
    /// Simulated running processes per scope.
    pub scope_processes: HashMap<String, u32>,
    /// Tracks killed scopes (for cleanup verification).
    pub killed_scopes: Vec<String>,
    /// Operation denied flag.
    pub operation_denied: bool,
    /// Metric read result.
    pub metric_read_value: Option<u64>,

    // --- Workload integration (extended) ---
    /// Handoff server for namespace creation/handoff.
    pub handoff_server: Option<pact_agent::handoff::HandoffServer>,
    /// Whether the handoff socket is available.
    pub handoff_socket_available: bool,
    /// Whether lattice is in standalone mode.
    pub lattice_standalone: bool,
    /// Whether readiness signal has been emitted.
    pub readiness_signal_emitted: bool,
    /// Queued requests (before readiness).
    pub queued_requests: Vec<String>,
    /// Tracks active allocations (allocation_id -> uenv image).
    pub active_allocations: HashMap<String, Option<String>>,
    /// Whether pact-agent has crashed and restarted.
    pub agent_restarted: bool,
    /// Allocations that ended while agent was down.
    pub ended_allocations_during_crash: Vec<String>,

    // --- Identity mapping ---
    pub uid_map: Option<pact_common::types::UidMap>,
    pub identity_mode: pact_common::types::IdentityMode,
    pub last_auth_subject: Option<String>,
    pub last_assigned_uid: Option<u32>,
    pub nfs_configured: bool,
    pub passwd_db_created: bool,
    pub group_db_created: bool,
    pub nsswitch_configured: bool,
    pub uid_map_loaded: bool,
    pub journal_committed: bool,
    pub db_files_updated: bool,
    pub nss_lookup_result: Option<u32>,
    pub nss_lookup_local: bool,
    pub nss_no_network: bool,
    pub nss_not_found: bool,
    pub nss_fallthrough: bool,
    pub service_waiting_for_uid_map: bool,
    pub service_started_after_resolve: bool,

    // --- Workload integration ---
    pub mount_manager: Option<pact_agent::handoff::MountRefManager>,

    // --- Network management ---
    /// Declared network interface configurations from overlay.
    pub network_configs: Vec<pact_agent::network::InterfaceConfig>,
    /// Configured interface states after network phase.
    pub network_interface_states: Vec<pact_agent::network::InterfaceState>,
    /// Whether network configuration should fail.
    pub network_config_will_fail: bool,
    /// Whether network has been configured.
    pub network_configured: bool,
    /// Default route configured.
    pub network_default_route: Option<String>,
    /// Whether pact-agent configured network interfaces (for systemd mode check).
    pub network_configured_by_pact: bool,

    // --- Boot phase state (platform_bootstrap) ---
    /// Ordered list of boot phases that should execute.
    pub boot_phase_order: Vec<String>,
    /// Boot phase that will fail (if any).
    pub boot_phase_fail: Option<String>,
    /// Current boot state: "Booting", "Ready", "BootFailed".
    pub boot_state: String,
    /// Which boot phase failed (if any).
    pub boot_failed_at: Option<String>,
    /// Whether boot was retried after failure resolution.
    pub boot_retried: bool,
    /// Whether failure condition has been resolved.
    pub boot_failure_resolved: bool,
    /// Whether running as PID 1.
    pub running_as_pid1: bool,
    /// Boot start timestamp (for timing).
    pub boot_start_time: Option<std::time::Instant>,
    /// Boot end timestamp.
    pub boot_end_time: Option<std::time::Instant>,
    /// Whether a warm journal (cached overlay) is available.
    pub warm_journal: bool,

    // --- Watchdog state ---
    /// Whether /dev/watchdog is available.
    pub watchdog_available: bool,
    /// Whether a watchdog handle has been opened.
    pub watchdog_handle_opened: bool,
    /// Whether the watchdog is being petted periodically.
    pub watchdog_petted: bool,
    /// Watchdog timeout in seconds.
    pub watchdog_timeout_seconds: Option<u32>,
    /// Whether the supervision loop is hung.
    pub supervision_loop_hung: bool,
    /// Whether the watchdog timer has expired.
    pub watchdog_timer_expired: bool,
    /// Whether BMC triggered a reboot.
    pub bmc_reboot_triggered: bool,

    // --- Adaptive supervision loop ---
    /// Whether there are active allocations on the node.
    pub has_active_allocations: bool,
    /// Current poll interval in ms (adaptive).
    pub supervision_poll_interval_ms: u64,
    /// Whether deep inspections are performed.
    pub deep_inspections: bool,
    /// Simulated CPU usage percent.
    pub cpu_usage_percent: f64,

    // --- Bootstrap identity / SPIRE ---
    /// Whether a bootstrap identity from OpenCHAMI is available.
    pub bootstrap_identity_available: bool,
    /// Whether pact-agent authenticated with bootstrap identity.
    pub authenticated_with_bootstrap: bool,
    /// Whether SPIRE agent is reachable.
    pub spire_agent_reachable: bool,
    /// Whether SPIRE agent has become reachable (after initially unreachable).
    pub spire_agent_became_reachable: bool,
    /// Whether an SVID was obtained from SPIRE.
    pub svid_obtained: bool,
    /// Whether pact-agent rotated to SPIRE-managed mTLS.
    pub spire_mtls_active: bool,
    /// Whether bootstrap identity was discarded.
    pub bootstrap_identity_discarded: bool,
    /// Whether SPIRE SVID acquisition is being retried.
    pub spire_retry_active: bool,
    /// Whether no SPIRE agent is available at all.
    pub no_spire_agent: bool,

    // --- Device coldplug ---
    /// Whether device nodes were set up from sysfs.
    pub device_nodes_setup: bool,
    /// Whether kernel modules were loaded.
    pub kernel_modules_loaded: bool,
    /// Whether device permissions were set.
    pub device_permissions_set: bool,
    /// Whether a persistent hotplug daemon is running.
    pub hotplug_daemon_running: bool,

    // --- Node assignment ---
    /// Node ID to vCluster assignment.
    pub node_vcluster_assignment: Option<(String, String)>,

    // --- Diag fleet ---
    pub diag_fleet_nodes: Vec<String>,
    pub diag_unreachable_nodes: Vec<String>,

    // --- Auth (hpc-auth) ---
    /// Auth server URL for test scenarios.
    pub auth_server_url: Option<String>,
    /// Temporary directory for token cache in auth tests.
    pub auth_cache_dir: Option<tempfile::TempDir>,
    /// Whether a browser is available (simulated).
    pub auth_browser_available: bool,
    /// Whether the IdP is reachable (simulated).
    pub auth_idp_reachable: bool,
    /// Whether the server is reachable (simulated).
    pub auth_server_reachable: bool,
    /// Manual IdP configuration URL (if any).
    pub auth_manual_idp_url: Option<String>,
    /// Whether manual IdP override is enabled.
    pub auth_manual_idp_override: bool,
    /// Simulated IdP capabilities.
    pub auth_idp_supports_pkce: bool,
    pub auth_idp_supports_device_code: bool,
    pub auth_idp_supports_confidential: bool,
    /// The flow that was selected by the auth system.
    pub auth_selected_flow: Option<String>,
    /// Whether login was attempted.
    pub auth_login_attempted: bool,
    /// Whether login succeeded.
    pub auth_login_succeeded: bool,
    /// Auth error message (if any).
    pub auth_error: Option<String>,
    /// Whether token was stored in cache.
    pub auth_token_stored: bool,
    /// Whether cache was modified.
    pub auth_cache_modified: bool,
    /// Whether a new auth flow was initiated.
    pub auth_flow_initiated: bool,
    /// Permission mode for cache (strict or lenient).
    pub auth_permission_mode: String,
    /// Simulated cache file permissions (octal, e.g. 0o600).
    pub auth_cache_permissions: u32,
    /// Whether a permissions warning was logged.
    pub auth_permissions_warning: bool,
    /// Whether permissions were fixed automatically.
    pub auth_permissions_fixed: bool,
    /// Cached discovery document available.
    pub auth_cached_discovery: bool,
    /// Whether cached discovery document is stale.
    pub auth_cached_discovery_stale: bool,
    /// Default server URL.
    pub auth_default_server: Option<String>,
    /// Multi-server token cache: server URL -> token validity.
    pub auth_server_tokens: std::collections::HashMap<String, AuthTokenState>,
    /// Client credentials provided for service-account flow.
    pub auth_client_credentials_valid: bool,
    /// Whether token revocation was attempted.
    pub auth_revocation_attempted: bool,
    /// Last CLI command simulated (for auth integration tests).
    pub auth_last_cli_command: Option<String>,
    /// Whether the auth refresh was silent.
    pub auth_silent_refresh: bool,
    /// Scopes returned after refresh (if different).
    pub auth_refresh_scopes: Option<Vec<String>>,
}

// Manual Debug impl — real types don't all derive Debug uniformly.
impl fmt::Debug for PactWorld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PactWorld")
            .field("journal_entries", &self.journal.entries.len())
            .field("drift_filtered", &self.drift_filtered)
            .field("enforcement_mode", &self.enforcement_mode)
            .field("current_identity", &self.current_identity)
            .field("auth_result", &self.auth_result)
            .field("mcp_active", &self.mcp_active)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Supporting types (test-only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AuditEventRecord {
    pub action: String,
    pub detail: String,
    pub identity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub logged: bool,
}

#[derive(Debug, Clone)]
pub enum AuthResult {
    Authorized,
    Denied { reason: String },
    ApprovalRequired { approval_id: String },
}

#[derive(Debug, Clone)]
pub struct ConfigSubscription {
    pub vcluster_id: VClusterId,
    pub from_sequence: EntrySeq,
}

#[derive(Debug, Clone)]
pub struct ConfigUpdateEvent {
    pub sequence: EntrySeq,
    pub update_type: String,
}

#[derive(Debug, Clone)]
pub enum BootStreamChunk {
    BaseOverlay { version: u64, data: Vec<u8>, checksum: String },
    NodeDelta { data: Vec<u8> },
    Complete { base_version: u64, node_version: Option<u64> },
}

#[derive(Debug, Clone)]
pub struct LokiEvent {
    pub component: String,
    pub entry_type: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct HealthResponse {
    pub status_code: u16,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthTokenState {
    /// Access token is valid, not expired.
    Valid,
    /// Access token expired, refresh token valid.
    AccessExpired,
    /// Both tokens expired.
    AllExpired,
    /// No refresh token, access token expired.
    NoRefresh,
    /// Cache corrupted.
    Corrupted,
}

// ---------------------------------------------------------------------------
// World initialization
// ---------------------------------------------------------------------------

impl PactWorld {
    #[allow(clippy::too_many_lines)]
    fn new() -> Self {
        let blacklist = BlacklistConfig::default();
        let weights = DriftWeights::default();
        let commit_config = CommitWindowConfig::default();

        Self {
            // Real code instances
            journal: JournalState::default(),
            drift_evaluator: DriftEvaluator::new(blacklist.clone(), weights.clone()),
            commit_mgr: CommitWindowManager::new(commit_config),
            emergency_mgr: EmergencyManager::new(14400),
            policy_engine: DefaultPolicyEngine::new(1800),
            current_identity: None,
            auth_result: None,

            // Drift helpers
            drift_vector_override: DriftVector::default(),
            drift_weights: weights,
            blacklist_config: blacklist,
            drift_filtered: false,
            enforcement_mode: "observe".to_string(),

            // Supervisor stubs
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

            // Shell stubs
            shell_session_active: false,
            shell_session_id: None,
            shell_whitelist: steps::helpers::default_whitelist(),
            shell_whitelist_mode: "learning".to_string(),
            exec_results: Vec::new(),
            whitelist_suggestions: Vec::new(),
            available_commands: Vec::new(),
            blocked_commands: Vec::new(),
            lesssecure_set: false,

            // Capability stubs
            capability_report: None,
            gpu_capabilities: Vec::new(),
            cpu_capability: None,
            memory_capability: None,
            network_interfaces: None,
            storage_capability: None,
            software_capability: None,
            manifest_written: false,
            socket_available: false,

            // Config subscription
            subscriptions: HashMap::new(),
            received_updates: Vec::new(),

            // Boot
            boot_phases_completed: Vec::new(),
            boot_stream_chunks: Vec::new(),

            // CLI
            cli_output: None,
            cli_exit_code: None,

            // Federation
            sovra_reachable: true,
            federated_templates: Vec::new(),
            compliance_reports: Vec::new(),

            // Observability
            loki_enabled: false,
            loki_events: Vec::new(),
            metrics_available: false,
            health_status: None,

            // Errors
            last_error: None,
            last_denial_reason: None,

            // MCP
            mcp_active: false,

            // Journal cluster
            journal_reachable: true,
            journal_leader: Some(1),
            journal_cluster_size: 3,

            // Policy flags
            opa_available: true,
            policy_degraded: false,

            // Rollback/alert
            rollback_triggered: false,
            rollback_deferred: false,
            active_consumer_count: 0,
            alert_raised: false,

            // Merge conflict
            conflict_mgr: pact_agent::conflict::ConflictManager::new(900),
            conflict_local_value: None,
            conflict_journal_value: None,

            // Resource isolation
            cgroup_manager: None,
            cgroup_scopes: HashMap::new(),
            cgroup_fail_services: Vec::new(),
            audit_events: Vec::new(),
            emergency_session_active: false,
            emergency_session_identity: None,
            frozen_slices: Vec::new(),
            namespace_sets: HashMap::new(),
            systemd_scope_created: None,
            direct_cgroup_entries_created: false,
            scope_processes: HashMap::new(),
            killed_scopes: Vec::new(),
            operation_denied: false,
            metric_read_value: None,

            // Workload integration (extended)
            handoff_server: None,
            handoff_socket_available: true,
            lattice_standalone: false,
            readiness_signal_emitted: false,
            queued_requests: Vec::new(),
            active_allocations: HashMap::new(),
            agent_restarted: false,
            ended_allocations_during_crash: Vec::new(),

            // Identity mapping
            uid_map: None,
            identity_mode: pact_common::types::IdentityMode::OnDemand,
            last_auth_subject: None,
            last_assigned_uid: None,
            nfs_configured: false,
            passwd_db_created: false,
            group_db_created: false,
            nsswitch_configured: false,
            uid_map_loaded: false,
            journal_committed: false,
            db_files_updated: false,
            nss_lookup_result: None,
            nss_lookup_local: false,
            nss_no_network: false,
            nss_not_found: false,
            nss_fallthrough: false,
            service_waiting_for_uid_map: false,
            service_started_after_resolve: false,

            // Workload integration
            mount_manager: None,

            // Network management
            network_configs: Vec::new(),
            network_interface_states: Vec::new(),
            network_config_will_fail: false,
            network_configured: false,
            network_default_route: None,
            network_configured_by_pact: false,

            // Boot phase state (platform_bootstrap)
            boot_phase_order: Vec::new(),
            boot_phase_fail: None,
            boot_state: "Idle".to_string(),
            boot_failed_at: None,
            boot_retried: false,
            boot_failure_resolved: false,
            running_as_pid1: false,
            boot_start_time: None,
            boot_end_time: None,
            warm_journal: false,

            // Watchdog
            watchdog_available: false,
            watchdog_handle_opened: false,
            watchdog_petted: false,
            watchdog_timeout_seconds: None,
            supervision_loop_hung: false,
            watchdog_timer_expired: false,
            bmc_reboot_triggered: false,

            // Adaptive supervision loop
            has_active_allocations: false,
            supervision_poll_interval_ms: 1000,
            deep_inspections: false,
            cpu_usage_percent: 0.0,

            // Bootstrap identity / SPIRE
            bootstrap_identity_available: false,
            authenticated_with_bootstrap: false,
            spire_agent_reachable: false,
            spire_agent_became_reachable: false,
            svid_obtained: false,
            spire_mtls_active: false,
            bootstrap_identity_discarded: false,
            spire_retry_active: false,
            no_spire_agent: false,

            // Device coldplug
            device_nodes_setup: false,
            kernel_modules_loaded: false,
            device_permissions_set: false,
            hotplug_daemon_running: false,

            // Node assignment
            node_vcluster_assignment: None,

            // Diag fleet
            diag_fleet_nodes: Vec::new(),
            diag_unreachable_nodes: Vec::new(),

            // Auth (hpc-auth)
            auth_server_url: None,
            auth_cache_dir: None,
            auth_browser_available: true,
            auth_idp_reachable: true,
            auth_server_reachable: true,
            auth_manual_idp_url: None,
            auth_manual_idp_override: false,
            auth_idp_supports_pkce: true,
            auth_idp_supports_device_code: true,
            auth_idp_supports_confidential: true,
            auth_selected_flow: None,
            auth_login_attempted: false,
            auth_login_succeeded: false,
            auth_error: None,
            auth_token_stored: false,
            auth_cache_modified: false,
            auth_flow_initiated: false,
            auth_permission_mode: "strict".to_string(),
            auth_cache_permissions: 0o600,
            auth_permissions_warning: false,
            auth_permissions_fixed: false,
            auth_cached_discovery: false,
            auth_cached_discovery_stale: false,
            auth_default_server: None,
            auth_server_tokens: HashMap::new(),
            auth_client_credentials_valid: true,
            auth_revocation_attempted: false,
            auth_last_cli_command: None,
            auth_silent_refresh: false,
            auth_refresh_scopes: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Cucumber runner
// ---------------------------------------------------------------------------

fn main() {
    // Use tokio runtime so that PactSupervisor (tokio::process::Command) works
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(PactWorld::cucumber().run("features/"));
}
