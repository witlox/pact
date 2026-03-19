# Shared Kernel Data Models (pact-common)

All types shared across crate boundaries. These are the canonical domain types.

**Design note:** Where Rust types and proto types diverge, this document describes the **target** Rust types. Proto alignment issues from the architect review are resolved here.

---

## Identity & Authorization

```rust
pub type NodeId = String;
pub type VClusterId = String;
pub type EntrySeq = u64;

pub struct Identity {
    pub principal: String,          // email or service account
    pub principal_type: PrincipalType,
    pub role: String,               // OIDC role claim
}
// Invariant J3: principal and role must be non-empty

pub enum PrincipalType { Human, Agent, Service }
// Proto alignment: proto comment says "admin" — fix to "human"

pub struct RoleBinding {
    pub role: String,
    pub principals: Vec<String>,
    pub allowed_actions: Vec<String>,
}
```

## Configuration State

```rust
pub enum ConfigState {
    ObserveOnly,    // Initial deployment mode (ADR-002)
    Committed,      // Declared = actual
    Drifted,        // Declared ≠ actual, commit window open
    Converging,     // Auto-converge in progress
    Emergency,      // Extended window, no auto-rollback (ADR-004)
}

pub enum EntryType {
    Commit, Rollback, AutoConverge, DriftDetected,
    CapabilityChange, PolicyUpdate, BootConfig,
    EmergencyStart, EmergencyEnd,
    ExecLog, ShellSession, ServiceLifecycle,
    PendingApproval,  // Two-person approval workflow
    NodeEnrolled,     // Node registered in enrollment registry (ADR-008)
    NodeActivated,    // Node boot enrollment succeeded (ADR-008)
    NodeDeactivated,  // Node heartbeat timeout (ADR-008)
    NodeDecommissioned, // Node revoked and removed (ADR-008)
    NodeAssigned,     // Node assigned to vCluster (ADR-008)
    NodeUnassigned,   // Node removed from vCluster (ADR-008)
    CertSigned,       // CSR signed by journal intermediate CA (ADR-008)
    CertRevoked,      // Certificate revoked via Raft revocation registry (ADR-008)
}
// Proto fix needed: add ENTRY_TYPE_PENDING_APPROVAL to config.proto

pub enum Scope {
    Global,
    VCluster(VClusterId),
    Node(NodeId),
}

pub struct ConfigEntry {
    pub sequence: EntrySeq,
    pub timestamp: DateTime<Utc>,
    pub entry_type: EntryType,
    pub scope: Scope,
    pub author: Identity,           // Invariant J3: non-empty
    pub parent: Option<EntrySeq>,   // Invariant J4: parent < sequence
    pub state_delta: Option<StateDelta>,
    pub policy_ref: Option<String>,
    pub ttl_seconds: Option<u32>,   // Proto fix: align proto to u32 (not Duration)
    pub emergency_reason: Option<String>,
}
```

## State Deltas & Drift

```rust
pub struct StateDelta {
    pub mounts: Vec<DeltaItem>,
    pub files: Vec<DeltaItem>,
    pub network: Vec<DeltaItem>,
    pub services: Vec<DeltaItem>,
    pub kernel: Vec<DeltaItem>,
    pub packages: Vec<DeltaItem>,
    pub gpu: Vec<DeltaItem>,
}

pub struct DeltaItem {
    pub action: DeltaAction,
    pub key: String,
    pub value: Option<String>,
    pub previous: Option<String>,
}

pub enum DeltaAction { Add, Remove, Modify }

/// Drift magnitude per dimension — used for commit window formula.
/// This is the Rust-native computation type (f64 magnitudes).
pub struct DriftVector {
    pub mounts: f64,
    pub files: f64,
    pub network: f64,
    pub services: f64,
    pub kernel: f64,
    pub packages: f64,
    pub gpu: f64,
}
// Note: Proto DriftVector uses repeated Delta* messages (detailed deltas).
// Resolution: Rust DriftVector stays as f64 magnitudes for computation.
// Proto DriftVector stays as detailed deltas for wire format.
// Conversion: DriftVector::from_delta(&StateDelta) computes magnitudes.
// Agent computes DriftVector locally; proto carries StateDelta for detail.

pub struct DriftWeights {
    pub mounts: f64,  // default 1.0
    pub files: f64,   // default 1.0
    pub network: f64, // default 1.0
    pub services: f64,// default 1.0
    pub kernel: f64,  // default 2.0
    pub packages: f64,// default 1.0
    pub gpu: f64,     // default 2.0
}
```

## VCluster Policy (17 fields, matching policy.proto)

```rust
pub struct VClusterPolicy {
    pub vcluster_id: VClusterId,
    pub policy_id: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub drift_sensitivity: f64,              // default 2.0
    pub base_commit_window_seconds: u32,     // default 900
    pub emergency_window_seconds: u32,       // default 14400
    pub auto_converge_categories: Vec<String>,
    pub require_ack_categories: Vec<String>,
    pub enforcement_mode: String,            // "observe" | "warn" | "enforce"
    pub role_bindings: Vec<RoleBinding>,
    pub regulated: bool,
    pub two_person_approval: bool,
    pub emergency_allowed: bool,             // default true
    pub audit_retention_days: u32,           // default 2555
    pub federation_template: Option<String>,
    pub supervisor_backend: String,          // "pact" | "systemd"
    pub exec_whitelist: Vec<String>,
    pub shell_whitelist: Vec<String>,
}
// Default impl: permissive observe-only (ADR-002 bootstrap)
```

## Boot Overlay

```rust
pub struct BootOverlay {
    pub vcluster_id: VClusterId,
    pub version: u64,
    pub data: Vec<u8>,              // zstd compressed
    pub checksum: String,           // Invariant J5: matches hash of data
}
```

## Service Declaration & State

```rust
pub enum ServiceState {
    Starting, Running, Stopping, Stopped, Failed, Restarting,
}

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

pub enum RestartPolicy { Always, OnFailure, Never }

pub struct HealthCheck {
    pub check_type: HealthCheckType,
    pub interval_seconds: u32,
}

pub enum HealthCheckType {
    Process,
    Http { url: String },
    Tcp { port: u16 },
}

pub enum SupervisorBackend { Pact, Systemd }
```

## Capability Reporting

```rust
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

// --- GPU (unchanged, already complete) ---

pub enum GpuVendor { Nvidia, Amd }
pub enum GpuHealth { Healthy, Degraded, Failed }

pub struct GpuCapability {
    pub index: u32,
    pub vendor: GpuVendor,
    pub model: String,
    pub memory_bytes: u64,
    pub health: GpuHealth,
    pub pci_bus_id: String,
}

// --- CPU ---

pub enum CpuArchitecture { X86_64, Aarch64, Unknown }

pub struct CpuCapability {
    pub architecture: CpuArchitecture,
    pub model: String,                    // e.g. "Intel Xeon w9-3495X", "NVIDIA Grace"
    pub physical_cores: u32,
    pub logical_cores: u32,               // includes SMT/HT threads
    pub base_frequency_mhz: u32,
    pub max_frequency_mhz: u32,          // turbo/boost frequency
    pub features: Vec<String>,            // ISA features: "avx512f", "sve", "amx", etc.
    pub numa_nodes: u32,                  // number of NUMA nodes (matches memory topology)
    pub cache_l3_bytes: u64,              // total L3 cache across all sockets
}

// --- Memory (expanded with NUMA topology, huge pages, memory type) ---

pub enum MemoryType { Ddr4, Ddr5, Hbm2e, Hbm3, Hbm3e, Unknown }

pub struct NumaNode {
    pub id: u32,
    pub total_bytes: u64,
    pub cpus: Vec<u32>,                   // logical CPU IDs in this NUMA node
}
```

**NUMA CPU list**: `NumaNode.cpus` contains logical CPU IDs (0-based) parsed from
`/sys/devices/system/node/node*/cpulist`. Range format "0-27,112-139" is expanded
to individual IDs [0,1,...,27,112,...,139].

**CPU features**: `CpuCapability.features` contains raw flag strings from
`/proc/cpuinfo flags` field (x86_64) or `Features` field (aarch64). No
normalization is performed — consumers must understand vendor-specific naming.

```rust
pub struct HugePageInfo {
    pub size_2mb_total: u64,
    pub size_2mb_free: u64,
    pub size_1gb_total: u64,
    pub size_1gb_free: u64,
}

pub struct MemoryCapability {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub memory_type: MemoryType,
    pub numa_nodes: u32,
    pub numa_topology: Vec<NumaNode>,     // per-node memory and CPU affinity
    pub hugepages: HugePageInfo,
}

// --- Network (per-interface, replacing single struct) ---

pub enum NetworkFabric { Slingshot, Ethernet, Unknown }
pub enum InterfaceOperState { Up, Down }

pub struct NetworkInterface {
    pub name: String,                     // e.g. "cxi0", "eth0"
    pub fabric: NetworkFabric,            // detected from driver: cxi → Slingshot
    pub speed_mbps: u64,                  // from /sys/class/net/*/speed (0 if unknown)
    pub state: InterfaceOperState,        // from /sys/class/net/*/operstate
    pub mac: String,                      // from /sys/class/net/*/address
    pub ipv4: Option<String>,             // primary IPv4 address if assigned
}

// --- Storage (expanded with NVMe, real capacity) ---

pub enum StorageNodeType { Diskless, LocalStorage }
pub enum DiskType { Nvme, Ssd, Hdd, Unknown }
pub enum FsType { Nfs, Lustre, Ext4, Xfs, Tmpfs, Other(String) }

pub struct LocalDisk {
    pub device: String,                   // e.g. "/dev/nvme0n1"
    pub model: String,                    // from /sys/block/*/device/model
    pub capacity_bytes: u64,              // from /sys/block/*/size * 512
    pub disk_type: DiskType,
}

pub struct MountInfo {
    pub path: String,                     // mount point
    pub fs_type: FsType,                  // filesystem type
    pub source: String,                   // device or NFS server:path
    pub total_bytes: u64,                 // from statvfs()
    pub available_bytes: u64,             // from statvfs()
}

pub struct StorageCapability {
    pub node_type: StorageNodeType,
    pub local_disks: Vec<LocalDisk>,      // empty for diskless nodes
    pub mounts: Vec<MountInfo>,           // active mounts with real capacity
}

// --- Software (unchanged) ---

pub struct SoftwareCapability {
    pub loaded_modules: Vec<String>,
    pub uenv_image: Option<String>,
    pub services: Vec<ServiceStatusInfo>,
}

pub struct ServiceStatusInfo {
    pub name: String,
    pub state: ServiceState,
    pub pid: u32,
    pub uptime_seconds: u64,
    pub restart_count: u32,
}

pub struct EmergencyInfo {
    pub reason: String,
    pub admin_identity: Identity,  // Proto fix: change proto from string to Identity message
    pub started_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct SupervisorStatus {
    pub backend: SupervisorBackend,
    pub services_declared: u32,
    pub services_running: u32,
    pub services_failed: u32,
}
```

**Note**: Service counts in `SupervisorStatus` are a point-in-time snapshot taken
at `CapabilityReport.timestamp`. Between reports, services may crash or restart.
Consumers should treat counts as advisory.

## Node Enrollment & Domain Membership (ADR-008)

```rust
/// Source: ADR-008, domain-model.md Node Management context, invariants E1-E10

pub enum EnrollmentState {
    Registered,  // Enrolled by admin, cert pre-signed, awaiting first boot
    Active,      // Node connected, cert served, mTLS established
    Inactive,    // Node disconnected (heartbeat timeout), cert may still be valid
    Revoked,     // Admin decommissioned, cert revoked via Raft revocation registry
}

pub struct HardwareIdentity {
    pub mac_addresses: Vec<String>,       // Primary NIC MAC(s)
    pub bmc_serial: String,               // SMBIOS/DMI BMC serial
    pub tpm_ek_hash: Option<String>,      // TPM endorsement key hash (optional)
}
// Detection: mac from /sys/class/net/*/address, bmc from /sys/class/dmi/id/board_serial

pub struct NodeEnrollment {
    pub node_id: NodeId,
    pub hardware_identity: HardwareIdentity,
    pub domain_id: String,                // Which pact domain
    pub enrolled_by: Identity,            // Admin who enrolled (E10)
    pub enrolled_at: DateTime<Utc>,
    pub state: EnrollmentState,
    pub vcluster_id: Option<VClusterId>,  // None = maintenance mode (E8)
    pub assigned_by: Option<Identity>,
    pub assigned_at: Option<DateTime<Utc>>,
    pub cert_serial: Option<String>,      // Serial of last signed cert (public info)
    pub cert_not_after: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>, // Subscription stream liveness
}
// Invariant E2: hardware_identity unique within a domain
// Invariant E3: Active in at most one domain (physical constraint)
// Invariant E4: CSR signed locally by journal intermediate CA — no private keys stored
// Invariant E8: vcluster_id is independent of enrollment state
// Note: NO private key material in this struct or in Raft state

pub struct SignedCert {
    pub cert_pem: String,       // Signed by journal's intermediate CA (public)
    pub serial: String,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
}
// No key_pem — agent holds its own private key in RAM only
```

---

## Admin Operations & Audit

```rust
pub struct AdminOperation {
    pub operation_id: String,
    pub timestamp: DateTime<Utc>,
    pub actor: Identity,
    pub operation_type: AdminOperationType,
    pub scope: Scope,
    pub detail: String,
}

pub enum AdminOperationType {
    Exec, ShellSessionStart, ShellSessionEnd,
    ServiceStart, ServiceStop, ServiceRestart,
    EmergencyStart, EmergencyEnd,
    ApprovalDecision,
    NodeEnroll,         // Admin enrolled a node (ADR-008)
    NodeDecommission,   // Admin decommissioned a node (ADR-008)
    NodeAssign,         // Admin assigned node to vCluster (ADR-008)
    NodeUnassign,       // Admin unassigned node from vCluster (ADR-008)
    NodeMove,           // Admin moved node between vClusters (ADR-008)
}
// Proto fix needed: define AdminOperationType enum in proto

pub enum ApprovalStatus { Pending, Approved, Rejected, Expired }

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
// Stored as ConfigEntry(EntryType::PendingApproval) in journal.
// The PendingApproval struct is the state_delta payload.
// Approval/rejection creates a new ConfigEntry referencing the original via parent.
```
