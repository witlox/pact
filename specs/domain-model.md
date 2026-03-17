# Pact Domain Model

## Bounded Contexts

### 1. Configuration Management (Journal)

The journal is the single source of truth for declared configuration state. All mutations go through Raft consensus. Reads served from local state machine replicas.

**Aggregate Root: JournalState**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `ConfigEntry` | Immutable log entry recording a config change | Created via Raft, never modified |
| `NodeState` | Per-node current config state (ObserveOnly/Committed/Drifted/Converging/Emergency) | Updated via Raft on state transitions |
| `VClusterPolicy` | Per-vCluster policy governing drift, windows, approvals, whitelists | Set/replaced via Raft |
| `BootOverlay` | Pre-computed compressed config bundle for a vCluster | Rebuilt on commit or on-demand |
| `AdminOperation` | Audit record of an admin action (exec, shell, service, emergency) | Appended via Raft, never modified |
| `PendingApproval` | Two-person approval request awaiting second admin | Created on policy requirement, resolved or expired |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `EntrySeq` | Monotonically increasing sequence number (u64) |
| `Scope` | Global, VCluster(id), or Node(id) |
| `Identity` | Principal + type (Human/Agent/Service) + role |
| `StateDelta` | Changes across 7 dimensions (mounts, files, network, services, kernel, packages, gpu) |
| `DriftVector` | Magnitude per dimension (f64 each) |
| `DriftWeights` | Per-dimension weights for magnitude computation |

**Invariants:**
- EntrySeq is monotonically increasing, no gaps
- Every ConfigEntry has an authenticated Identity (author)
- Parent chain is acyclic (parent < sequence)
- Overlay checksums match content

### 2. Node Management (Agent)

Each compute node runs pact-agent as its init system. The agent observes system state, detects drift, manages commit windows, supervises services, and provides remote access.

Nodes must be enrolled in the pact domain before they can connect. Enrollment is managed by the journal's enrollment registry (ADR-008). vCluster assignment is independent of enrollment — an enrolled node with no vCluster is in maintenance mode.

**Aggregate Root: Agent (per-node singleton)**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `NodeEnrollment` | Domain membership record: hardware identity, enrollment state, pre-signed cert | Created by admin, persists until decommission |
| `HardwareIdentity` | MAC addresses, BMC serial, optional TPM attestation | Read from SMBIOS/DMI at boot |
| `ServiceDecl` | Declaration of a service to supervise | From boot config, persists until overlay changes |
| `ServiceInstance` | Running instance of a declared service | Started/stopped/restarted by supervisor |
| `CommitWindow` | Time-limited window after drift detection | Opens on drift, closes on commit/rollback/expiry |
| `EmergencySession` | Extended commit window with suspended rollback | Opened by admin, closed by commit/rollback/force-end |
| `ShellSession` | Interactive restricted bash session | Created on connect, destroyed on disconnect |
| `CapabilityReport` | Hardware capability snapshot | Rebuilt periodically and on change |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `ServiceState` | Starting/Running/Stopping/Stopped/Failed/Restarting |
| `ConfigState` | ObserveOnly/Committed/Drifted/Converging/Emergency |
| `RestartPolicy` | Always/OnFailure/Never |
| `EnrollmentState` | Registered/Active/Inactive/Revoked |
| `SupervisorBackend` | Pact (default) / Systemd (fallback) |
| `GpuHealth` | Healthy/Degraded/Failed |
| `GpuCapability` | Index, vendor, model, memory, health, PCI bus |
| `MemoryCapability` | Total bytes, available bytes, NUMA nodes |
| `NetworkCapability` | Fabric type, bandwidth, latency |
| `StorageCapability` | tmpfs size, mount points |
| `SoftwareCapability` | Loaded modules, uenv image, service statuses |

### 3. Policy & Authorization

Policy evaluation runs as a library crate inside the journal process (ADR-003). Agents cache policy for partition resilience.

**Aggregate Root: PolicyEngine**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `VClusterPolicy` | Effective policy for a vCluster (17 fields) | Set via journal Raft, cached by agents |
| `RoleBinding` | Maps role to principals and allowed actions | Part of VClusterPolicy |
| `ApprovalRequest` | Pending two-person approval | Created on policy trigger, resolved/expired |

**Invariants:**
- Every operation evaluated against policy before execution
- Degraded mode (PolicyService unreachable): cached policy only, two-person denied, complex rules denied
- Platform admin always authorized (logged)

### 4. Admin Operations (CLI)

The CLI connects to journal (config commands) and agent (exec/shell commands) via gRPC.

**No persistent entities** — the CLI is stateless. All state lives in journal or agent.

### 5. Federation (Optional)

Config state is site-local. Policy templates are federated via Sovra.

**Entities:**

| Entity | Description |
|--------|-------------|
| `RegoTemplate` | OPA policy template synced from Sovra |
| `ComplianceReport` | Drift/audit summary for cross-site reporting |

**Hard boundary:** Config state, drift events, shell logs, capability reports NEVER leave site.

---

## Entity Relationships

```
JournalState ──1:N──▶ ConfigEntry
JournalState ──1:N──▶ NodeState (per node)
JournalState ──1:N──▶ VClusterPolicy (per vCluster)
JournalState ──1:N──▶ BootOverlay (per vCluster)
JournalState ──1:N──▶ AdminOperation (audit log)
JournalState ──1:N──▶ NodeEnrollment (domain membership + cert)
JournalState ──1:N──▶ NodeAssignment (node → vCluster, optional)

VClusterPolicy ──1:N──▶ RoleBinding
VClusterPolicy ──────▶ exec_whitelist, shell_whitelist

Agent ──1:1──▶ NodeEnrollment (domain membership)
Agent ──0:1──▶ vCluster assignment (None = maintenance mode)
Agent ──1:N──▶ ServiceDecl (from boot config)
Agent ──1:N──▶ ServiceInstance (running processes)
Agent ──0:1──▶ CommitWindow (at most one active)
Agent ──0:1──▶ EmergencySession (at most one active)
Agent ──0:N──▶ ShellSession (concurrent sessions)
Agent ──1:1──▶ CapabilityReport (latest snapshot)

ConfigEntry ──0:1──▶ StateDelta (optional change payload)
ConfigEntry ──0:1──▶ parent ConfigEntry (chain)
ConfigEntry ──1:1──▶ Identity (author)
ConfigEntry ──1:1──▶ Scope

CapabilityReport ──1:N──▶ GpuCapability
CapabilityReport ──1:1──▶ MemoryCapability
CapabilityReport ──0:1──▶ NetworkCapability
CapabilityReport ──1:1──▶ StorageCapability
CapabilityReport ──1:1──▶ SoftwareCapability
```

---

## Aggregate Boundaries

| Aggregate | Consistency | Storage |
|-----------|-------------|---------|
| JournalState | Strong (Raft) | WAL + snapshots on journal nodes |
| Agent state | Local (single node) | In-memory, cached config from journal |
| PolicyEngine | Strong (hosted in journal) | Part of JournalState (policies map) |
| CapabilityReport | Eventually consistent | tmpfs manifest + journal record |

---

## State Machines

### EnrollmentState (per node, per domain)
```
Registered ──boot(hw match)──▶ Active
Active ──heartbeat timeout──▶ Inactive
Inactive ──boot(hw match)──▶ Active
Registered ──decommission──▶ Revoked
Active ──decommission──▶ Revoked
Inactive ──decommission──▶ Revoked
```

### ConfigState (per node)
```
ObserveOnly ──enforce──▶ Committed
Committed ──drift──▶ Drifted
Drifted ──commit──▶ Committed
Drifted ──rollback──▶ Committed
Drifted ──emergency──▶ Emergency
Emergency ──commit──▶ Committed
Emergency ──rollback──▶ Committed
Emergency ──force-end──▶ Committed
```

### ServiceState (per service)
```
Starting ──success──▶ Running
Starting ──fail──▶ Failed
Running ──stop──▶ Stopping
Running ──crash──▶ Failed
Stopping ──done──▶ Stopped
Failed ──restart(Always|OnFailure)──▶ Restarting
Failed ──restart(Never)──▶ Stopped
Restarting ──success──▶ Running
Restarting ──fail──▶ Failed
```

### ApprovalStatus
```
Pending ──approve──▶ Approved
Pending ──deny──▶ Rejected
Pending ──timeout──▶ Expired
```
