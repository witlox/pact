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

    // --- Merge conflict ---
    pub conflict_local_value: Option<String>,
    pub conflict_journal_value: Option<String>,

    // --- Resource isolation ---
    pub cgroup_manager: Option<Box<dyn hpc_node::CgroupManager>>,

    // --- Identity mapping ---
    pub uid_map: Option<pact_common::types::UidMap>,
    pub identity_mode: pact_common::types::IdentityMode,
    pub last_auth_subject: Option<String>,
    pub last_assigned_uid: Option<u32>,

    // --- Workload integration ---
    pub mount_manager: Option<pact_agent::handoff::MountRefManager>,

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
            alert_raised: false,

            // Merge conflict
            conflict_local_value: None,
            conflict_journal_value: None,

            // Resource isolation
            cgroup_manager: None,

            // Identity mapping
            uid_map: None,
            identity_mode: pact_common::types::IdentityMode::OnDemand,
            last_auth_subject: None,
            last_assigned_uid: None,

            // Workload integration
            mount_manager: None,

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
