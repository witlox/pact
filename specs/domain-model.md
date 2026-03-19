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

Each compute node runs pact-agent as its init system. The agent observes system state, detects drift, manages commit windows, and provides remote access.

Nodes must be enrolled in the pact domain before they can connect. Enrollment is managed by the journal's enrollment registry (ADR-008). vCluster assignment is independent of enrollment — an enrolled node with no vCluster is in maintenance mode.

Node management is decomposed into six sub-contexts, each with a **strategy pattern** providing a PactSupervisor (default) and SystemdBackend (compat) implementation. When running in systemd compat mode, pact delegates to native systemd mechanisms. When running as PID 1, pact manages everything directly.

**Aggregate Root: Agent (per-node singleton)**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `NodeEnrollment` | Domain membership record: hardware identity, enrollment state, pre-signed cert | Created by admin, persists until decommission |
| `HardwareIdentity` | MAC addresses, BMC serial, optional TPM attestation | Read from SMBIOS/DMI at boot |
| `CommitWindow` | Time-limited window after drift detection | Opens on drift, closes on commit/rollback/expiry |
| `EmergencySession` | Extended commit window with suspended rollback | Opened by admin, closed by commit/rollback/force-end |
| `ShellSession` | Interactive restricted bash session | Created on connect, destroyed on disconnect |
| `CapabilityReport` | Hardware capability snapshot | Rebuilt periodically and on change |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `ConfigState` | ObserveOnly/Committed/Drifted/Converging/Emergency |
| `EnrollmentState` | Registered/Active/Inactive/Revoked |
| `SupervisorBackend` | Pact (default) / Systemd (fallback) |
| `GpuHealth` | Healthy/Degraded/Failed |
| `GpuCapability` | Index, vendor, model, memory, health, PCI bus |
| `CpuCapability` | CPU hardware snapshot: architecture, core count, frequency, ISA features |
| `CpuArchitecture` | X86_64/Aarch64/Unknown |
| `MemoryCapability` | Total bytes, available bytes, memory type, NUMA topology, huge pages |
| `MemoryType` | DDR4/DDR5/HBM2e/HBM3/HBM3e/Unknown |
| `NumaNode` | Per-NUMA-node info: id, total bytes, CPU list |
| `HugePageInfo` | Huge page allocation: 2MB and 1GB totals and free counts |
| `NetworkInterface` | Per-interface: name, fabric, speed, state, MAC, IPv4 |
| `NetworkFabric` | Slingshot/Ethernet/Unknown |
| `InterfaceOperState` | Up/Down |
| `StorageCapability` | Node type, local disks, mounts |
| `StorageNodeType` | Diskless/LocalStorage |
| `LocalDisk` | Per-disk: device, model, capacity, disk type |
| `DiskType` | Nvme/Ssd/Hdd/Unknown |
| `MountInfo` | Per-mount: path, fs type, source, total bytes, available bytes |
| `FsType` | Nfs/Lustre/Ext4/Xfs/Tmpfs/Other |
| `SoftwareCapability` | Loaded modules, uenv image, service statuses |

**Hardware Detection Backends:**

CPU, Memory, Network, and Storage detection each use a backend trait following the GpuBackend pattern. Each has a Linux implementation (parsing `/proc` and `/sys`) and a Mock implementation (configurable for tests and macOS development). No feature flags are required — all use standard Linux interfaces. GPU detection remains feature-gated per vendor (NVIDIA/AMD).

#### 2a. Process Supervision

Lifecycle management of declared services. Owns the supervision loop, health checks, restart policies, and dependency ordering.

**Entities:**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `ServiceDecl` | Declaration of a service: binary, args, restart policy, dependencies, order, health check config | From boot overlay, persists until overlay changes |
| `ProcessState` | Runtime state of a supervised process: state, PID, child handle, restart count, last exit code | Created on start, destroyed on stop/crash cleanup |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `ServiceState` | Starting/Running/Stopping/Stopped/Failed/Restarting |
| `RestartPolicy` | Always/OnFailure/Never |
| `HealthCheckType` | Process (alive check) / HTTP (GET endpoint) / TCP (port connect) |
| `HealthCheckResult` | Healthy/unhealthy + detail message |

**Strategy:**
- **PactSupervisor**: Direct `tokio::process::Command` spawn. Adaptive background supervision loop: polls faster when idle (deeper inspections, eBPF signal checks), slower when workloads are active (minimize overhead). Coupled to hardware watchdog — loop tick pets `/dev/watchdog`. Graceful shutdown: SIGTERM → configurable grace period → SIGKILL.
- **SystemdBackend**: Generates systemd unit files from `ServiceDecl`. Delegates lifecycle and restart to systemd's native mechanisms. No pact supervision loop — systemd handles `Restart=`, `WatchdogSec=`.

**Relationship to Resource Isolation:** Process Supervision requests a `CgroupHandle` from Resource Isolation before spawning. On spawn failure, Resource Isolation receives a callback to clean up the cgroup. On process death, Supervision notifies Isolation, which kills all remaining processes in the scope (cgroup.kill) and releases it.

#### 2b. Resource Isolation

cgroup v2 hierarchy management, per-service resource limits, OOM containment, and namespace creation. Provides `CgroupHandle` to Process Supervision for process placement.

**Entities:**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `CgroupSlice` | A cgroup v2 slice in the hierarchy (e.g., `pact.slice/gpu.slice`) | Created at boot, persists until shutdown |
| `CgroupScope` | A cgroup v2 scope within a slice for a specific service | Created on service start, destroyed on service stop |
| `NamespaceSet` | Set of Linux namespaces (pid/net/mount) created for an allocation | Created on allocation request, destroyed on allocation release |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `ResourceLimits` | Memory max, CPU weight, IO limits for a cgroup scope |
| `CgroupHandle` | Reference to a created cgroup scope, passed to Process Supervision for process placement |
| `SliceOwnership` | Enum: Pact / Workload. Determines which system owns a slice subtree |
| `NamespaceFd` | File descriptor for a created namespace, passed via handoff protocol |

**cgroup hierarchy (shared contract with lattice via hpc-core `hpc-node`):**

```
/sys/fs/cgroup/
├── pact.slice/                    # Owned by pact — exclusive write
│   ├── infra.slice/               # chronyd, dbus-daemon, rasdaemon
│   ├── network.slice/             # cxi_rh instances
│   ├── gpu.slice/                 # nvidia-persistenced, nv-hostengine
│   └── audit.slice/               # auditd, audit-forwarder (regulated only)
├── workload.slice/                # Owned by lattice — exclusive write
│   └── (lattice creates sub-hierarchy per allocation)
└── pact-agent scope               # pact-agent itself, OOMScoreAdj=-1000
```

**Boundary rules:**
- Exclusive write: each owner writes only to its own slice subtree
- Shared read: both can read metrics from any slice (for monitoring/capability reporting)
- Emergency override: pact can freeze/kill processes in `workload.slice/` during declared emergency mode, with audit trail

**Strategy:**
- **PactSupervisor**: Direct cgroup v2 filesystem API. Creates slices/scopes, writes resource limits, manages namespace lifecycle via `unshare(2)`.
- **SystemdBackend**: Delegates to systemd slice/scope units. systemd manages cgroup hierarchy natively.

#### 2c. Identity Mapping

OIDC subject → POSIX UID/GID translation for NFS compatibility. Only active when pact is init (PactSupervisor mode) AND NFS storage is used. This is a bypass shim, not a core identity system.

**Entities:**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `UidMap` | Complete mapping table: OIDC subjects → POSIX entries | Loaded from journal at boot, updated via subscription |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `UidEntry` | Single mapping: OIDC subject, uid, gid, username, home, shell, org |
| `GroupEntry` | Group mapping: name, gid, member list (full supplementary group resolution) |
| `OrgIndex` | Sequential index assigned to a federated org on join (0=local, 1=first partner, ...). Raft-committed. Reclaimable on federation departure. |
| `Stride` | Max users per org in UID space. Configurable, default 10,000. Determines precursor spacing. |

**UID assignment models (configurable per vCluster):**
- **On-demand (default)**: Unknown OIDC subject authenticates → pact-policy checks IdP → assigns UID from org range → commits to journal → propagates to agents
- **Pre-provisioned (regulated)**: Admin pre-provisions all users. Unknown subjects rejected. Required for sensitive vClusters.

**Federation deconfliction via computed precursor ranges:**
- Each Sovra-federated org is assigned an `org_index` (sequential, Raft-committed on federation join)
- UID precursor = `base_uid + org_index * stride` (default: base_uid=10000, stride=10000)
- GID precursor = `base_gid + org_index * stride` (default: base_gid=10000, same stride)
- Same formula, same org_index, same stride for both UID and GID
- Stride is configurable per site (adjustable default, trades max-users-per-org vs max-orgs)
- UID assignment within precursor range is sequential (precursor to precursor + stride - 1)
- Collision impossible by construction (sequential org_index, non-overlapping ranges)
- On federation departure: GC all UidEntries for that org, org_index reclaimable
- Max orgs with default stride in usable UID space: ~429,000

**Implementation:**
- `pact-nss` crate (cdylib): NSS module using `libnss` 0.9.0 crate. Reads from `/run/pact/passwd.db` and `/run/pact/group.db` via mmap. Zero network calls, ~1μs per lookup.
- pact-agent writes .db files to tmpfs on boot and on journal subscription updates
- `/etc/nsswitch.conf`: `passwd: files pact` / `group: files pact`

**Strategy:**
- **PactSupervisor**: Active. NSS module loaded, .db files maintained.
- **SystemdBackend**: Inactive. SSSD handles identity resolution.

#### 2d. Network Management

Interface configuration and network setup. Replaces wickedd/NetworkManager when pact is init.

**Entities:**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `NetworkInterface` | A network interface with its configuration (address, MTU, routes) | Configured at boot, updated on overlay changes |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `NetlinkConfig` | IP addresses, routes, MTU, link state for an interface |
| `InterfaceState` | Up/Down/Configuring |

**Strategy:**
- **PactSupervisor**: Direct netlink API via `nix` crate. Configures interfaces, sets addresses/routes, manages link state.
- **SystemdBackend**: Delegates to wickedd or NetworkManager. pact does not touch network config.

#### 2e. Platform Bootstrap

Boot sequence orchestration, hardware watchdog, SPIRE integration, device coldplug, and boot readiness signaling. This context owns the transition from "kernel booted" to "node ready."

**Entities:**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `BootSequence` | Ordered phases of node initialization | Created at boot, completed when readiness signaled |
| `WatchdogHandle` | Handle to `/dev/watchdog` hardware timer | Opened at boot (PID 1 mode only), petted periodically |
| `BootstrapIdentity` | Temporary credential from OpenCHAMI provisioning for initial journal auth | Used until SPIRE SVID obtained, then discarded |
| `SpireSvid` | SPIFFE Verifiable Identity Document obtained from SPIRE agent | Obtained after SPIRE is reachable, replaces bootstrap identity |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `BootPhase` | InitHardware / ConfigureNetwork / LoadIdentity / PullOverlay / StartServices / Ready |
| `ReadinessSignal` | Signal emitted when node is fully initialized (file, socket, or gRPC) |

**Boot sequence (pact as init):**

```
Phase 1: InitHardware     — coldplug device setup, kernel module loading
Phase 2: ConfigureNetwork — netlink interface config (delegates to Network Management)
Phase 3: LoadIdentity     — bootstrap identity → journal auth → UidMap (delegates to Identity Mapping)
Phase 4: PullOverlay      — vCluster overlay + node delta from journal
Phase 5: StartServices    — dependency-ordered service startup (delegates to Process Supervision)
Phase 6: Ready            — readiness signal emitted, capability report sent
```

**SPIRE bootstrap model:**
1. pact-agent starts with bootstrap identity from OpenCHAMI (in SquashFS image or kernel cmdline)
2. Bootstrap identity sufficient for initial journal authentication and overlay pull
3. SPIRE agent becomes reachable (started by pact or pre-existing)
4. pact-agent obtains SPIRE SVID and rotates to SPIRE-managed mTLS
5. No hard dependency on SPIRE — works without it (standalone mode)

**Strategy:**
- **PactSupervisor**: PID 1. Hardware watchdog (`/dev/watchdog`) petting. Full boot orchestration. Coldplug device setup.
- **SystemdBackend**: Regular systemd service. No watchdog (systemd handles). No boot orchestration (systemd handles). pact starts after systemd brings up the system.

#### 2f. Workload Integration (hpc-core shared kernel)

The integration protocol between pact-agent and lattice-node-agent for namespace handoff, mount sharing, and boot readiness. Defined in hpc-core (`hpc-node` crate) as a shared contract. Both systems implement provider and consumer sides independently.

**Entities:**

| Entity | Description | Lifecycle |
|--------|-------------|-----------|
| `AllocationEnvironment` | Prepared environment for a lattice allocation: namespace FDs, mount points, cgroup scope | Created on allocation request, destroyed on release |

**Value Objects:**

| Value Object | Description |
|--------------|-------------|
| `NamespaceHandoff` | Message containing namespace FDs (pid, net, mount) for an allocation |
| `MountRef` | Reference-counted mount point. Multiple allocations can share one uenv mount. |
| `ReadinessGate` | Signal that the node environment is ready for allocations |

**Communication channel:** Unix socket between pact-agent and lattice-node-agent.

**Mount refcounting:**
- pact-agent mounts uenv SquashFS images once per unique image
- Each allocation gets a bind-mount into its mount namespace
- `MountRef` tracks active consumers per image
- Lazy unmount when refcount reaches zero (configurable hold time for cache locality)

**Contract (hpc-core `hpc-node` crate):**
- Slice naming convention (constants/enums for `pact.slice/`, `workload.slice/`)
- Namespace FD passing protocol over unix socket
- Mount point conventions (base paths for uenv, working directories, data staging)
- Readiness signal protocol (how lattice knows "pact has prepared the environment")
- When lattice runs standalone (no pact): lattice creates its own cgroup hierarchy and mounts using the same hpc-core conventions

### Cross-cutting: Audit

Audit is not a bounded context — it is a cross-cutting concern. All bounded contexts emit `AuditEvent`s through a shared `AuditSink` trait. Each context is a publisher; none owns the destination.

**Shared types (hpc-core `hpc-audit` crate):**

| Type | Description |
|------|-------------|
| `AuditEvent` | Common event: who (principal), what (action), when (timestamp), where (node/vCluster), outcome (success/failure), detail |
| `AuditSink` trait | Destination interface: `fn emit(&self, event: AuditEvent)`. Implementations: journal append, file write, SIEM forward, Loki push |
| `CompliancePolicy` | Retention rules (e.g., 7-year for sensitive), required audit points |

**Shared types (hpc-core `hpc-identity` crate):**

| Type | Description |
|------|-------------|
| `IdentityProvider` trait | Obtain workload identity: `async fn get_identity() -> Result<WorkloadIdentity>`. Implementations: SpireProvider, SelfSignedProvider, StaticProvider |
| `CertRotator` trait | Certificate rotation: `async fn rotate(&self, new_identity: WorkloadIdentity) -> Result<()>`. Default: dual-channel swap pattern. |
| `WorkloadIdentity` | Cert chain + private key + trust bundle. Source-agnostic (SPIRE SVID or self-signed). |
| `IdentitySource` | Enum: Spire / SelfSigned / Bootstrap. Tracks provenance for audit. |

Both pact and lattice use `IdentityProvider` for mTLS. SPIRE is the primary provider when deployed. ADR-008's self-signed model is the fallback. Bootstrap identity is the initial provider on first boot.

**Design: loose coupling, high coherence.**
- Pact and lattice each maintain their own audit log (pact → journal, lattice → quorum)
- Both emit events in the same `AuditEvent` format
- A shared `AuditForwarder` implementation consumes from either and forwards to external SIEM
- No runtime dependency between them — either works alone

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

**Diagnostic Queries:**

| Value Object | Description |
|--------------|-------------|
| `DiagQuery` | Request parameters: node_id (or vcluster_id for fleet), source_filter (system/service/all), service_name, grep_pattern, line_limit |
| `DiagResult` | Response per source per node: node_id, source, lines (Vec<String>), truncated (bool) |
| `DiagSource` | Enum: Dmesg, Syslog, Journalctl, ServiceLog(name). Determines which local log source the agent reads. |

DiagQuery and DiagResult are wire-only types (proto messages on ShellService). They are not domain entities — no persistent state. The CLI constructs a DiagQuery, the agent collects and returns DiagResults.

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
JournalState ──1:1──▶ UidMap (OIDC → POSIX mapping table)
JournalState ──1:N──▶ OrgIndex (per federated org, sequential)

VClusterPolicy ──1:N──▶ RoleBinding
VClusterPolicy ──────▶ exec_whitelist, shell_whitelist

Agent ──1:1──▶ NodeEnrollment (domain membership)
Agent ──0:1──▶ vCluster assignment (None = maintenance mode)
Agent ──0:1──▶ CommitWindow (at most one active)
Agent ──0:1──▶ EmergencySession (at most one active)
Agent ──0:N──▶ ShellSession (concurrent sessions)
Agent ──1:1──▶ CapabilityReport (latest snapshot)

Process Supervision:
Agent ──1:N──▶ ServiceDecl (from boot overlay)
Agent ──1:N──▶ ProcessState (per running service)

Resource Isolation:
Agent ──1:N──▶ CgroupSlice (hierarchy created at boot)
Agent ──1:N──▶ CgroupScope (per service, created on start)
Agent ──0:N──▶ NamespaceSet (per allocation, created on request)

Identity Mapping:
Agent ──0:1──▶ UidMap (cached from journal, only in PactSupervisor mode)

Network Management:
Agent ──1:N──▶ NetworkInterface (configured at boot)

Platform Bootstrap:
Agent ──1:1──▶ BootSequence (boot phases)
Agent ──0:1──▶ WatchdogHandle (PID 1 mode only)
Agent ──1:1──▶ BootstrapIdentity (temporary, replaced by SpireSvid)
Agent ──0:1──▶ SpireSvid (obtained from SPIRE when available)

Workload Integration:
Agent ──0:N──▶ AllocationEnvironment (per active allocation)
Agent ──0:N──▶ MountRef (per unique uenv image, refcounted)

Cross-context relationships:
ProcessState ──1:1──▶ CgroupScope (every process placed in a cgroup)
ServiceDecl ──1:1──▶ ResourceLimits (resource config for the cgroup)
NamespaceSet ──1:1──▶ AllocationEnvironment (namespaces are part of allocation env)
AllocationEnvironment ──0:N──▶ MountRef (allocations reference shared mounts)

ConfigEntry ──0:1──▶ StateDelta (optional change payload)
ConfigEntry ──0:1──▶ parent ConfigEntry (chain)
ConfigEntry ──1:1──▶ Identity (author)
ConfigEntry ──1:1──▶ Scope

CapabilityReport ──1:1──▶ CpuCapability
CapabilityReport ──1:N──▶ GpuCapability
CapabilityReport ──1:1──▶ MemoryCapability
CapabilityReport ──0:N──▶ NetworkInterface
CapabilityReport ──1:1──▶ StorageCapability
CapabilityReport ──1:1──▶ SoftwareCapability
```

---

## Context Relationships

```
Bootstrap ──(customer-supplier)──▶ Process Supervision
Bootstrap ──(customer-supplier)──▶ Identity Mapping
Bootstrap ──(customer-supplier)──▶ Network Management

Process Supervision ──(customer-supplier)──▶ Resource Isolation
    Supervision requests CgroupHandle before spawn.
    Isolation returns handle via callback.
    On failure, Isolation cleans up cgroup.
    On process death, Supervision notifies Isolation to release.

Process Supervision ──(conformist)──▶ Identity Mapping
    Supervision needs UID resolved for services running as specific user.
    Does not influence how identity mapping works.

Process Supervision ──(temporal dependency)──▶ Network Management
    Some services need network up first (boot ordering).

All contexts ──(published language)──▶ Audit (cross-cutting)
    Shared AuditEvent type, each context is a publisher.

Workload Integration ──(shared kernel, hpc-core)──▶ Resource Isolation
    Namespace creation delegated to Isolation.
    Handoff protocol defined in hpc-core.

Workload Integration ──(shared kernel, hpc-core)──▶ Process Supervision
    lattice-node-agent is a supervised service.
    Receives namespace FDs via unix socket.
```

---

## Aggregate Boundaries

| Aggregate | Consistency | Storage |
|-----------|-------------|---------|
| JournalState | Strong (Raft) | WAL + snapshots on journal nodes |
| Agent state | Local (single node) | In-memory, cached config from journal |
| PolicyEngine | Strong (hosted in journal) | Part of JournalState (policies map) |
| UidMap | Strong (Raft-committed assignments) | Part of JournalState, cached on agents as tmpfs .db files |
| CapabilityReport | Eventually consistent | tmpfs manifest + journal record |
| CgroupHierarchy | Local (single node) | cgroup v2 filesystem |
| NamespaceSet | Local (single node) | Kernel namespace FDs |
| MountRef | Local (single node) | In-memory refcount + kernel mount table |

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

### BootPhase (per node)
```
InitHardware ──done──▶ ConfigureNetwork
ConfigureNetwork ──done──▶ LoadIdentity
LoadIdentity ──done──▶ PullOverlay
PullOverlay ──done──▶ StartServices
StartServices ──all started──▶ Ready
Any phase ──fatal error──▶ BootFailed
BootFailed ──retry──▶ (re-enter failed phase)
```

### CgroupScope (per service)
```
Creating ──success──▶ Active
Creating ──fail──▶ Failed (callback to Supervision)
Active ──process death──▶ Releasing
Active ──emergency kill──▶ Releasing
Releasing ──cleanup done──▶ Released
```

### MountRef (per unique uenv image)
```
Unmounted ──first allocation──▶ Mounting
Mounting ──success──▶ Mounted(refcount=1)
Mounted(n) ──new allocation──▶ Mounted(n+1)
Mounted(n) ──allocation release──▶ Mounted(n-1)
Mounted(1) ──allocation release──▶ HoldForCache(timer)
HoldForCache ──timer expires──▶ Unmounting
HoldForCache ──new allocation──▶ Mounted(1)
Unmounting ──done──▶ Unmounted
```

### InterfaceState (per network interface)
```
Down ──configure──▶ Configuring
Configuring ──success──▶ Up
Configuring ──fail──▶ Down
Up ──link lost──▶ Down
Up ──reconfigure──▶ Configuring
```
