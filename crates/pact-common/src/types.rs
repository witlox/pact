//! Core domain types for pact.
//!
//! All public types derive `Debug, Clone, Serialize, Deserialize` where possible.
//! Algebraic types (enums) for state, not strings.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Semantic type aliases for clarity.
pub type NodeId = String;
pub type VClusterId = String;
pub type DomainId = String;
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
    NodeEnrolled,
    NodeActivated,
    NodeDeactivated,
    NodeDecommissioned,
    NodeAssigned,
    NodeUnassigned,
    CertSigned,
    CertRevoked,
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
    /// Memory limit (e.g., "512M", "1G"). Maps to cgroup `memory.max`.
    pub cgroup_memory_max: Option<String>,
    /// cgroup slice to place this service in (e.g., "pact.slice/gpu.slice").
    /// Defaults to "pact.slice/infra.slice" if not set.
    #[serde(default)]
    pub cgroup_slice: Option<String>,
    /// CPU weight (1-10000). Maps to cgroup `cpu.weight`. Default: 100.
    #[serde(default)]
    pub cgroup_cpu_weight: Option<u16>,
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
    pub cpu: CpuCapability,
    pub gpus: Vec<GpuCapability>,
    pub memory: MemoryCapability,
    pub network: Vec<NetworkInterface>,
    pub storage: StorageCapability,
    pub software: SoftwareCapability,
    pub config_state: ConfigState,
    pub drift_summary: Option<DriftVector>,
    pub emergency: Option<EmergencyInfo>,
    pub supervisor_status: SupervisorStatus,
}

/// CPU architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CpuArchitecture {
    X86_64,
    Aarch64,
    #[default]
    Unknown,
}

/// CPU capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCapability {
    pub architecture: CpuArchitecture,
    /// CPU model name, e.g. "Intel Xeon w9-3495X", "NVIDIA Grace".
    pub model: String,
    pub physical_cores: u32,
    /// Logical cores including SMT/HT threads.
    pub logical_cores: u32,
    pub base_frequency_mhz: u32,
    /// Turbo/boost frequency.
    pub max_frequency_mhz: u32,
    /// ISA features: "avx512f", "sve", "amx", etc.
    pub features: Vec<String>,
    /// Number of NUMA nodes (matches memory topology).
    pub numa_nodes: u32,
    /// Total L3 cache across all sockets.
    pub cache_l3_bytes: u64,
}

impl Default for CpuCapability {
    fn default() -> Self {
        Self {
            architecture: CpuArchitecture::default(),
            model: String::new(),
            physical_cores: 0,
            logical_cores: 0,
            base_frequency_mhz: 0,
            max_frequency_mhz: 0,
            features: Vec::new(),
            numa_nodes: 1,
            cache_l3_bytes: 0,
        }
    }
}

/// Memory type classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MemoryType {
    Ddr4,
    Ddr5,
    Hbm2e,
    Hbm3,
    Hbm3e,
    #[default]
    Unknown,
}

/// A NUMA node with per-node memory and CPU affinity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumaNode {
    pub id: u32,
    pub total_bytes: u64,
    /// Logical CPU IDs in this NUMA node.
    pub cpus: Vec<u32>,
}

/// Huge page allocation information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HugePageInfo {
    pub size_2mb_total: u64,
    pub size_2mb_free: u64,
    pub size_1gb_total: u64,
    pub size_1gb_free: u64,
}

/// Memory capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCapability {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub memory_type: MemoryType,
    pub numa_nodes: u32,
    /// Per-node memory and CPU affinity.
    pub numa_topology: Vec<NumaNode>,
    pub hugepages: HugePageInfo,
}

impl Default for MemoryCapability {
    fn default() -> Self {
        Self {
            total_bytes: 0,
            available_bytes: 0,
            memory_type: MemoryType::default(),
            numa_nodes: 1,
            numa_topology: Vec::new(),
            hugepages: HugePageInfo::default(),
        }
    }
}

/// Network fabric type detected from driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkFabric {
    Slingshot,
    Ethernet,
    Unknown,
}

/// Operational state of a network interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceOperState {
    Up,
    Down,
}

/// A detected network interface on the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub fabric: NetworkFabric,
    pub speed_mbps: u64,
    pub state: InterfaceOperState,
    pub mac: String,
    pub ipv4: Option<String>,
}

/// Whether this node has local storage or is diskless.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageNodeType {
    Diskless,
    LocalStorage,
}

/// Type of local disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiskType {
    Nvme,
    Ssd,
    Hdd,
    Unknown,
}

/// Filesystem type for a mount point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsType {
    Nfs,
    Lustre,
    Ext4,
    Xfs,
    Tmpfs,
    Other(String),
}

/// A local disk detected on the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDisk {
    pub device: String,
    pub model: String,
    pub capacity_bytes: u64,
    pub disk_type: DiskType,
}

/// A mount point with filesystem and capacity information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountInfo {
    pub path: String,
    pub fs_type: FsType,
    pub source: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// Storage capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCapability {
    pub node_type: StorageNodeType,
    pub local_disks: Vec<LocalDisk>,
    pub mounts: Vec<MountInfo>,
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

impl BootOverlay {
    /// Create a new overlay with an automatically computed checksum (J5).
    pub fn new(vcluster_id: impl Into<VClusterId>, version: u64, data: Vec<u8>) -> Self {
        let checksum = compute_overlay_checksum(&data);
        Self { vcluster_id: vcluster_id.into(), version, data, checksum }
    }
}

/// Compute a deterministic checksum for overlay data (invariant J5).
pub fn compute_overlay_checksum(data: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:x}", hasher.finish())
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
    NodeEnroll,
    NodeDecommission,
    NodeAssign,
    NodeUnassign,
    NodeMove,
    CertRenew,
    CertRevoke,
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

/// Enrollment state of a node in the pact domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnrollmentState {
    /// Admin has registered the node but agent has not yet connected.
    Registered,
    /// Agent has connected, CSR signed, mTLS established.
    Active,
    /// Heartbeat timeout — agent has not been seen within the timeout window.
    Inactive,
    /// Administratively decommissioned — certificate revoked.
    Revoked,
}

/// Hardware identity presented by a node during enrollment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareIdentity {
    /// Primary MAC address.
    pub mac_address: String,
    /// BMC/IPMI serial number (optional — not all hardware exposes this).
    #[serde(default)]
    pub bmc_serial: Option<String>,
    /// Additional hardware identifiers (SMBIOS UUID, etc.).
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

/// Enrollment record for a node in the pact domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEnrollment {
    pub node_id: NodeId,
    pub domain_id: DomainId,
    pub state: EnrollmentState,
    pub hardware_identity: HardwareIdentity,
    /// vCluster assignment (None = maintenance mode).
    pub vcluster_id: Option<VClusterId>,
    /// Signed certificate serial number (set after CSR signing).
    pub cert_serial: Option<String>,
    /// Certificate expiry (set after CSR signing).
    pub cert_expires_at: Option<DateTime<Utc>>,
    /// Last time the node was seen (heartbeat).
    pub last_seen: Option<DateTime<Utc>>,
    /// When the enrollment record was created.
    pub enrolled_at: DateTime<Utc>,
    /// Who enrolled this node.
    pub enrolled_by: Identity,
    /// Number of active shell/exec sessions on this node.
    #[serde(default)]
    pub active_sessions: u32,
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
            cpu: CpuCapability::default(),
            memory: MemoryCapability {
                total_bytes: 549_755_813_888,
                available_bytes: 500_000_000_000,
                memory_type: MemoryType::default(),
                numa_nodes: 2,
                numa_topology: vec![],
                hugepages: HugePageInfo::default(),
            },
            network: vec![NetworkInterface {
                name: "cxi0".into(),
                fabric: NetworkFabric::Slingshot,
                speed_mbps: 200_000,
                state: InterfaceOperState::Up,
                mac: "00:11:22:33:44:55".into(),
                ipv4: None,
            }],
            storage: StorageCapability {
                node_type: StorageNodeType::Diskless,
                local_disks: vec![],
                mounts: vec![MountInfo {
                    path: "/scratch".into(),
                    fs_type: FsType::Lustre,
                    source: "mds01:/scratch".into(),
                    total_bytes: 1_073_741_824,
                    available_bytes: 500_000_000,
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
        assert_eq!(decoded.network.len(), 1);
        assert_eq!(decoded.network[0].fabric, NetworkFabric::Slingshot);
        assert_eq!(decoded.memory.numa_nodes, 2);
    }
}

// --- Identity Mapping types (ADR-016) ---

/// Single OIDC → POSIX UID/GID mapping entry.
///
/// Immutable once assigned within a federation membership (IM1).
/// Assigned sequentially within the org's precursor range (IM3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UidEntry {
    /// OIDC subject identifier (e.g., "user@cscs.ch").
    pub subject: String,
    /// POSIX user ID.
    pub uid: u32,
    /// Primary POSIX group ID.
    pub gid: u32,
    /// Username for NSS (e.g., "pwitlox").
    pub username: String,
    /// Home directory.
    pub home: String,
    /// Login shell.
    pub shell: String,
    /// Organization identifier (for federation).
    pub org: String,
}

/// Group mapping entry for full supplementary group resolution (IM4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupEntry {
    /// Group name.
    pub name: String,
    /// POSIX group ID.
    pub gid: u32,
    /// Member usernames.
    pub members: Vec<String>,
}

/// Federated org index for UID/GID precursor range computation (IM2).
///
/// `precursor = base_uid + org_index * stride`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrgIndex {
    /// Organization identifier.
    pub org: String,
    /// Sequential index assigned on federation join (0 = local).
    pub index: u32,
}

/// Complete OIDC → POSIX mapping table.
///
/// Stored in journal Raft state, cached on agents as tmpfs .db files.
/// Only active when `SupervisorBackend::Pact` and NFS in use (IM6).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UidMap {
    /// User entries keyed by OIDC subject.
    pub users: HashMap<String, UidEntry>,
    /// Group entries keyed by group name.
    pub groups: HashMap<String, GroupEntry>,
    /// Organization indices for federation.
    pub org_indices: Vec<OrgIndex>,
    /// Base UID for precursor computation. Default: 10000.
    pub base_uid: u32,
    /// Base GID for precursor computation. Default: 10000.
    pub base_gid: u32,
    /// Stride (max users per org). Default: 10000.
    pub stride: u32,
    /// Next UID to assign per org (org name → next offset within range).
    pub next_uid_offset: HashMap<String, u32>,
}

/// Identity assignment mode per vCluster.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityMode {
    /// On-demand: unknown subjects get UIDs assigned automatically.
    #[default]
    OnDemand,
    /// Pre-provisioned: unknown subjects rejected (IM4).
    PreProvisioned,
}

impl UidMap {
    /// Create with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self { base_uid: 10_000, base_gid: 10_000, stride: 10_000, ..Default::default() }
    }

    /// Compute UID precursor for an org.
    #[must_use]
    pub fn uid_precursor(&self, org_index: u32) -> u32 {
        self.base_uid + org_index * self.stride
    }

    /// Compute GID precursor for an org.
    #[must_use]
    pub fn gid_precursor(&self, org_index: u32) -> u32 {
        self.base_gid + org_index * self.stride
    }

    /// Get org index for an org name.
    #[must_use]
    pub fn org_index(&self, org: &str) -> Option<u32> {
        self.org_indices.iter().find(|o| o.org == org).map(|o| o.index)
    }

    /// Assign a new UID for a subject in the given org.
    ///
    /// Returns the assigned `UidEntry` or an error if the range is exhausted.
    pub fn assign_uid(
        &mut self,
        subject: &str,
        username: &str,
        org: &str,
        home: &str,
        shell: &str,
    ) -> Result<UidEntry, crate::error::PactError> {
        // Check if already assigned
        if let Some(existing) = self.users.get(subject) {
            return Ok(existing.clone());
        }

        // Get org index
        let idx = self
            .org_index(org)
            .ok_or_else(|| crate::error::PactError::OrgNotRegistered(org.to_string()))?;

        // Get next offset within range
        let offset = self.next_uid_offset.get(org).copied().unwrap_or(0);
        if offset >= self.stride {
            return Err(crate::error::PactError::UidRangeExhausted {
                org: org.to_string(),
                stride: self.stride,
                assigned: offset,
            });
        }

        let uid = self.uid_precursor(idx) + offset;
        let gid = self.gid_precursor(idx) + offset;

        let entry = UidEntry {
            subject: subject.to_string(),
            uid,
            gid,
            username: username.to_string(),
            home: home.to_string(),
            shell: shell.to_string(),
            org: org.to_string(),
        };

        self.users.insert(subject.to_string(), entry.clone());
        self.next_uid_offset.insert(org.to_string(), offset + 1);

        Ok(entry)
    }

    /// Remove all entries for an org (federation departure GC).
    pub fn gc_org(&mut self, org: &str) {
        self.users.retain(|_, e| e.org != org);
        self.groups.retain(|_, g| {
            g.members.retain(|m| !self.users.values().any(|u| &u.username == m && u.org == org));
            true
        });
        self.next_uid_offset.remove(org);
        self.org_indices.retain(|o| o.org != org);
    }

    /// Look up a user by UID.
    #[must_use]
    pub fn get_by_uid(&self, uid: u32) -> Option<&UidEntry> {
        self.users.values().find(|e| e.uid == uid)
    }

    /// Look up a user by username.
    #[must_use]
    pub fn get_by_username(&self, username: &str) -> Option<&UidEntry> {
        self.users.values().find(|e| e.username == username)
    }
}

#[test]
fn uid_map_precursor_computation() {
    let map = UidMap::new();
    assert_eq!(map.uid_precursor(0), 10_000); // local
    assert_eq!(map.uid_precursor(1), 20_000); // partner-a
    assert_eq!(map.uid_precursor(2), 30_000); // partner-b
    assert_eq!(map.gid_precursor(0), 10_000);
    assert_eq!(map.gid_precursor(1), 20_000);
}

#[test]
fn uid_map_assign_and_lookup() {
    let mut map = UidMap::new();
    map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });

    let entry =
        map.assign_uid("user@cscs.ch", "pwitlox", "local", "/users/pwitlox", "/bin/bash").unwrap();
    assert_eq!(entry.uid, 10_000);
    assert_eq!(entry.gid, 10_000);
    assert_eq!(entry.username, "pwitlox");

    // Second user
    let entry2 =
        map.assign_uid("user2@cscs.ch", "jdoe", "local", "/users/jdoe", "/bin/bash").unwrap();
    assert_eq!(entry2.uid, 10_001);

    // Lookup by UID
    assert_eq!(map.get_by_uid(10_000).unwrap().username, "pwitlox");
    assert_eq!(map.get_by_username("jdoe").unwrap().uid, 10_001);
}

#[test]
fn uid_map_idempotent_assign() {
    let mut map = UidMap::new();
    map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });

    let e1 =
        map.assign_uid("user@cscs.ch", "pwitlox", "local", "/users/pwitlox", "/bin/bash").unwrap();
    let e2 =
        map.assign_uid("user@cscs.ch", "pwitlox", "local", "/users/pwitlox", "/bin/bash").unwrap();
    assert_eq!(e1.uid, e2.uid); // Same UID, idempotent
}

#[test]
fn uid_map_range_exhaustion() {
    let mut map = UidMap { stride: 2, ..UidMap::new() };
    map.org_indices.push(OrgIndex { org: "small".into(), index: 0 });

    map.assign_uid("u1@x", "u1", "small", "/u1", "/bin/bash").unwrap();
    map.assign_uid("u2@x", "u2", "small", "/u2", "/bin/bash").unwrap();
    let err = map.assign_uid("u3@x", "u3", "small", "/u3", "/bin/bash");
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("exhausted"));
}

#[test]
fn uid_map_federation_gc() {
    let mut map = UidMap::new();
    map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    map.org_indices.push(OrgIndex { org: "partner".into(), index: 1 });

    map.assign_uid("u1@local", "u1", "local", "/u1", "/bin/bash").unwrap();
    map.assign_uid("u2@partner", "u2", "partner", "/u2", "/bin/bash").unwrap();

    assert_eq!(map.users.len(), 2);

    map.gc_org("partner");
    assert_eq!(map.users.len(), 1);
    assert!(map.get_by_username("u1").is_some());
    assert!(map.get_by_username("u2").is_none());
    assert!(map.org_index("partner").is_none());
}

#[test]
fn uid_map_serialization_roundtrip() {
    let mut map = UidMap::new();
    map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    map.assign_uid("user@cscs.ch", "pwitlox", "local", "/users/pwitlox", "/bin/bash").unwrap();

    let json = serde_json::to_string(&map).unwrap();
    let deser: UidMap = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.users.len(), 1);
    assert_eq!(deser.get_by_username("pwitlox").unwrap().uid, 10_000);
}
