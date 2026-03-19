# Agent Interfaces

Trait interfaces for agent subsystems and gRPC service interfaces.

---

## ServiceManager Trait

```rust
/// Process supervision interface. Two implementations:
/// - PactSupervisor (default): direct fork/exec, cgroup v2
/// - SystemdBackend (feature "systemd"): D-Bus delegation
/// Source: ADR-006, process_supervisor.feature
#[async_trait]
pub trait ServiceManager: Send + Sync {
    /// Start a service. Respects dependency ordering (invariant A6).
    async fn start(&self, service: &ServiceDecl) -> Result<(), PactError>;
    /// Stop a running service. Sends SIGTERM → grace period → SIGKILL.
    async fn stop(&self, service: &ServiceDecl) -> Result<(), PactError>;
    /// Restart a service (stop + start).
    async fn restart(&self, service: &ServiceDecl) -> Result<(), PactError>;
    /// Get current service state.
    async fn status(&self, service: &ServiceDecl) -> Result<ServiceInstance, PactError>;
    /// Run health check (process, HTTP, or TCP).
    async fn health(&self, service: &ServiceDecl) -> Result<bool, PactError>;
}
```

**Contract:**
- `start` logs ServiceLifecycle entry to journal (process_supervisor.feature: scenario 16)
- `stop` uses reverse dependency order for ordered shutdown (scenario 12)
- Restart policy enforced automatically (scenarios 7-10)
- ServiceInstance tracks pid, uptime, restart_count

## GpuBackend Trait

```rust
/// GPU hardware detection. Feature-gated per vendor.
/// - NvidiaBackend (feature "nvidia"): NVML + nvidia-smi fallback
/// - AmdBackend (feature "amd"): ROCm SMI + rocm-smi fallback
/// - MockGpuBackend: for macOS dev/test
/// Source: capability_reporting.feature
#[async_trait]
pub trait GpuBackend: Send + Sync {
    /// Detect all GPUs and return capability info.
    async fn detect(&self) -> Result<Vec<GpuCapability>, PactError>;
}
```

## CpuBackend Trait

```rust
/// CPU hardware detection. Parses /proc/cpuinfo and /sys/devices/system/cpu/.
/// - LinuxCpuBackend: reads /proc/cpuinfo (model, features, core count),
///   /sys/devices/system/cpu/ (frequency, topology), /sys/devices/system/node/ (NUMA)
/// - MockCpuBackend: configurable for tests and macOS development
/// No feature flag needed — uses standard Linux interfaces.
/// Source: hardware_detection.feature
#[async_trait]
pub trait CpuBackend: Send + Sync {
    /// Detect CPU capabilities and return snapshot.
    async fn detect(&self) -> Result<CpuCapability, PactError>;
}
```

## MemoryBackend Trait

```rust
/// Memory hardware detection. Parses /proc/meminfo and sysfs NUMA topology.
/// - LinuxMemoryBackend: reads /proc/meminfo (total, available, hugepages),
///   /sys/devices/system/node/node*/meminfo (NUMA topology),
///   optional dmidecode --type 17 for memory type (needs root, graceful fallback)
/// - MockMemoryBackend: configurable for tests and macOS development
/// No feature flag needed — uses standard Linux interfaces.
/// Source: hardware_detection.feature
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Detect memory capabilities and return snapshot.
    async fn detect(&self) -> Result<MemoryCapability, PactError>;
}
```

## NetworkBackend Trait

```rust
/// Network interface detection. Parses /sys/class/net/*/.
/// - LinuxNetworkBackend: reads /sys/class/net/*/speed (link speed),
///   /sys/class/net/*/operstate (link state), /sys/class/net/*/address (MAC),
///   /sys/class/net/*/device/driver symlink (driver → fabric: cxi = Slingshot)
/// - MockNetworkBackend: configurable for tests and macOS development
/// No feature flag needed — uses standard Linux interfaces.
/// Source: hardware_detection.feature
#[async_trait]
pub trait NetworkBackend: Send + Sync {
    /// Detect all network interfaces and return per-interface info.
    async fn detect(&self) -> Result<Vec<NetworkInterface>, PactError>;
}
```

## StorageBackend Trait

```rust
/// Storage detection. Parses /sys/block/, /proc/mounts, and statvfs().
/// - LinuxStorageBackend: reads /sys/block/nvme*/ (NVMe devices),
///   /proc/mounts (active mounts), statvfs() for real capacity per mount.
///   Node is Diskless if no /sys/block/nvme* or /sys/block/sd* found.
/// - MockStorageBackend: configurable for tests and macOS development
/// No feature flag needed — uses standard Linux interfaces.
/// Source: hardware_detection.feature
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Detect storage capabilities and return snapshot.
    async fn detect(&self) -> Result<StorageCapability, PactError>;
}
```

## Capability Report Delivery

**Manifest file**: JSON at `/run/pact/capability.json`
- Written atomically (write to temp + rename)
- Schema matches CapabilityReport proto serialized as JSON

**Unix socket**: `/run/pact/capability.sock`
- Request-response: client sends empty request, server responds with latest CapabilityReport as JSON
- Used by lattice-node-agent for live polling

**Timeouts**:
- dmidecode subprocess: 2 second timeout. On timeout → MemoryType::Unknown.
- statvfs() per mount: 2 second timeout via tokio::time::timeout on blocking task. On timeout → total_bytes=0, available_bytes=0.
- Network speed read: parse /sys/class/net/*/speed as i64. Negative values (including -1) → speed_mbps=0.

**Interface filtering**:
- Include: all interfaces with `/sys/class/net/*/device` symlink (physical NICs)
- Exclude: loopback (`lo`), interfaces without device symlink (pure virtual: bridges, tunnels)
- VLANs: included (they have device symlinks pointing to parent)
- Mark unknown-driver interfaces as NetworkFabric::Unknown

## StateObserver Trait

```rust
/// System state observation. Multiple implementations compose together.
/// - EbpfObserver (feature "ebpf"): system-level tracepoints
/// - InotifyObserver: config file path watches
/// - NetlinkObserver: interface/address/mount changes
/// - MockObserver: for macOS dev/test
/// Source: drift_detection.feature, ADR-002
#[async_trait]
pub trait StateObserver: Send + Sync {
    /// Start observing. Emits DriftEvents through the channel.
    async fn start(&self, tx: mpsc::Sender<DriftEvent>) -> Result<(), PactError>;
    /// Stop observing.
    async fn stop(&self) -> Result<(), PactError>;
}
```

**Contract:**
- Events for blacklisted paths are filtered before emission (invariant D1)
- Observe-only mode: events emitted but no commit windows opened (D5)
- Multiple observers run concurrently, all feed same drift evaluator

## DriftEvaluator Interface

```rust
/// Compares actual vs declared state, computes DriftVector.
/// Source: drift_detection.feature, invariants D1-D5
pub struct DriftEvaluator {
    pub blacklist: BlacklistConfig,
    pub weights: DriftWeights,
}

impl DriftEvaluator {
    /// Process a drift event. Returns updated DriftVector if not blacklisted.
    pub fn evaluate(&self, event: &DriftEvent) -> Option<DriftVector>;
    /// Compute total magnitude from vector.
    /// Formula: weighted Euclidean norm (invariant D3: non-negative).
    pub fn magnitude(&self, vector: &DriftVector) -> f64;
}
```

## CommitWindowManager Interface

```rust
/// Manages the commit window lifecycle.
/// Source: commit_window.feature, invariants A1, A3, A4, A5
pub struct CommitWindowManager {
    pub active_window: Option<CommitWindow>,
    pub config: CommitWindowConfig,
}

impl CommitWindowManager {
    /// Open a commit window based on drift magnitude.
    /// Invariant A1: at most one active window.
    pub fn open(&mut self, drift: &DriftVector, magnitude: f64) -> &CommitWindow;
    /// Commit: close window, record in journal.
    pub async fn commit(&mut self, journal: &dyn JournalClient) -> Result<(), PactError>;
    /// Rollback: close window, check active consumers (A5), revert state.
    pub async fn rollback(&mut self, journal: &dyn JournalClient) -> Result<(), PactError>;
    /// Check expiry, trigger auto-rollback if expired (A4).
    /// Exception: emergency mode suspends auto-rollback.
    pub async fn tick(&mut self, now: DateTime<Utc>, journal: &dyn JournalClient) -> Result<(), PactError>;
}
```

## ShellService (shell.proto)

```rust
#[tonic::async_trait]
impl ShellService for AgentServer {
    /// Execute single command. Whitelisted, fork/exec'd directly.
    /// Auth: OIDC token in metadata (P1). Whitelist check (S1).
    /// State-changing commands trigger commit window (S5).
    /// All commands logged to journal (S4).
    type ExecStream: Stream<Item = Result<ExecOutput, Status>>;
    async fn exec(&self, request: ExecRequest)
        -> Result<Response<Self::ExecStream>, Status>;

    /// Interactive shell session. Restricted bash (S3).
    /// Auth: requires higher privilege than exec (shell_session.feature: scenario 4).
    /// Bidirectional stream: ShellInput/ShellOutput.
    /// Session recorded in journal (S4).
    async fn shell(&self, request: Request<Streaming<ShellInput>>)
        -> Result<Response<Self::ShellStream>, Status>;
}
```

**Contract:**
- Whitelist enforced via PATH restriction, not command parsing (ADR-007)
- Platform admin can bypass whitelist (S2), still logged
- Shell does NOT pre-classify commands — drift observer detects changes (S6)
- Learning mode captures command-not-found events

## DiagService (on ShellService)

```rust
/// Collect diagnostic logs from the node. Server-side filtering.
/// Source: diag_retrieval.feature
/// Authorization: pact-ops-{vcluster} or pact-platform-admin (LOG1)
type CollectDiagStream: Stream<Item = Result<DiagChunk, Status>>;
async fn collect_diag(&self, request: DiagRequest)
    -> Result<Response<Self::CollectDiagStream>, Status>;
```

```protobuf
message DiagRequest {
    string source_filter = 1;     // "system", "service", "all" (default: "all")
    string service_name = 2;      // specific service (empty = all services)
    string grep_pattern = 3;      // server-side grep (empty = no filter)
    uint32 line_limit = 4;        // max lines per source (0 = default 100, max 10000)
}

message DiagChunk {
    string source = 1;            // "dmesg", "syslog", "nvidia-persistenced", etc.
    repeated string lines = 2;    // batch of log lines
    bool truncated = 3;           // true if hit line_limit for this source
}
```

**Contract:**
- Agent reads from local sources only (no network calls)
- Grep is applied per-line before transmission (LOG2)
- Line limit enforced per source, not total (LOG3)
- PactSupervisor mode: reads /dev/kmsg (dmesg), /var/log/syslog or /var/log/messages, /run/pact/logs/{service}.log
- Systemd mode: runs `dmesg`, `journalctl --no-pager -n {limit}`, `journalctl -u {service} --no-pager -n {limit}`
- Missing source: skip with `DiagChunk { source, lines: [], truncated: false }` (F43)
