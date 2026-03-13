# Event Schemas

Payload definitions for all events in the pact system. These define the data contract between producers and consumers.

---

## ConfigEntry (Journal Event Payload)

The universal journal event. All config and admin events are recorded as ConfigEntry instances in the Raft log.

```rust
pub struct ConfigEntry {
    pub sequence: EntrySeq,              // u64, monotonic (J1)
    pub entry_type: EntryType,           // discriminator (13 variants)
    pub scope: Scope,                    // Global | VCluster(id) | Node(id)
    pub timestamp: DateTime<Utc>,
    pub author: Identity,                // principal + role (J3: non-empty)
    pub parent: Option<EntrySeq>,        // causal link (J4: acyclic)
    pub delta: Option<StateDelta>,       // what changed
    pub ttl: Option<u32>,                // seconds until expiry (emergency changes)
    pub metadata: HashMap<String, String>,
}
```

### EntryType Variants

| Variant | delta present? | ttl present? | parent typical? | Notes |
|---------|---------------|-------------|-----------------|-------|
| Commit | Yes | No | Yes (previous state) | Closes commit window |
| Rollback | Yes (reverse delta) | No | Yes (entry being rolled back) | Auto or manual |
| AutoConverge | Yes | No | Optional | Categories in `auto_converge_categories` |
| DriftDetected | Yes (drift vector) | No | No | Informational, no state change |
| CapabilityChange | Yes (capability diff) | No | No | Hardware state change |
| PolicyUpdate | Yes (policy diff) | No | Optional (previous policy) | Through Raft SetPolicy |
| BootConfig | Yes (overlay + delta) | No | No | First entry for a node boot |
| EmergencyStart | No | Yes (emergency window) | No | `ttl` = emergency_window_seconds |
| EmergencyEnd | No | No | Yes (EmergencyStart entry) | Links back to start |
| ExecLog | No | No | No | `metadata["command"]` has command |
| ShellSession | No | No | Optional (start links to end) | `metadata["action"]` = "start"/"end" |
| ServiceLifecycle | No | No | No | `metadata["service"]`, `metadata["action"]` |
| PendingApproval | No | Yes (approval timeout) | No | `metadata["approval_id"]` |

### Scope

```rust
pub enum Scope {
    Global,
    VCluster(VClusterId),
    Node(NodeId),
}
```

### Identity

```rust
pub struct Identity {
    pub principal: String,     // OIDC subject (e.g., "alice@example.com")
    pub role: String,          // OIDC role (e.g., "pact-ops-ml-training")
}
```

### StateDelta

```rust
pub struct StateDelta {
    pub changes: Vec<Change>,
}

pub struct Change {
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}
```

---

## DriftEvent (Agent Internal)

Ephemeral event flowing through `mpsc` channel within the agent process. Not persisted directly — summarized into DriftVector and recorded as DriftDetected ConfigEntry.

```rust
pub struct DriftEvent {
    pub timestamp: DateTime<Utc>,
    pub source: DriftSource,
    pub dimension: DriftDimension,
    pub key: String,               // e.g., "/etc/hosts", "eth0", "nvidia_uvm"
    pub detail: String,            // human-readable change description
}

pub enum DriftSource {
    Ebpf,       // eBPF tracepoint
    Inotify,    // filesystem watch
    Netlink,    // network/mount change
    Manual,     // operator-triggered scan
}

pub enum DriftDimension {
    Mounts,     // filesystem mount/umount
    Files,      // file content/permission changes
    Network,    // interface/address/route changes
    Services,   // process/service state changes
    Kernel,     // sysctl, modules, hostname
    Packages,   // package install/remove (if applicable)
    Gpu,        // GPU state/driver changes
}
```

### DriftVector (Aggregated State)

```rust
pub struct DriftVector {
    pub mounts: f64,
    pub files: f64,
    pub network: f64,
    pub services: f64,
    pub kernel: f64,
    pub packages: f64,
    pub gpu: f64,
}
```

**Magnitude formula (D3, D4):**
```
magnitude = sqrt(Σ (weight_i * dimension_i)²)
```

Default weights: `kernel=2.0, gpu=2.0, all others=1.0`

---

## AdminOperation (Audit Event)

```rust
pub struct AdminOperation {
    pub operation_type: AdminOperationType,
    pub identity: Identity,
    pub node_id: NodeId,
    pub vcluster_id: VClusterId,
    pub timestamp: DateTime<Utc>,
    pub detail: String,
}

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
```

---

## PendingApproval (Two-Person Workflow)

```rust
pub struct PendingApproval {
    pub approval_id: String,
    pub original_request: String,     // serialized operation description
    pub action: String,               // "commit", "exec", "service_start", etc.
    pub scope: Scope,
    pub requester: Identity,
    pub approver: Option<Identity>,   // filled when approved/rejected
    pub status: ApprovalStatus,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,    // P5: default 30 min timeout
}

pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}
```

**Constraint:** `approver.principal != requester.principal` (P4: same admin cannot self-approve)

---

## Conflict Events (Partition Reconnect + Promote)

### MergeConflict (Agent → Journal on reconnect)

```rust
pub struct MergeConflict {
    pub node_id: NodeId,
    pub vcluster_id: VClusterId,
    pub conflicts: Vec<ConflictEntry>,
    pub detected_at: DateTime<Utc>,
    pub grace_period_expires: DateTime<Utc>,
}

pub struct ConflictEntry {
    pub key: String,                    // config key (e.g., "kernel.shmmax")
    pub local_value: String,            // value on the agent
    pub journal_value: String,          // value in journal state
    pub local_changed_at: DateTime<Utc>,
    pub journal_changed_at: DateTime<Utc>,
}

pub enum ConflictResolution {
    AcceptLocal,      // keep agent's value, promote to journal
    AcceptJournal,    // overwrite agent with journal value
    GracePeriodExpired, // automatic journal-wins after timeout
}
```

**Producer:** Agent (on reconnect, after CR1 local feedback)
**Consumer:** Journal (records conflict), CLI (notifies admin per CR5), Loki (alert)
**Invariants:** CR2 (pause convergence), CR3 (grace period)

### PromoteConflict (CLI → Journal during promote)

```rust
pub struct PromoteConflict {
    pub promoting_node: NodeId,
    pub vcluster_id: VClusterId,
    pub conflicts: Vec<PromoteConflictEntry>,
}

pub struct PromoteConflictEntry {
    pub key: String,
    pub promoted_value: String,         // value being promoted
    pub conflicting_node: NodeId,       // node with local change
    pub conflicting_value: String,      // that node's local value
}
```

**Producer:** CLI promote workflow
**Consumer:** Journal (validates), CLI (blocks for admin resolution)
**Invariant:** CR4 (promote requires acknowledgment)

---

## CapabilityReport (Node → Scheduler)

```rust
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
```

Sub-schemas defined in `data-models/shared-kernel.md`.

---

## Loki Event Schema (JSON)

All events streamed to Loki follow this envelope:

```json
{
  "timestamp": "2026-03-12T10:30:00Z",
  "component": "journal|agent|policy",
  "node_id": "node-001",
  "vcluster_id": "ml-training",
  "event_type": "config_commit|emergency_start|drift_detected|...",
  "severity": "info|warn|error",
  "sequence": 42,
  "identity": {
    "principal": "alice@example.com",
    "role": "pact-ops-ml-training"
  },
  "detail": { ... }
}
```

The `detail` object is event-type-specific — it carries the relevant ConfigEntry fields, AdminOperation fields, or alert-specific data.

---

## Proto Wire Format

Events cross the wire as protobuf messages. Key mappings:

| Rust Type | Proto Message | Proto File |
|-----------|--------------|------------|
| ConfigEntry | pact.config.ConfigEntry | config.proto |
| StateDelta | pact.config.StateDelta | config.proto |
| Scope | pact.config.Scope | config.proto |
| CapabilityReport | pact.capability.CapabilityReport | capability.proto |
| PolicyEvalRequest | pact.policy.PolicyEvalRequest | policy.proto |
| ConfigChunk | pact.stream.ConfigChunk | stream.proto |
| ExecRequest/Output | pact.shell.ExecRequest/ExecOutput | shell.proto |
| ShellInput/Output | pact.shell.ShellInput/ShellOutput | shell.proto |

See `proto/pact/` for canonical definitions. Rust bindings generated by `tonic-prost-build` in `pact-common/build.rs`.
