//! Core domain types for pact.
//!
//! All public types derive `Debug, Clone, Serialize, Deserialize` where possible.
//! Algebraic types (enums) for state, not strings.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Semantic type aliases for clarity.
pub type NodeId = String;
pub type VClusterId = String;
pub type EntrySeq = u64;

/// Configuration state of a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigState {
    /// Observing only, no enforcement.
    ObserveOnly,
    /// All config committed, no drift.
    Committed,
    /// Drift detected, within commit window.
    Drifted,
    /// Actively converging to declared state.
    Converging,
    /// Emergency mode — extended window, no auto-rollback.
    Emergency,
}

/// Type of configuration entry in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryType {
    Commit,
    Rollback,
    AutoConverge,
    DriftDetected,
    CapabilityChange,
    PolicyUpdate,
    BootConfig,
    EmergencyStart,
    EmergencyEnd,
    ExecLog,
    ShellSession,
    ServiceLifecycle,
    PendingApproval,
}

/// Scope of a configuration entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Global,
    VCluster(VClusterId),
    Node(NodeId),
}

/// Identity of the actor performing an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub principal: String,
    pub principal_type: PrincipalType,
    pub role: String,
}

/// Type of principal performing an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalType {
    Human,
    Agent,
    Service,
}

/// An immutable configuration entry in the journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub sequence: EntrySeq,
    pub timestamp: DateTime<Utc>,
    pub entry_type: EntryType,
    pub scope: Scope,
    pub author: Identity,
    pub parent: Option<EntrySeq>,
    pub state_delta: Option<StateDelta>,
    pub policy_ref: Option<String>,
    pub ttl_seconds: Option<u32>,
    pub emergency_reason: Option<String>,
}

/// State delta representing changes in a config entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateDelta {
    pub mounts: Vec<DeltaItem>,
    pub files: Vec<DeltaItem>,
    pub network: Vec<DeltaItem>,
    pub services: Vec<DeltaItem>,
    pub kernel: Vec<DeltaItem>,
    pub packages: Vec<DeltaItem>,
    pub gpu: Vec<DeltaItem>,
}

/// A single change within a delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaItem {
    pub action: DeltaAction,
    pub key: String,
    pub value: Option<String>,
    pub previous: Option<String>,
}

/// Type of change in a delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaAction {
    Add,
    Remove,
    Modify,
}

/// Drift vector with magnitude per category.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriftVector {
    pub mounts: f64,
    pub files: f64,
    pub network: f64,
    pub services: f64,
    pub kernel: f64,
    pub packages: f64,
    pub gpu: f64,
}

impl DriftVector {
    /// Compute total drift magnitude (weighted L2 norm).
    #[must_use]
    #[allow(clippy::suboptimal_flops)]
    pub fn magnitude(&self, weights: &DriftWeights) -> f64 {
        let sum = weights.mounts * self.mounts * self.mounts
            + weights.files * self.files * self.files
            + weights.network * self.network * self.network
            + weights.services * self.services * self.services
            + weights.kernel * self.kernel * self.kernel
            + weights.packages * self.packages * self.packages
            + weights.gpu * self.gpu * self.gpu;
        sum.sqrt()
    }
}

/// Per-category weights for drift magnitude computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftWeights {
    pub mounts: f64,
    pub files: f64,
    pub network: f64,
    pub services: f64,
    pub kernel: f64,
    pub packages: f64,
    pub gpu: f64,
}

impl Default for DriftWeights {
    fn default() -> Self {
        Self {
            mounts: 1.0,
            files: 1.0,
            network: 1.0,
            services: 1.0,
            kernel: 2.0,
            packages: 1.0,
            gpu: 2.0,
        }
    }
}

/// Status of a supervised service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Restarting,
}

/// Declaration of a service to be supervised.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDecl {
    pub name: String,
    pub binary: String,
    pub args: Vec<String>,
    pub restart: RestartPolicy,
    pub restart_delay_seconds: u32,
    pub depends_on: Vec<String>,
    pub order: u32,
    pub cgroup_memory_max: Option<String>,
    pub health_check: Option<HealthCheck>,
}

/// Restart policy for supervised services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    Never,
}

/// Health check configuration for a service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub check_type: HealthCheckType,
    pub interval_seconds: u32,
}

/// Type of health check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthCheckType {
    Process,
    Http { url: String },
    Tcp { port: u16 },
}

/// Supervisor backend selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupervisorBackend {
    Pact,
    Systemd,
}

/// Hardware capability report from a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub node_id: NodeId,
    pub timestamp: DateTime<Utc>,
    pub report_id: Uuid,
    pub gpus: Vec<GpuCapability>,
    pub memory: MemoryCapability,
    pub network: Option<NetworkCapability>,
    pub storage: StorageCapability,
    pub software: SoftwareCapability,
    pub config_state: ConfigState,
    pub drift_summary: Option<DriftVector>,
    pub emergency: Option<EmergencyInfo>,
    pub supervisor_status: SupervisorStatus,
}

/// Memory capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCapability {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub numa_nodes: u32,
}

/// Network fabric capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapability {
    pub fabric_type: String,
    pub bandwidth_bps: u64,
    pub latency_us: f64,
}

/// Storage capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCapability {
    pub tmpfs_bytes: u64,
    pub mounts: Vec<MountPointInfo>,
}

/// Information about a mount point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountPointInfo {
    pub path: String,
    pub fs_type: String,
    pub source: String,
    pub available: bool,
}

/// Software capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareCapability {
    pub loaded_modules: Vec<String>,
    pub uenv_image: Option<String>,
    pub services: Vec<ServiceStatusInfo>,
}

/// Status information for a running service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatusInfo {
    pub name: String,
    pub state: ServiceState,
    pub pid: u32,
    pub uptime_seconds: u64,
    pub restart_count: u32,
}

/// Information about active emergency mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyInfo {
    pub reason: String,
    pub admin_identity: Identity,
    pub started_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// GPU vendor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
}

/// GPU health status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuHealth {
    Healthy,
    Degraded,
    Failed,
}

/// GPU capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuCapability {
    pub index: u32,
    pub vendor: GpuVendor,
    pub model: String,
    pub memory_bytes: u64,
    pub health: GpuHealth,
    pub pci_bus_id: String,
}

/// Status of the process supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorStatus {
    pub backend: SupervisorBackend,
    pub services_declared: u32,
    pub services_running: u32,
    pub services_failed: u32,
}

/// Role binding: maps a role to principals and allowed actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleBinding {
    pub role: String,
    pub principals: Vec<String>,
    pub allowed_actions: Vec<String>,
}

/// Policy for a vCluster — controls drift thresholds, commit windows, approvals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VClusterPolicy {
    pub vcluster_id: VClusterId,
    /// Unique identifier for this policy version.
    #[serde(default)]
    pub policy_id: String,
    /// When this policy was last updated.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Maximum drift magnitude before action is required.
    pub drift_sensitivity: f64,
    /// Base commit window in seconds.
    pub base_commit_window_seconds: u32,
    /// Emergency window in seconds (default 14400 = 4 hours).
    #[serde(default = "default_emergency_window_seconds")]
    pub emergency_window_seconds: u32,
    /// Categories that auto-converge without acknowledgment.
    #[serde(default)]
    pub auto_converge_categories: Vec<String>,
    /// Categories that require explicit acknowledgment.
    #[serde(default)]
    pub require_ack_categories: Vec<String>,
    /// Enforcement mode: "observe", "warn", "enforce".
    #[serde(default = "default_enforcement_mode")]
    pub enforcement_mode: String,
    /// Role bindings for this vCluster.
    #[serde(default)]
    pub role_bindings: Vec<RoleBinding>,
    /// Whether this vCluster handles regulated/sensitive workloads.
    #[serde(default)]
    pub regulated: bool,
    /// Whether two-person approval is required for state changes.
    #[serde(default)]
    pub two_person_approval: bool,
    /// Whether emergency mode is allowed.
    #[serde(default = "default_true_flag")]
    pub emergency_allowed: bool,
    /// Audit log retention in days (default 2555 = ~7 years).
    #[serde(default = "default_audit_retention_days")]
    pub audit_retention_days: u32,
    /// Federation policy template name (optional).
    #[serde(default)]
    pub federation_template: Option<String>,
    /// Supervisor backend: "pact" or "systemd".
    #[serde(default = "default_supervisor_backend_str")]
    pub supervisor_backend: String,
    /// Allowed commands for exec.
    #[serde(default)]
    pub exec_whitelist: Vec<String>,
    /// Allowed commands for shell.
    #[serde(default)]
    pub shell_whitelist: Vec<String>,
}

const fn default_emergency_window_seconds() -> u32 {
    14400
}

fn default_enforcement_mode() -> String {
    "observe".to_string()
}

const fn default_true_flag() -> bool {
    true
}

const fn default_audit_retention_days() -> u32 {
    2555
}

fn default_supervisor_backend_str() -> String {
    "pact".to_string()
}

impl Default for VClusterPolicy {
    fn default() -> Self {
        Self {
            vcluster_id: String::new(),
            policy_id: String::new(),
            updated_at: None,
            drift_sensitivity: 2.0,
            base_commit_window_seconds: 900,
            emergency_window_seconds: 14400,
            auto_converge_categories: Vec::new(),
            require_ack_categories: Vec::new(),
            enforcement_mode: "observe".to_string(),
            role_bindings: Vec::new(),
            regulated: false,
            two_person_approval: false,
            emergency_allowed: true,
            audit_retention_days: 2555,
            federation_template: None,
            supervisor_backend: "pact".to_string(),
            exec_whitelist: Vec::new(),
            shell_whitelist: Vec::new(),
        }
    }
}

/// Pre-computed boot overlay for a vCluster (compressed config bundle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootOverlay {
    pub vcluster_id: VClusterId,
    pub version: u64,
    pub data: Vec<u8>,
    pub checksum: String,
}

/// Record of an admin operation for the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminOperation {
    pub operation_id: String,
    pub timestamp: DateTime<Utc>,
    pub actor: Identity,
    pub operation_type: AdminOperationType,
    pub scope: Scope,
    pub detail: String,
}

/// Type of admin operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminOperationType {
    Exec,
    ShellSessionStart,
    ShellSessionEnd,
    ServiceStart,
    ServiceStop,
    ServiceRestart,
    EmergencyStart,
    EmergencyEnd,
    ApprovalDecision,
}

/// Status of a pending approval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

/// A pending two-person approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub approval_id: String,
    pub original_request: String,
    pub action: String,
    pub scope: Scope,
    pub requester: Identity,
    pub approver: Option<Identity>,
    pub status: ApprovalStatus,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_magnitude_zero_when_no_drift() {
        let drift = DriftVector {
            mounts: 0.0,
            files: 0.0,
            network: 0.0,
            services: 0.0,
            kernel: 0.0,
            packages: 0.0,
            gpu: 0.0,
        };
        let weights = DriftWeights::default();
        assert!((drift.magnitude(&weights) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn drift_magnitude_respects_weights() {
        let drift = DriftVector {
            mounts: 1.0,
            files: 0.0,
            network: 0.0,
            services: 0.0,
            kernel: 1.0,
            packages: 0.0,
            gpu: 0.0,
        };
        let weights = DriftWeights::default();
        // mounts: 1.0*1.0*1.0 = 1.0, kernel: 2.0*1.0*1.0 = 2.0 → sqrt(3.0)
        let expected = 3.0_f64.sqrt();
        assert!((drift.magnitude(&weights) - expected).abs() < 1e-10);
    }

    #[test]
    fn config_state_serialization_roundtrip() {
        let state = ConfigState::Drifted;
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: ConfigState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn service_decl_deserialize_from_toml() {
        let toml_str = r#"
            name = "chronyd"
            binary = "/usr/sbin/chronyd"
            args = ["-d"]
            restart = "Always"
            restart_delay_seconds = 5
            depends_on = []
            order = 1
        "#;
        let decl: ServiceDecl = toml::from_str(toml_str).unwrap();
        assert_eq!(decl.name, "chronyd");
        assert_eq!(decl.restart, RestartPolicy::Always);
        assert_eq!(decl.order, 1);
    }

    #[test]
    fn vcluster_policy_default_is_permissive() {
        let policy = VClusterPolicy::default();
        assert!((policy.drift_sensitivity - 2.0).abs() < f64::EPSILON);
        assert_eq!(policy.base_commit_window_seconds, 900);
        assert_eq!(policy.emergency_window_seconds, 14400);
        assert_eq!(policy.enforcement_mode, "observe");
        assert!(policy.emergency_allowed);
        assert!(!policy.two_person_approval);
        assert!(!policy.regulated);
        assert_eq!(policy.audit_retention_days, 2555);
        assert_eq!(policy.supervisor_backend, "pact");
    }

    #[test]
    fn role_binding_serde_roundtrip() {
        let binding = RoleBinding {
            role: "pact-ops-ml".into(),
            principals: vec!["alice@example.com".into()],
            allowed_actions: vec!["commit".into(), "exec".into()],
        };
        let json = serde_json::to_string(&binding).unwrap();
        let decoded: RoleBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.role, "pact-ops-ml");
        assert_eq!(decoded.principals.len(), 1);
        assert_eq!(decoded.allowed_actions.len(), 2);
    }

    #[test]
    fn pending_approval_serde_roundtrip() {
        let approval = PendingApproval {
            approval_id: "apr-001".into(),
            original_request: "commit config".into(),
            action: "commit".into(),
            scope: Scope::VCluster("ml-train".into()),
            requester: Identity {
                principal: "alice@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-ops-ml".into(),
            },
            approver: None,
            status: ApprovalStatus::Pending,
            created_at: Utc::now(),
            expires_at: Utc::now(),
        };
        let json = serde_json::to_string(&approval).unwrap();
        let decoded: PendingApproval = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.approval_id, "apr-001");
        assert_eq!(decoded.status, ApprovalStatus::Pending);
    }

    #[test]
    fn gpu_health_serde_roundtrip() {
        let health = GpuHealth::Degraded;
        let json = serde_json::to_string(&health).unwrap();
        let decoded: GpuHealth = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, GpuHealth::Degraded);
    }

    #[test]
    fn capability_report_with_all_fields() {
        let report = CapabilityReport {
            node_id: "node-001".into(),
            timestamp: Utc::now(),
            report_id: Uuid::new_v4(),
            gpus: vec![GpuCapability {
                index: 0,
                vendor: GpuVendor::Nvidia,
                model: "H100".into(),
                memory_bytes: 80_000_000_000,
                health: GpuHealth::Healthy,
                pci_bus_id: "0000:3b:00.0".into(),
            }],
            memory: MemoryCapability {
                total_bytes: 549_755_813_888,
                available_bytes: 500_000_000_000,
                numa_nodes: 2,
            },
            network: Some(NetworkCapability {
                fabric_type: "slingshot".into(),
                bandwidth_bps: 200_000_000_000,
                latency_us: 1.5,
            }),
            storage: StorageCapability {
                tmpfs_bytes: 1_073_741_824,
                mounts: vec![MountPointInfo {
                    path: "/scratch".into(),
                    fs_type: "lustre".into(),
                    source: "mds01:/scratch".into(),
                    available: true,
                }],
            },
            software: SoftwareCapability {
                loaded_modules: vec!["cuda/12.0".into()],
                uenv_image: Some("ml-base:latest".into()),
                services: vec![],
            },
            config_state: ConfigState::Committed,
            drift_summary: None,
            emergency: None,
            supervisor_status: SupervisorStatus {
                backend: SupervisorBackend::Pact,
                services_declared: 3,
                services_running: 3,
                services_failed: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: CapabilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.node_id, "node-001");
        assert_eq!(decoded.gpus.len(), 1);
        assert_eq!(decoded.gpus[0].health, GpuHealth::Healthy);
        assert!(decoded.network.is_some());
        assert_eq!(decoded.memory.numa_nodes, 2);
    }
}
