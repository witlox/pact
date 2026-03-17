# hpc-audit Interface Definitions

Shared contract crate in hpc-core. Defines audit event types and sink trait. Loose coupling, high coherence — each system owns its log, shared format for SIEM forwarding.

**Source:** domain-model.md Cross-cutting: Audit
**Invariants:** O3 (audit trail continuity)
**Design:** Loose coupling, high coherence (confirmed by domain expert)

---

## Audit Event Types

```rust
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// Universal audit event. Both pact and lattice emit these.
/// Source: domain-model.md Cross-cutting: Audit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event ID
    pub id: String,
    /// When the event occurred
    pub timestamp: DateTime<Utc>,
    /// Who performed the action
    pub principal: AuditPrincipal,
    /// What action was performed
    pub action: String,
    /// Where (node, vCluster, allocation)
    pub scope: AuditScope,
    /// Success or failure
    pub outcome: AuditOutcome,
    /// Human-readable detail
    pub detail: String,
    /// Structured metadata (action-specific)
    pub metadata: serde_json::Value,
    /// Source system
    pub source: AuditSource,
}

/// Who performed the action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPrincipal {
    /// OIDC subject or service identity
    pub identity: String,
    /// Principal type
    pub principal_type: PrincipalType,
    /// Role at time of action
    pub role: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalType {
    Human,
    Agent,
    Service,
    System, // internal pact/lattice operations (e.g., supervision loop restart)
}

/// Where the action occurred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditScope {
    pub node_id: Option<String>,
    pub vcluster_id: Option<String>,
    pub allocation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

/// Which system emitted the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditSource {
    PactAgent,
    PactJournal,
    PactCli,
    LatticeNodeAgent,
    LatticeQuorum,
    LatticeCli,
}

/// Well-known action strings for cross-system consistency.
/// Source: feature files (resource_isolation.feature, workload_integration.feature, etc.)
pub mod actions {
    // Process supervision
    pub const SERVICE_START: &str = "service.start";
    pub const SERVICE_STOP: &str = "service.stop";
    pub const SERVICE_RESTART: &str = "service.restart";
    pub const SERVICE_CRASH: &str = "service.crash";

    // Resource isolation
    pub const CGROUP_CREATE: &str = "cgroup.create";
    pub const CGROUP_DESTROY: &str = "cgroup.destroy";
    pub const CGROUP_KILL_FAILED: &str = "cgroup.kill_failed";
    pub const EMERGENCY_FREEZE: &str = "emergency.freeze";
    pub const EMERGENCY_KILL: &str = "emergency.kill";

    // Identity mapping
    pub const UID_ASSIGNED: &str = "identity.uid_assigned";
    pub const UID_RANGE_EXHAUSTED: &str = "identity.range_exhausted";
    pub const FEDERATION_DEPARTURE_GC: &str = "identity.federation_gc";

    // Workload integration
    pub const NAMESPACE_HANDOFF: &str = "namespace.handoff";
    pub const NAMESPACE_HANDOFF_FAILED: &str = "namespace.handoff_failed";
    pub const NAMESPACE_CLEANUP: &str = "namespace.cleanup";
    pub const NAMESPACE_LEAK_DETECTED: &str = "namespace.leak_detected";
    pub const MOUNT_ACQUIRE: &str = "mount.acquire";
    pub const MOUNT_RELEASE: &str = "mount.release";
    pub const MOUNT_FORCE_UNMOUNT: &str = "mount.force_unmount";
    pub const MOUNT_REFCOUNT_CORRECTED: &str = "mount.refcount_corrected";

    // Network
    pub const NETWORK_CONFIGURED: &str = "network.configured";
    pub const NETWORK_FAILED: &str = "network.failed";
    pub const NETWORK_LINK_LOST: &str = "network.link_lost";

    // Bootstrap
    pub const BOOT_PHASE_COMPLETE: &str = "boot.phase_complete";
    pub const BOOT_PHASE_FAILED: &str = "boot.phase_failed";
    pub const BOOT_READY: &str = "boot.ready";
    pub const WATCHDOG_TIMEOUT: &str = "boot.watchdog_timeout";
    pub const SPIRE_SVID_ACQUIRED: &str = "boot.spire_svid_acquired";
}
```

---

## Audit Sink Trait

```rust
/// Destination for audit events.
/// Implementations: journal append, file write, SIEM forward, Loki push.
/// Source: domain-model.md Cross-cutting: Audit
///
/// CONTRACT: emit() must not block. Implementations should buffer
/// and flush asynchronously. Audit trail continuity (O3) is the
/// responsibility of the sink implementation, not the caller.
pub trait AuditSink: Send + Sync {
    /// Emit an audit event.
    /// Must not block the caller. Buffer internally if needed.
    fn emit(&self, event: AuditEvent);

    /// Flush any buffered events. Called on graceful shutdown.
    async fn flush(&self) -> Result<(), AuditError>;
}

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("audit sink unavailable: {reason}")]
    SinkUnavailable { reason: String },
    #[error("audit flush failed: {reason}")]
    FlushFailed { reason: String },
}
```

---

## Compliance Policy

```rust
/// Retention and compliance requirements.
/// Source: domain-model.md Cross-cutting: CompliancePolicy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompliancePolicy {
    /// Minimum retention period for audit events
    pub retention_days: u32,
    /// Whether all access must be logged (regulated/sensitive)
    pub log_all_access: bool,
    /// Required audit points — actions that MUST emit events
    pub required_audit_points: Vec<String>,
}

impl Default for CompliancePolicy {
    fn default() -> Self {
        Self {
            retention_days: 365, // 1 year default
            log_all_access: false,
            required_audit_points: vec![
                actions::SERVICE_CRASH.to_string(),
                actions::EMERGENCY_FREEZE.to_string(),
                actions::EMERGENCY_KILL.to_string(),
                actions::UID_ASSIGNED.to_string(),
                actions::NAMESPACE_HANDOFF_FAILED.to_string(),
                actions::BOOT_PHASE_FAILED.to_string(),
            ],
        }
    }
}

/// Regulated/sensitive compliance policy.
pub fn regulated_compliance() -> CompliancePolicy {
    CompliancePolicy {
        retention_days: 2555, // 7 years
        log_all_access: true,
        required_audit_points: vec![
            // All default points plus additional regulated requirements
            actions::SERVICE_START.to_string(),
            actions::SERVICE_STOP.to_string(),
            actions::CGROUP_CREATE.to_string(),
            actions::CGROUP_DESTROY.to_string(),
            actions::NAMESPACE_HANDOFF.to_string(),
            actions::MOUNT_ACQUIRE.to_string(),
            actions::MOUNT_RELEASE.to_string(),
            // ... all actions for full audit trail
        ],
    }
}
```
