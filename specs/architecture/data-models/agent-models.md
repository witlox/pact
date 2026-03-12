# Agent Data Models (pact-agent)

Runtime entities managed by the agent on each node. These are the missing entities identified in the architect review.

---

## ServiceInstance (review finding #13)

Tracks a running service process. Distinct from ServiceDecl (declaration) and ServiceState (enum).

```rust
/// A running service instance managed by the supervisor.
pub struct ServiceInstance {
    pub decl: ServiceDecl,
    pub state: ServiceState,
    pub pid: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub uptime_seconds: u64,
    pub restart_count: u32,
    pub last_health_check: Option<DateTime<Utc>>,
    pub last_health_status: Option<bool>,
}
// Source: domain-model.md ServiceInstance entity
// Used by: capability_reporting.feature, process_supervisor.feature
```

## CommitWindow (review finding #4)

Runtime tracking of an active commit window. Distinct from CommitWindowConfig.

```rust
/// An active commit window on a node.
/// Invariant A1: at most one active per node.
pub struct CommitWindow {
    pub opened_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub drift_vector: DriftVector,
    pub drift_magnitude: f64,
    pub base_window_seconds: u32,
    pub sensitivity: f64,
    pub window_seconds: u32,        // computed: base / (1 + magnitude * sensitivity)
    pub scope: Scope,
}

impl CommitWindow {
    /// Compute window duration from drift magnitude.
    /// Formula (invariant A3): window = base / (1 + magnitude * sensitivity)
    pub fn compute_duration(base: u32, magnitude: f64, sensitivity: f64) -> u32;

    /// Check if window has expired.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool;
}
// Source: commit_window.feature, system-architecture.md commit window formula
// Invariant A4: expiry triggers auto-rollback (unless emergency)
// Invariant A5: rollback checks active consumers first
```

## EmergencySession (review finding #5)

Runtime tracking of an active emergency mode session.

```rust
/// An active emergency mode session on a node.
/// Invariant A2: at most one active per node.
pub struct EmergencySession {
    pub started_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reason: String,
    pub admin: Identity,
    pub window_seconds: u32,          // from VClusterPolicy.emergency_window_seconds
    pub changes: Vec<EntrySeq>,       // entries made during emergency
    pub default_ttl_seconds: Option<u32>,  // default TTL for emergency changes
}

impl EmergencySession {
    /// Check if emergency has become stale (exceeded window).
    /// Stale triggers: Loki alert + lattice cordon (failure-modes.md: F4)
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool;
}
// Source: emergency_mode.feature, ADR-004
// Invariant A10: does NOT expand shell whitelist
```

## ShellSession

```rust
/// An active interactive shell session.
pub struct ShellSession {
    pub session_id: String,
    pub node_id: NodeId,
    pub user: Identity,
    pub started_at: DateTime<Utc>,
    pub vcluster_id: VClusterId,
    pub commands_executed: u32,
}
// Source: shell_session.feature
// Invariant S3: restricted bash environment
// Invariant S4: every command logged via PROMPT_COMMAND
```

## ConfigCache (for partition resilience)

```rust
/// Cached configuration for operation during journal partition.
/// Source: failure-modes.md F1, F2, F3; invariant A9
pub struct ConfigCache {
    pub vcluster_policy: VClusterPolicy,
    pub overlays: HashMap<VClusterId, BootOverlay>,
    pub last_sequence: EntrySeq,
    pub cached_at: DateTime<Utc>,
    /// Operations performed while partitioned, for replay on reconnect.
    pub pending_replay: Vec<ConfigEntry>,
}
// Invariant P7: degraded mode restrictions apply when using cache
```

## DriftEvent

```rust
/// A drift event emitted by any observer (eBPF, inotify, netlink).
pub struct DriftEvent {
    pub timestamp: DateTime<Utc>,
    pub source: DriftSource,
    pub dimension: DriftDimension,
    pub key: String,
    pub detail: String,
}

pub enum DriftSource { Ebpf, Inotify, Netlink, Manual }

pub enum DriftDimension {
    Mounts, Files, Network, Services, Kernel, Packages, Gpu,
}
// Source: drift_detection.feature
// Invariant D1: blacklisted paths produce no drift events
```
