# Pact System Invariants

Constraints that must ALWAYS hold. Violation of any invariant is a bug. Organized by bounded context.

---

## Journal Invariants

### J1: Monotonic sequence numbers
EntrySeq values are strictly increasing with no gaps. If entry N exists, entries 0..N all exist.

### J2: Immutability
Once a ConfigEntry is committed through Raft, it is never modified or deleted. The journal is append-only.

### J3: Authenticated authorship
Every ConfigEntry, AdminOperation, and PendingApproval has a non-empty Identity with a valid principal and role. No anonymous operations.

### J4: Acyclic parent chain
If ConfigEntry.parent = Some(p), then p < entry.sequence. No cycles.

### J5: Overlay consistency
BootOverlay.checksum matches a deterministic hash of BootOverlay.data. Overlay version corresponds to the latest config sequence for that vCluster.

### J6: Single policy per vCluster
At any point in time, at most one VClusterPolicy exists per VClusterId in JournalState.policies.

### J7: Raft consensus for writes
All state mutations (AppendEntry, UpdateNodeState, SetPolicy, SetOverlay, RecordOperation, AssignNode) go through Raft consensus. No direct state mutation.

### J8: Reads from local state
Boot config streaming, entry lookups, and overlay reads are served from local state machine replicas without Raft round-trips.

### J9: No duplicate entries from concurrent commits
Raft serializes all writes. Even if two clients submit simultaneously, each gets a unique sequence number.

---

## Agent Invariants

### A1: At most one commit window per node
A node has at most one active commit window at any time. New drift while a window is open extends/replaces the existing window.

### A2: At most one emergency session per node
Emergency mode is node-scoped. Cannot start a second emergency while one is active.

### A3: Commit window formula
`window_seconds = base_window / (1 + drift_magnitude * sensitivity)` where base_window > 0 and sensitivity >= 0. Result is always positive.

### A4: Auto-rollback on window expiry
When a commit window expires without commit, the system automatically rolls back to declared state. Exception: emergency mode suspends auto-rollback.

### A5: Active consumer check before rollback
Rollback must verify no active consumers hold resources (e.g., open handles on a mount). Rollback fails (does not proceed) if consumers exist.

### A6: Service dependency ordering
Services start in `order` field sequence with dependencies satisfied. Services shut down in reverse order.

### A7: Resource budget
Agent resource budget depends on workload state:
- **Active steady-state** (workloads running): RSS < 50 MB, CPU < 0.5%. Minimal overhead.
- **Idle steady-state** (no active allocations): RSS < 50 MB, CPU < 2%. Deeper inspections (eBPF signal checks, extended health checks) permitted.
- **During drift evaluation**: CPU < 2% (regardless of workload state).

### A8: Boot time target
From agent start to node ready: < 2 seconds (with warm journal).

### A9: Cached config during partition
When journal is unreachable, agent continues with cached configuration and cached policy. Operations are logged locally for replay on reconnect.

### A10: Emergency does not expand whitelist
Emergency mode suspends auto-rollback and extends the commit window. It does NOT add commands to exec/shell whitelist (ADR-004).

---

## Drift Invariants

### D1: Blacklist exclusion
Changes to paths matching blacklist patterns produce no drift. Default blacklist: /tmp/**, /var/log/**, /proc/**, /sys/**, /dev/**, /run/user/**.

### D2: Seven dimensions
Drift is tracked in exactly 7 dimensions: mounts, files, network, services, kernel, packages, gpu.

### D3: Non-negative magnitudes
Each dimension of DriftVector is >= 0. Total magnitude (weighted norm) is >= 0.

### D4: Weight influence
DriftWeights modify magnitude computation. Default: kernel=2.0, gpu=2.0, others=1.0. Zero weight = dimension ignored.

### D5: Observe-only mode logs without enforcement
In observe-only mode (enforcement_mode="observe"), drift is detected and logged but does NOT open commit windows or trigger rollback.

---

## Policy Invariants

### P1: Every operation is authenticated
All gRPC requests carry an OIDC Bearer token. Unauthenticated requests are rejected.

### P2: Every operation is authorized
After authentication, RBAC and policy evaluation determine whether the operation is allowed. Unauthorized requests are rejected with a reason.

### P3: Role scoping
pact-ops-{vcluster} and pact-viewer-{vcluster} roles are scoped to a single vCluster. Operations on other vClusters are denied.

### P4: Two-person approval for regulated vClusters
When VClusterPolicy.two_person_approval = true, state-changing operations require approval from a second admin. The requester cannot approve their own request.

### P5: Approval timeout
PendingApproval requests expire after a configurable timeout (default 30 minutes). Expired requests are rejected.

### P6: Platform admin always authorized
pact-platform-admin role is authorized for all operations. All actions are still logged.

### P7: Degraded mode restrictions
When PolicyService is unreachable: cached whitelist checks honored, two-person approval denied (not deferred), complex OPA rules denied, platform admin authorized with cached role.

### P8: AI agent emergency restriction
pact-service-ai principal cannot enter or exit emergency mode. Emergency requires human admin.

---

## Process Supervision Invariants

### PS1: Adaptive supervision loop interval
The supervision loop adjusts its poll interval based on node workload state:
- **Idle** (no processes in workload.slice): faster polling (e.g., 500ms), deeper inspections (eBPF signals, extended health checks)
- **Active** (any process exists in workload.slice): slower polling (e.g., 2-5s), minimal overhead to avoid disturbing workloads
- Transition trigger: presence/absence of processes in workload.slice (independent of lattice — pact can detect this via cgroup). This works regardless of whether lattice is deployed.
- Default baseline: 1 second. Configurable. Exact thresholds are site-tunable.
The supervision loop tick is coupled to the watchdog pet (PB2): if the loop hangs, the watchdog is not petted.

### PS2: Watchdog and supervision loop are coupled
The hardware watchdog pet occurs as part of the supervision loop tick. A stuck supervision loop fails to pet the watchdog, triggering BMC reboot. This ensures that a hung supervisor (not just a crashed one) is detected and recovered.

### PS3: Cgroup scope cleanup kills all processes
When a supervised process dies, Resource Isolation kills all remaining processes in that CgroupScope (via cgroup.kill) before releasing the scope. No orphaned child processes. No grace period for children — immediate cleanup.

---

## Resource Isolation Invariants

### RI1: Exclusive slice ownership
Each cgroup slice subtree is owned by exactly one system. pact.slice/ is owned by pact, workload.slice/ is owned by lattice. No system writes to another system's slice except during declared emergency (RI3).

### RI2: Every supervised process has a cgroup scope
No process spawned by PactSupervisor runs outside a cgroup scope. The cgroup scope is created before process spawn and cleaned up after process death. Note: cgroup2 filesystem mount and top-level slice creation happen during Bootstrap Phase 1 (InitHardware). Resource Isolation creates per-service scopes only after the hierarchy exists. This ordering is implied by PB3 (strict boot phase ordering).

### RI3: Emergency override requires audit trail
pact-agent may freeze or kill processes in workload.slice/ (lattice's subtree) only during declared emergency mode, with OIDC-authenticated authorization and an AuditEvent emitted before the action.

### RI4: pact-agent OOM protection
pact-agent runs with OOMScoreAdj=-1000 (root cgroup or its own protected scope). It is the last process the kernel kills on OOM.

### RI5: CgroupHandle callback on failure
If cgroup creation fails or the spawned process fails to start, Resource Isolation receives a callback and cleans up the cgroup scope. No orphaned scopes.

### RI6: Shared read across slices
Both pact and lattice may read cgroup metrics (memory.current, cpu.stat, etc.) from any slice for monitoring and capability reporting. Read access does not require emergency mode.

---

## Identity Mapping Invariants

### IM1: UID assignment is stable within federation membership
Once an OIDC subject is assigned a UID via the journal, that assignment persists as long as the subject's organization is federated. On federation departure, all UidEntries for that org are GC'd and the org_index becomes reclaimable.

### IM2: Precursor ranges do not overlap
Each federated org gets a computed precursor range: `precursor = base + org_index * stride`. Applies identically to both UIDs and GIDs (same formula, same org_index, same stride; separate base_uid and base_gid). Ranges are non-overlapping by construction (sequential org_index assignment). Stride is configurable (default 10,000). org_index is Raft-committed on federation join.

### IM3: Sequential assignment within precursor range
On-demand UID assignment allocates sequentially within the org's precursor range (precursor to precursor + stride - 1). If the range is exhausted (stride users reached), assignment fails with an error and alert. No wrap-around, no hash-based assignment.

### IM4: Pre-provisioned mode rejects unknowns
When identity_mode = "pre-provisioned" (regulated vClusters), authentication of an OIDC subject with no UidEntry is rejected. No automatic assignment.

### IM5: NSS module is read-only
libnss_pact.so only reads from /run/pact/passwd.db and group.db. It never writes, never makes network calls, and never blocks on I/O beyond mmap.

### IM6: Identity mapping only active in PactSupervisor mode
When SupervisorBackend = Systemd, the NSS module is not loaded and .db files are not written. SSSD handles identity resolution.

### IM7: UidMap loaded before non-root service startup
Services declared with a non-root user in ServiceDecl require UidMap to be loaded and .db files written before they can start. This is guaranteed by boot phase ordering (Phase 3: LoadIdentity before Phase 5: StartServices) but must also hold for services added dynamically via overlay updates at runtime.

---

## Network Management Invariants

### NM1: Netlink only in PactSupervisor mode
pact-agent configures network interfaces via netlink only when SupervisorBackend = Pact. In systemd mode, network configuration is delegated to the existing network manager (wickedd/NetworkManager).

### NM2: Network before services
Network interfaces must be configured and up before any network-dependent service is started. This is enforced by the boot phase ordering (ConfigureNetwork before StartServices).

---

## Platform Bootstrap Invariants

### PB1: Hardware watchdog only as PID 1
WatchdogHandle is only opened when pact-agent is PID 1 on a BMC-equipped node. In systemd mode, pact does not manage the watchdog.

### PB2: Watchdog pet interval
pact-agent pets the hardware watchdog at least every T/2 seconds, where T is the watchdog timeout. Missing a pet triggers BMC reboot.

### PB3: Boot phase ordering is strict
Boot phases execute in order: InitHardware → ConfigureNetwork → LoadIdentity → PullOverlay → StartServices → Ready. No phase starts before the previous completes. A phase failure blocks all subsequent phases.

### PB4: Bootstrap identity is temporary
The bootstrap identity from OpenCHAMI is discarded once a SPIRE SVID is obtained. It is never stored persistently and never used after SVID acquisition.

### PB5: No hard SPIRE dependency
pact-agent functions without SPIRE. In standalone mode (no SPIRE), the bootstrap identity or journal-signed mTLS cert is used for the full session.

---

## Workload Integration Invariants

### WI1: Namespace handoff via unix socket only
Namespace FDs are passed from pact-agent to lattice-node-agent exclusively via the defined unix socket protocol. No other channel (shared memory, filesystem, gRPC) is used for FD passing.

### WI2: Mount refcount accuracy
MountRef refcount exactly equals the number of active allocations using that mount. Refcount reaching zero starts the cache hold timer. Refcount going negative is a bug.

### WI3: Lazy unmount with hold time
When a mount's refcount reaches zero, it is not immediately unmounted. A configurable hold time (default TBD) allows a subsequent allocation to reuse the mount without re-mounting. After hold time expires, unmount proceeds. Exception: emergency mode with `--force` overrides the hold timer and unmounts immediately (see RI3).

### WI4: Lattice standalone creates own hierarchy
When lattice-node-agent runs without pact (standalone mode), it creates cgroup slices and manages mounts using the same hpc-core conventions. The hpc-node contract is the single source of truth for naming/layout regardless of which system creates the hierarchy.

### WI5: NamespaceSet cleanup on cgroup empty
pact detects when all processes in an allocation's CgroupScope have exited (cgroup becomes empty) and automatically cleans up the NamespaceSet. This is resilient to lattice-node-agent crashes — pact does not depend on lattice signaling allocation completion. Coherence over coupling.

### WI6: Mount refcount reconstruction on agent restart
If pact-agent crashes and restarts, it reconstructs MountRef refcounts by scanning the kernel mount table and correlating with active allocations from journal state. Existing mounts are preserved — running allocations are not disrupted.

---

## Shell & Exec Invariants

### S1: Whitelist enforcement
Non-whitelisted commands are rejected (exec) or unavailable (shell). No exceptions except platform admin bypass (S2).

### S2: Platform admin whitelist bypass
Platform admins can execute non-whitelisted commands via exec. All such executions are logged.

### S3: Restricted bash environment
Shell sessions use rbash. Cannot change PATH, run absolute paths, or redirect output to files.

### S4: Session audit
Every exec command and every shell command (via PROMPT_COMMAND) is logged to the journal with the authenticated identity.

### S5: State-changing commands trigger commit windows
Exec of a state-changing command opens a commit window (same as any other drift source).

### S6: Shell does not pre-classify commands
Shell sessions do NOT parse or classify commands before execution. Drift observer detects actual state changes post-execution.

---

## Observability Invariants

### O1: No per-agent Prometheus scraping
Agents do NOT expose Prometheus metrics endpoints. Agent health flows through lattice-node-agent eBPF (ADR-005).

### O2: Journal metrics on port 9091
Journal metrics endpoint listens on port 9091, not 9090 (avoids Prometheus server default conflict).

### O3: Audit trail continuity
The audit log is never interrupted. Including during emergency mode, partition, and degraded operation.

---

## Federation Invariants

### F1: Config state is site-local
Configuration state, drift events, admin session logs, capability reports, and shell/exec logs NEVER leave the site.

### F2: Policy templates are federated
OPA/Rego policy templates are synced from Sovra. Local data is pushed to OPA separately and never leaves site.

### F3: Graceful federation failure
If Sovra is unreachable, the system continues with locally cached policy templates. No functionality is lost.

---

## Conflict Resolution Invariants

### CR1: Local changes fed back before journal sync
When a partitioned agent reconnects, it reports all unpromoted local drift to the journal BEFORE accepting the journal's current state stream.

### CR2: Merge conflict pauses agent convergence
If local changes conflict with journal state on the same config keys, the agent pauses convergence and flags a merge conflict. It does NOT silently overwrite local state.

### CR3: Grace period fallback to journal-wins
If a merge conflict is not resolved by an admin within the grace period (default: commit window duration), the system falls back to journal-wins. Overwritten local changes are logged for audit.

### CR4: Promote requires conflict acknowledgment
When promoting node-level changes to a vCluster overlay, if target nodes have local changes recorded in the journal on conflicting keys, the promoting admin must explicitly accept or overwrite each conflict.

### CR5: Admin notification on overwrite
If an admin's uncommitted or local changes are overwritten by a promote or grace period timeout, and the admin has an active CLI session, they are notified in that session.

### CR6: No cross-vCluster atomicity
Config commits are scoped to a single vCluster. Cross-vCluster atomic operations are not supported. Partial failures across multiple vCluster commits are handled operationally.

---

## Node Delta Invariants

### ND1: TTL minimum bound
Node delta TTL must be >= 15 minutes. Deltas with shorter TTL are rejected at commit time.

### ND2: TTL maximum bound
Node delta TTL must be <= 10 days. Deltas with longer TTL are rejected at commit time.

### ND3: vCluster homogeneity expectation
All nodes within a vCluster are expected to converge to the same overlay. Per-node deltas are temporary exceptions. The system warns when nodes within a vCluster have divergent configurations or when deltas exceed their TTL.

---

## Raft Invariants

### R1: Independent Raft groups
Pact journal and lattice quorum are always independent Raft groups, even in co-located mode. Separate consensus, state, ports, WAL.

### R2: Pact is incumbent
In co-located mode, pact journal quorum is running before lattice starts. Pact does not depend on lattice.

### R3: Quorum ports
Pact Raft port: 9444. Pact gRPC port: 9443. These are separate from lattice ports (9000/50051/8080).

---

## Enrollment & Certificate Invariants (ADR-008)

### E1: No connection without enrollment
A node cannot establish an mTLS connection to the journal without a matching enrollment record. The enrollment endpoint is the only unauthenticated gRPC endpoint.

### E2: Hardware identity uniqueness per domain
Within a single domain, each hardware identity (MAC + BMC serial) maps to at most one enrollment record. Duplicate hardware identities are rejected.

### E3: Multi-domain enrollment, single activation
A node may be enrolled in multiple domains but can be Active in at most one at a time. This is enforced by physics (single boot target), not by distributed locks.

### E4: CSR model — no private keys in journal
Agents generate their own keypair and submit a CSR to the journal. The journal signs CSRs locally using its intermediate CA key. No private key material is stored in Raft state, transmitted over the wire, or held by the journal. Compromise of a journal node does not expose agent private keys.

### E5: Certificate lifetime and renewal
Default certificate lifetime is 3 days. Renewal triggers at 2/3 of lifetime. Renewal is agent-driven: agent generates new keypair + CSR, journal signs locally.

### E6: Dual-channel rotation
Certificate rotation uses a passive channel built with the new cert, health-checked, then atomically swapped with the active channel. In-flight operations are not interrupted.

### E7: Enrollment state governs CSR signing
Only nodes in Registered or Inactive enrollment state have their CSR signed on boot. Active nodes are rejected with ALREADY_ACTIVE (prevents concurrent enrollment race). Revoked nodes are rejected with NODE_REVOKED.

### E8: vCluster assignment is independent of enrollment
vCluster assignment is a separate operation from enrollment. An enrolled node may have no vCluster (maintenance mode). Moving between vClusters does not affect the certificate.

### E9: Decommission revokes certificate
Decommissioning a node sets enrollment state to Revoked and adds cert serial to Raft-stored revocation registry. The node's mTLS connection is terminated.

### E10: Only platform-admin can enroll and decommission
Node enrollment and decommission operations require pact-platform-admin role. vCluster assignment may be performed by pact-ops-{vcluster} for their own vCluster.

---

## Authentication Invariants (hpc-auth crate)

### Auth1: No unauthenticated commands
No authenticated command executes without a valid, non-expired access token. Only `login`, `logout`, `version`, and `--help` are exempt.

### Auth2: Fail closed on cache corruption
Corrupted or unreadable token cache is rejected. The user must re-login. The system never attempts to use a token from a corrupt cache.

### Auth3: Concurrent refresh safety
Multiple processes may refresh the same token concurrently. This is safe because refresh is idempotent at the IdP. Last writer wins on the cache file.

### Auth4: Logout always clears local state
Logout always deletes the local cached tokens, regardless of whether the IdP revocation succeeds.

### Auth5: Cache file permissions
Token cache files must have 0600 permissions. In strict mode (PACT default): reject cache with wrong permissions. In lenient mode (Lattice default): warn, fix, proceed.

### Auth6: Per-server token isolation
Token cache is keyed by server URL. Tokens for different servers never collide or cross-contaminate.

### Auth7: Refresh tokens never logged
Refresh tokens and client secrets are never included in log output, error messages, or diagnostics.

### Auth8: Cascading flow fallback
The auth crate selects the best available OAuth2 flow: Auth Code + PKCE → Confidential Client → Device Code → Manual Paste. The cascade is driven by IdP discovery, not hardcoded.

---

## Authentication Invariants (PACT-specific consumer)

### PAuth1: Strict permission mode
PACT CLI uses strict permission mode. Token cache with wrong permissions is rejected, never auto-fixed.

### PAuth2: Emergency mode requires human identity
Emergency mode cannot be initiated by a service/AI principal. The token must have a human principal type. Enforced by RBAC (P8) and at the CLI auth layer.

### PAuth3: Auth discovery endpoint is public
The pact-journal auth discovery endpoint does not require authentication. It returns the IdP URL and public client ID.

### PAuth4: Break-glass is BMC console
When the IdP is down and tokens are expired, the break-glass path is BMC console access (out-of-band, via OpenCHAMI). PACT CLI does not provide its own break-glass authentication mechanism.

### PAuth5: Two-person approval requires distinct identities
Two-person approval validates that the approver's token identity differs from the requester's. Same-identity approval is rejected regardless of token freshness.
