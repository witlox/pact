# hpc-node Interface Definitions

Shared contract crate in hpc-core. Defines cgroup conventions, namespace handoff protocol, mount conventions, and readiness signaling. Both pact and lattice implement independently.

**Source:** domain-model.md §2b (Resource Isolation), §2f (Workload Integration)
**Invariants:** RI1, RI6, WI1-WI6

---

## Cgroup Contract

```rust
/// Cgroup slice ownership — who has exclusive write access.
/// Source: invariant RI1 (exclusive slice ownership)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SliceOwner {
    /// pact-agent owns this slice (system services)
    Pact,
    /// lattice-node-agent owns this slice (workloads)
    Workload,
}

/// Well-known cgroup slice paths.
/// Source: domain-model.md §2b cgroup hierarchy
pub mod slices {
    pub const PACT_ROOT: &str = "pact.slice";
    pub const PACT_INFRA: &str = "pact.slice/infra.slice";
    pub const PACT_NETWORK: &str = "pact.slice/network.slice";
    pub const PACT_GPU: &str = "pact.slice/gpu.slice";
    pub const PACT_AUDIT: &str = "pact.slice/audit.slice";
    pub const WORKLOAD_ROOT: &str = "workload.slice";
}

/// Returns the owner of a given cgroup path.
/// Source: invariant RI1
pub fn slice_owner(path: &str) -> Option<SliceOwner> {
    // CONTRACT: paths under pact.slice/ → Pact, under workload.slice/ → Workload
    todo!()
}

/// Resource limits for a cgroup scope.
/// Source: domain-model.md §2b ResourceLimits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Memory limit in bytes (maps to memory.max). None = unlimited.
    pub memory_max: Option<u64>,
    /// CPU weight (1-10000, maps to cpu.weight). None = default (100).
    pub cpu_weight: Option<u16>,
    /// IO max in bytes/sec per device. None = unlimited.
    pub io_max: Option<u64>,
}

/// Trait for cgroup hierarchy management.
/// Both pact (direct) and lattice (standalone) implement this.
/// Source: invariants RI1-RI6, WI4
pub trait CgroupManager: Send + Sync {
    /// Create top-level slice hierarchy.
    /// Called once at boot. Idempotent.
    fn create_hierarchy(&self) -> Result<(), CgroupError>;

    /// Create a scoped cgroup for a service or allocation.
    /// Returns a handle for process placement.
    /// Source: invariant RI2
    fn create_scope(
        &self,
        parent_slice: &str,
        name: &str,
        limits: &ResourceLimits,
    ) -> Result<CgroupHandle, CgroupError>;

    /// Kill all processes in a scope and release it.
    /// Source: invariant PS3 (immediate kill, no grace period)
    fn destroy_scope(&self, handle: &CgroupHandle) -> Result<(), CgroupError>;

    /// Read metrics from any slice (shared read, invariant RI6).
    fn read_metrics(&self, path: &str) -> Result<CgroupMetrics, CgroupError>;

    /// Check if a scope is empty (no processes).
    /// Source: invariant WI5 (namespace cleanup on cgroup empty)
    fn is_scope_empty(&self, handle: &CgroupHandle) -> Result<bool, CgroupError>;
}

/// Opaque handle to a created cgroup scope.
/// Passed to process spawn for placement.
/// Source: domain-model.md §2b CgroupHandle
#[derive(Debug, Clone)]
pub struct CgroupHandle {
    /// Full cgroup path (e.g., /sys/fs/cgroup/pact.slice/gpu.slice/nvidia-persistenced)
    pub path: String,
}

/// Metrics read from a cgroup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgroupMetrics {
    pub memory_current: u64,
    pub memory_max: Option<u64>,
    pub cpu_usage_usec: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    #[error("cgroup creation failed: {reason}")]
    CreationFailed { reason: String },
    #[error("cgroup.kill failed: {reason}")]
    KillFailed { reason: String },
    #[error("cgroup path not found: {path}")]
    NotFound { path: String },
    #[error("permission denied: {path} owned by {owner:?}")]
    PermissionDenied { path: String, owner: SliceOwner },
    #[error("cgroup I/O error: {source}")]
    Io { source: std::io::Error },
}
```

---

## Namespace Handoff Protocol

```rust
/// Namespace types that can be created and handed off.
/// Source: domain-model.md §2b NamespaceSet
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamespaceType {
    Pid,
    Net,
    Mount,
}

/// Request from lattice to pact for allocation namespaces.
/// Sent over unix socket.
/// Source: interaction N7, invariant WI1
#[derive(Debug, Serialize, Deserialize)]
pub struct NamespaceRequest {
    pub allocation_id: String,
    pub namespaces: Vec<NamespaceType>,
    /// Optional uenv image to mount
    pub uenv_image: Option<String>,
}

/// Response from pact to lattice with namespace FDs.
/// FDs are passed via SCM_RIGHTS on the unix socket.
/// Source: invariant WI1
#[derive(Debug, Serialize, Deserialize)]
pub struct NamespaceResponse {
    pub allocation_id: String,
    /// Number of FDs attached via SCM_RIGHTS, in order matching requested types
    pub fd_count: usize,
    /// Mount point for uenv bind-mount inside the mount namespace (if requested)
    pub uenv_mount_path: Option<String>,
}

/// Notification that an allocation has ended.
/// Source: invariant WI5 (cgroup-empty detection)
#[derive(Debug, Serialize, Deserialize)]
pub struct AllocationEnded {
    pub allocation_id: String,
}

/// Well-known socket path for namespace handoff.
/// Source: assumption A-Int7
pub const HANDOFF_SOCKET_PATH: &str = "/run/pact/handoff.sock";

/// Trait for the namespace handoff provider (pact implements).
/// Source: interaction N7, domain-model.md §2f
pub trait NamespaceProvider: Send + Sync {
    /// Create namespaces for an allocation and return FDs.
    fn create_namespaces(
        &self,
        request: &NamespaceRequest,
    ) -> Result<NamespaceResponse, NamespaceError>;

    /// Release namespaces for a completed allocation.
    fn release_namespaces(&self, allocation_id: &str) -> Result<(), NamespaceError>;
}

/// Trait for the namespace handoff consumer (lattice implements).
/// When pact is not available, lattice uses its own implementation.
/// Source: invariant WI4 (lattice standalone), feature workload_integration.feature
pub trait NamespaceConsumer: Send + Sync {
    /// Request namespaces from the provider (pact).
    /// Falls back to self-service if provider unavailable (F27).
    fn request_namespaces(
        &self,
        request: &NamespaceRequest,
    ) -> Result<NamespaceResponse, NamespaceError>;
}

#[derive(Debug, thiserror::Error)]
pub enum NamespaceError {
    #[error("handoff socket unavailable: {reason}")]
    SocketUnavailable { reason: String },
    #[error("namespace creation failed: {reason}")]
    CreationFailed { reason: String },
    #[error("allocation not found: {allocation_id}")]
    AllocationNotFound { allocation_id: String },
    #[error("FD passing failed: {source}")]
    FdPassing { source: std::io::Error },
}
```

---

## Mount Conventions

```rust
/// Well-known mount paths.
/// Source: domain-model.md §2f mount refcounting
pub mod mount_paths {
    /// Base directory for uenv SquashFS mounts
    pub const UENV_MOUNT_BASE: &str = "/run/pact/uenv";
    /// Base directory for allocation working directories
    pub const WORKDIR_BASE: &str = "/run/pact/workdir";
    /// Base directory for data staging mounts
    pub const DATA_STAGE_BASE: &str = "/run/pact/data";
}

/// Mount reference with refcount tracking.
/// Source: invariant WI2 (refcount accuracy), WI3 (lazy unmount)
pub trait MountManager: Send + Sync {
    /// Acquire a reference to a uenv mount. Mounts if first reference.
    /// Source: workload_integration.feature "First allocation mounts uenv image"
    fn acquire_mount(&self, image_path: &str) -> Result<MountHandle, MountError>;

    /// Release a reference. Starts hold timer when refcount reaches zero.
    /// Source: invariant WI3
    fn release_mount(&self, handle: &MountHandle) -> Result<(), MountError>;

    /// Force-unmount regardless of refcount or hold timer.
    /// Only allowed during emergency mode (RI3).
    /// Source: invariant WI3 exception
    fn force_unmount(&self, image_path: &str) -> Result<(), MountError>;

    /// Reconstruct refcounts from kernel mount table + allocation state.
    /// Called on agent restart.
    /// Source: invariant WI6
    fn reconstruct_state(
        &self,
        active_allocations: &[String],
    ) -> Result<(), MountError>;
}

/// Handle to an acquired mount.
#[derive(Debug, Clone)]
pub struct MountHandle {
    pub image_path: String,
    pub mount_point: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("mount failed: {reason}")]
    MountFailed { reason: String },
    #[error("unmount failed: {reason}")]
    UnmountFailed { reason: String },
    #[error("refcount inconsistency: {detail}")]
    RefcountInconsistency { detail: String },
    #[error("mount I/O error: {source}")]
    Io { source: std::io::Error },
}
```

---

## Readiness Protocol

```rust
/// Readiness signal emitted by pact when node is fully initialized.
/// Source: domain-model.md §2e ReadinessSignal, invariant PB3
pub const READINESS_SOCKET_PATH: &str = "/run/pact/ready.sock";
pub const READINESS_FILE_PATH: &str = "/run/pact/ready";

/// Trait for readiness signaling.
/// Source: workload_integration.feature "Readiness gate signals lattice"
pub trait ReadinessGate: Send + Sync {
    /// Check if the node is ready for allocations.
    fn is_ready(&self) -> bool;

    /// Wait until the node is ready. Returns immediately if already ready.
    /// Source: workload_integration.feature "Lattice requests before readiness are queued"
    async fn wait_ready(&self) -> Result<(), ReadinessError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ReadinessError {
    #[error("boot failed: {reason}")]
    BootFailed { reason: String },
    #[error("timeout waiting for readiness")]
    Timeout,
}
```
