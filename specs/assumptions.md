# Pact Assumptions

Assumptions underlying pact's design. Each is classified as **Validated** (confirmed true), **Accepted** (team decision, not externally verified), or **Unknown** (needs investigation).

---

## Infrastructure Assumptions

### A-I1: Diskless compute nodes [Validated]
Compute nodes boot from SquashFS images provisioned by OpenCHAMI. No persistent local storage. All persistent state lives in the journal.

### A-I2: mTLS certificates managed by pact via CSR + journal CA [Accepted — supersedes original A-I2]
Certificate lifecycle is pact's responsibility (ADR-008). Journal generates an ephemeral CA at startup (or loads from disk). Agents generate their own keypairs at boot and submit CSRs to the journal, which signs them locally. When SPIRE is deployed, SPIRE is the primary identity provider and journal CA signing is only used as fallback.

### A-I3: 3-5 journal nodes available [Accepted]
Raft quorum requires 3 (tolerates 1 failure) or 5 (tolerates 2 failures) nodes. These are either dedicated or co-located with lattice management nodes.

### A-I4: Management network available at boot [Accepted — updated for ADR-017]
pact-agent can reach journal nodes via the management network (1G Ethernet) at boot time. The management network is always available (PXE boot network). If management net is unavailable, agent falls back to cached config (degraded boot). The high-speed network (Slingshot/HSN) is NOT available during early boot — it comes up after pact starts `cxi_rh` (a supervised service). All pact traffic runs on management net. Lattice traffic runs on HSN.

### A-I5: cgroup v2 filesystem available [Validated]
PactSupervisor uses cgroup v2 for service isolation. Modern kernels (5.x+) have cgroup v2 by default. SquashFS images include cgroup2 mount.

### A-I6: 5-9 services per node [Validated — updated from real compute node analysis]
Diskless HPC nodes run 5-9 services depending on vCluster type. Derived from real `ps aux` analysis of HPE Cray EX compute nodes:
- **ML training (GPU)**: 7 services (chronyd, dbus, cxi_rh x4, nvidia-persistenced, nv-hostengine, rasdaemon, lattice-node-agent). Plus rpcbind/rpc.statd if NFS.
- **Regulated/sensitive**: +2 (auditd, audit-forwarder) = 9 services.
- **Dev sandbox**: 5 services (chronyd, dbus, cxi_rh, rasdaemon, lattice-node-agent).
This is far fewer than general-purpose servers, making PactSupervisor viable over systemd.

### A-I7: SPIRE pre-existing in infrastructure [Validated]
HPE Cray infrastructure uses SPIRE (SPIFFE Runtime Environment) for mTLS workload attestation. spire-agent runs on compute nodes. pact-agent integrates with SPIRE to obtain SVIDs rather than managing its own certificate lifecycle end-to-end. ADR-008 bootstrap identity is used until SPIRE is reachable.

### A-I8: Hardware watchdog available on BMC-equipped nodes [Accepted]
Nodes where pact-agent runs as PID 1 have hardware watchdog support (`/dev/watchdog`) backed by IPMI/BMC. If no hardware watchdog is available, the node runs in systemd mode (pact is a regular service, not PID 1).

### A-I9: cxi_rh instances match NIC count [Validated]
CXI resource handler runs one instance per Slingshot NIC device. Real compute nodes show 4 instances (cxi0-cxi3). Count is hardware-dependent and discovered at boot.

### A-I10: atomd and nomad replaced by pact [Accepted]
HPE ATOM (atomd) and HashiCorp Nomad are the current task orchestration layers on compute nodes. Both are fully replaced by pact-agent's process supervision. The 17+ Nomad executors currently deploying system services (slurmd, munge, pyxis, enroot, podman, uenv, storage, IAM, CDI, skybox, etc.) are replaced by declarative ServiceDecl entries in the vCluster overlay.

### A-I11: DVS is legacy [Accepted]
Cray DVS (Data Virtualization Service) is legacy. DVS-IPC_msg processes on compute nodes are not needed with modern storage (VAST NFS/S3). Not managed by pact.

---

## Scale Assumptions

### A-S1: 10,000+ nodes [Accepted]
Boot config streaming must handle 10,000+ concurrent boot requests. Overlays are ~100-200 KB, deltas < 1 KB.

### A-S2: 3-5 journal scrape targets [Validated]
Prometheus scrapes only journal servers (3-5), not individual agents. Agent health flows through lattice eBPF.

### A-S3: Overlay size ~100-200 KB [Accepted]
vCluster overlays are small enough for sub-second streaming. If overlays grow beyond this, chunked streaming handles it.

### A-S4: Node delta < 1 KB [Accepted]
Per-node deltas are small configuration differences. If they grow significantly, the promote workflow converts them to overlay changes.

---

## Security Assumptions

### A-Sec1: OIDC provider available [Accepted]
An external OIDC provider (Keycloak, Azure AD, etc.) issues tokens. Pact validates tokens but does not run an IdP.

### A-Sec2: BMC console is out-of-band [Validated]
BMC/Redfish console access (via OpenCHAMI) is available when pact-agent is down. It provides unrestricted bash — changes are detected as unattributed drift when the agent recovers.

### A-Sec3: OPA sidecar managed externally [Accepted]
OPA runs as a sidecar process on journal nodes. Its lifecycle is managed by systemd, PactSupervisor, or a container orchestrator — not by pact-policy code directly.

### A-Sec4: Restricted bash is sufficient [Accepted]
rbash with PATH restriction, PROMPT_COMMAND audit, and optional mount namespace provides adequate security for admin access. Known shell escape vectors (vi, python, etc.) are excluded from default whitelist.

### A-Sec5: Whitelist maintenance is operational [Accepted]
The default whitelist covers common diagnostics. Per-vCluster additions are an operational concern. Learning mode helps identify needed additions.

### A-Sec6: IdP supports OIDC discovery [Accepted — with fallback]
The IdP exposes `/.well-known/openid-configuration`. The hpc-auth crate uses this to discover supported grant types and endpoints. If discovery is unavailable, the crate falls back to a cached discovery document, then to manual configuration. Stale cached documents are cleared on auth failure.

### A-Sec7: Public client registration possible [Accepted — with fallback]
Authorization Code + PKCE requires a public client (no client_secret). If the IdP only supports confidential clients, the crate falls back to embedded client_secret, then Device Code, then manual paste. See cascading fallback chain in auth_login.feature.

### A-Sec8: Device Code grant available [Accepted — with fallback]
Headless environments need Device Code flow. If the IdP doesn't support it, the crate falls back to manual paste (user copies authorization code from browser). Degraded UX but functional.

### A-Sec9: Refresh tokens issued by IdP [Accepted — with fallback]
Silent token refresh depends on the IdP issuing refresh tokens. If not issued, every access token expiry requires full re-authentication. Browser session may still be active, making re-auth a single click.

### A-Sec10: Token cache on user's local workstation [Validated]
Token cache is stored on the user's client machine (workstation/laptop), not on HPC nodes. No shared filesystem concerns. Cache is per-IdP-endpoint to support multiple deployments.

---

## Identity Mapping Assumptions

### A-Id1: NFS requires POSIX UID/GID [Validated]
NFS wire protocol uses numeric UID/GID for file ownership and access control. OIDC tokens do not natively carry POSIX attributes. A mapping layer is required when pact is init and NFS is used. S3 works natively with OIDC — no mapping needed.

### A-Id2: Identity mapping is an NFS bypass shim [Accepted]
The UidMap and NSS module exist solely because NFS cannot authenticate via OIDC. This is not a core identity system — it is a compatibility shim. When systemd mode is active (SSSD handles NSS) or when storage is pure S3, this subsystem is inactive.

### A-Id3: IdP has POSIX attributes or can be synced [Accepted — with fallback]
The IdP (Keycloak, AD) either has uidNumber/gidNumber in its LDAP backend, or pact-journal assigns UIDs from configured ranges. If the IdP has POSIX attributes, SCIM/LDAP sync populates the journal's UidMap. If not, on-demand assignment creates mappings.

### A-Id4: Full supplementary group resolution needed [Validated]
NFS access control uses all supplementary groups, not just primary GID. The NSS module must resolve full group membership (getgrouplist). GroupEntry includes member lists.

### A-Id5: Stride of 10,000 per org sufficient [Accepted — configurable default]
Default stride (10,000 UIDs per org) is sufficient for typical HPC sites. Stride is a site-wide configurable default, adjustable before federation starts. Exhaustion triggers alert; admin must increase stride (requires UID remapping if increased after assignments). With stride 10,000 and base_uid 10,000, max ~429,000 federated orgs in 32-bit UID space.

### A-Id6: UID assignments reclaimable on federation departure [Accepted]
When an org leaves federation, its UidEntries are GC'd from the journal and the org_index becomes reclaimable. NFS files owned by departed org's UIDs become orphaned (numeric UIDs, no name resolution). Site admin is responsible for archiving or re-owning those files before departure.

---

## Consistency Assumptions

### A-C1: AP model is acceptable [Accepted]
Eventual consistency with acknowledged drift is the right trade-off for HPC infrastructure. Silent convergence (Puppet/Ansible model) is explicitly rejected.

### A-C2: Timestamp ordering for conflict resolution [Accepted]
During partitions, conflicting changes are ordered by timestamp. Admin-committed changes take precedence over auto-converge. This is simple but may miss edge cases.

### A-C3: Cached policy is sufficient during partition [Accepted]
When PolicyService is unreachable, agents use cached VClusterPolicy for basic authorization. Two-person approval and complex OPA rules are denied (fail-closed for complex operations, fail-open for basic ones).

---

## Integration Assumptions

### A-Int1: Lattice Rust client exists [Validated]
Drain/cordon commands delegate to lattice via its Rust client library. This dependency is available.

### A-Int2: OpenCHAMI client status unknown [Unknown]
Reboot/re-image commands delegate to OpenCHAMI/Manta APIs. A Rust client for these APIs may not exist yet. Delegation commands are stubbed initially.

### A-Int3: Sovra is optional [Accepted]
Federation via Sovra is feature-gated. The system is fully functional without it.

### A-Int4: lattice-node-agent mediates capability delivery [Accepted]
pact writes CapabilityReport to tmpfs + unix socket. lattice-node-agent reads it and reports to scheduler. pact does NOT stream directly to lattice scheduler.

### A-Int5: hpc-core contracts are trait-based [Accepted]
hpc-core crates (`hpc-node`, `hpc-audit`) define traits and types, not implementations. pact and lattice each implement the traits independently. Neither system depends on the other at runtime — only on the shared contract.

### A-Int6: Lattice works independently of pact [Accepted]
lattice-node-agent can run without pact (standalone mode on systemd-managed nodes). In standalone mode, lattice creates its own cgroup hierarchy and manages its own mounts using hpc-core conventions. When pact is present, lattice gains capabilities (namespace pre-creation, mount refcounting, cgroup-atomic checkpointing) — "supercharged" mode.

### A-Int7: Unix socket for namespace handoff [Accepted]
Namespace FDs are passed from pact to lattice via a unix socket (SCM_RIGHTS). This is the standard Linux mechanism for FD passing between processes. The socket path is defined in hpc-core `hpc-node` conventions.

### A-Int8: libnss 0.9.0 crate is suitable [Accepted]
The `libnss` Rust crate (0.9.0, Feb 2025, LGPL-3.0) provides a stable API for writing NSS modules. Dynamic linking (cdylib) satisfies LGPL requirements. Dependencies are minimal (libc, lazy_static, paste).

---

## Development Assumptions

### A-D1: macOS development, Linux production [Validated]
Developers work on macOS. Production runs on Linux. Three-tier strategy: feature-gate, mocks, devcontainer.

### A-D2: Stable Rust toolchain [Validated]
No nightly features used. `imports_granularity` and `group_imports` are nightly-only in rustfmt — not available.

### A-D3: openraft 0.10.0-alpha.14 is stable enough [Accepted]
Pinned alpha version. Loose `^0.10` caused breakage in lattice. Version pinned explicitly.

---

## Resolved Questions

### A-Q1: Cross-vCluster atomic operations [Accepted — Not Required]
Cross-vCluster atomicity is not supported. Each vCluster is an independent consistency domain. Platform-admin issues separate commits per vCluster; partial failures are handled operationally. Adding cross-vCluster transactions would require coordinating multiple Raft entries, adding significant complexity for a rare operation.

### A-Q2: Partition conflict replay mechanism [Accepted — Correctness-First with Availability Fallback]
During network partition, agents may accumulate local state changes (admin reconfigurations via pact shell, emergency mode activations, drift from manual intervention). On partition reconnect:

1. **Agent feeds back local changes first** — unpromoted local drift is reported to the journal before accepting the journal's current state.
2. **Conflict detection** — if local changes conflict with journal state (same config keys changed on both sides), the agent pauses and flags a merge conflict for admin resolution. It does NOT silently overwrite.
3. **Grace period fallback** — if no admin resolves the conflict within a configurable grace period (default: commit window duration), the system falls back to journal-wins (availability fallback). The overwritten local changes are logged for audit.
4. **Admin notification** — if an admin's changes are overwritten (either by grace period timeout or by a concurrent promote), they are notified if they have an active CLI session.
5. **Promote workflow integration** — when promoting node-level changes to vCluster overlay, if the target nodes have local changes recorded in the journal, the promoting admin must explicitly acknowledge each conflicting key: accept (keep local) or overwrite (apply promoted value).

Key assumptions:
- vCluster nodes are homogeneous (see A-H1). Per-node deltas are temporary exceptions.
- Agents do not write config entries to the journal autonomously — only admins (via CLI/shell) create config mutations.
- "Last write wins" is the tiebreaker when timestamps differ and no admin is online to resolve.

### A-Q3: Node delta TTL bounds [Accepted — Bounded]
Node deltas have enforced TTL bounds: minimum 15 minutes, maximum 10 days. Rationale:
- **15 min minimum**: short enough for debugging sessions, long enough to not expire mid-task.
- **10 day maximum**: carries over weekends with margin, forces periodic review, prevents forgotten deltas from accumulating.
TTL outside these bounds is rejected at commit time with an error explaining the valid range.

### A-Q4: Snapshot retention during replication [Accepted — Application Responsibility]
openraft does NOT handle snapshot retention internally during replication. The framework delegates snapshot storage entirely to the application via `RaftSnapshotBuilder` and `RaftStorage` traits. If a new snapshot is created while an old one is being transferred to a follower, the application must ensure the old snapshot remains accessible.

Mitigation for pact: journal state is small (config entries, policies, overlays) — use in-memory snapshots (`Cursor<Vec<u8>>`) rather than file-based snapshots. This avoids the garbage collection problem entirely since each snapshot is an independent allocation held by the replication task until transfer completes. If file-based snapshots are needed later (e.g., for very large deployments), use Unix file descriptor semantics: keep the fd open during transfer so the file survives unlink.

---

## Process Supervision Assumptions

### A-PS1: Adaptive polling does not miss critical events [Accepted]
During active workloads (slower polling, 2-5s), a service crash is not detected for up to the poll interval. This is acceptable because: (a) the supervision loop tick is the detection mechanism, not a real-time signal, (b) the watchdog pet is coupled to the tick, so agent hang is detected separately, (c) for sub-second crash detection, eBPF tracepoints on process exit could supplement polling in a future iteration.

### A-PS2: cgroup.kill is reliable on supported kernels [Accepted]
cgroup.kill (Linux 5.14+) is the cleanup mechanism for child processes. On older kernels, fallback to iterating cgroup.procs and sending SIGKILL. F30 handles the edge case where kill fails (D-state processes). SquashFS images should include kernel 5.14+ for full support.

### A-PS3: Services do not require inter-service communication during startup [Accepted]
Services are started sequentially in dependency order. There is no mechanism for a service to signal "I'm ready" to its dependents beyond being in Running state (process alive). If a service needs warm-up time (e.g., nv-hostengine initializing GPU contexts), the dependent service must handle the dependency not being fully ready yet. Health checks (HTTP/TCP) can be used to gate readiness, but this is per-service configuration, not a supervisor feature.

---

## Resource Isolation Assumptions

### A-RI1: cgroup v2 unified hierarchy [Validated]
All target systems use cgroup v2 unified hierarchy (not hybrid v1/v2). This is a hard requirement — PactSupervisor does not support cgroup v1 fallback. SquashFS images must have CONFIG_CGROUP_V2 enabled and cgroup2 mounted.

### A-RI2: Kernel supports cgroup.kill [Accepted — with fallback]
cgroup.kill requires Linux 5.14+. If not available, fallback to SIGKILL iteration on cgroup.procs. Degraded but functional. See A-PS2.

### A-RI3: OOMScoreAdj=-1000 is honored [Validated]
Linux kernel honors OOMScoreAdj=-1000 for the init process. pact-agent as PID 1 will not be OOM-killed unless all other processes have already been killed.

---

## Network Management Assumptions

### A-NM1: Netlink sufficient for HPC network configuration [Accepted]
Standard netlink (RTM_NEWADDR, RTM_NEWROUTE, RTM_SETLINK) covers all needed network operations: IP assignment, routing, MTU, link state. CXI/Slingshot-specific configuration is handled by cxi_rh (a supervised service), not by pact's network management. pact only configures standard IP interfaces.

### A-NM2: Network configuration is deterministic from overlay [Accepted]
On diskless compute nodes, network configuration is fully determined by the vCluster overlay (no DHCP discovery needed). Interface names, addresses, routes, and MTU are all declared. This assumption fails if nodes need dynamic network discovery — but diskless HPC nodes have pre-assigned network identities from OpenCHAMI.

---

## Workload Integration Assumptions

### A-WI1: SCM_RIGHTS works for namespace FD passing [Validated]
Unix domain sockets with SCM_RIGHTS ancillary data is the standard Linux mechanism for passing file descriptors between processes. Kernel support is universal. The namespace FDs (from /proc/self/ns/*) are passable via this mechanism.

### A-WI2: Periodic reconciliation catches all leaks [Accepted]
Mount refcount and namespace reconciliation during idle supervision ticks is sufficient to prevent resource leaks from accumulating. The reconciliation interval is bounded by the adaptive polling rate (faster when idle). A node running continuous workloads without idle periods could theoretically accumulate leaks — but the cache hold timer bounds mount accumulation, and namespace cleanup is triggered by cgroup-empty events regardless of reconciliation.

### A-WI3: Hold timer default TBD [Unknown]
The mount cache hold time (WI3) default has not been determined. Needs benchmarking: too short = no caching benefit, too long = resource waste. Likely in the range of 30-300 seconds. Must be determined by measuring uenv mount latency vs. available memory on target hardware.

---

## Homogeneity Assumption

### A-H1: vCluster node homogeneity [Accepted]
All nodes within a vCluster converge to the same overlay configuration. Per-node deltas are temporary exceptions (bounded by TTL, see A-Q3) that should be either promoted to the vCluster overlay or reverted. The system:
- **Warns** when per-node deltas exist beyond their TTL or when nodes within a vCluster have divergent configurations.
- **Reports** heterogeneity in status/diff output so operators can see which nodes deviate.
- **Does not enforce** homogeneity automatically — it flags the condition for admin decision.
Rationale: HPC vClusters assume uniform node configuration for scheduling correctness. Heterogeneous nodes can cause job failures if the scheduler assumes capability uniformity.

---

## Hardware Detection Assumptions

### A-Hw4: GH200 unified memory visible via standard interfaces [Accepted]
GH200 unified memory appears as standard `/proc/meminfo` (~854 GB total) with NUMA topology visible in `/sys/devices/system/node/`. No vendor-specific parsing needed for memory detection. GPU memory is reported separately via NVML (GpuBackend).

### A-Hw5: MI300A unified HBM visible via standard interfaces [Accepted]
MI300A unified HBM appears similarly in standard Linux memory interfaces. ROCm SMI is needed only for GPU-specific queries (health, temperature, utilization), not for base memory detection. Memory type detection via `dmidecode --type 17` reports HBM correctly.

### A-Hw6: Slingshot NICs use cxi kernel driver [Validated]
Slingshot NICs use the `cxi` kernel driver. Interface names follow `cxi0`-`cxiN` pattern. Driver detection via `/sys/class/net/*/device/driver` symlink resolves to `cxi` for Slingshot interfaces. This is used by NetworkBackend to classify interfaces as Slingshot fabric.

### A-Hw7: Standard /proc and /sys sufficient for hardware detection [Accepted]
All hardware detection (CPU, memory, network, storage) uses standard `/proc` and `/sys` interfaces available on any Linux kernel 5.x+. No vendor libraries or tools are needed except for GPU detection (nvidia-smi/NVML, rocm-smi). This means no feature flags are needed for CPU, memory, network, or storage backends.

---

## Assumption Risk Assessment

Assumptions that, if wrong, would invalidate architectural decisions. Ordered by impact.

### Critical (would require redesign)

| Assumption | Decision it supports | What breaks if wrong |
|-----------|---------------------|---------------------|
| A-I8: Hardware watchdog available | PB1, PS2: watchdog as PID 1 crash recovery | If no watchdog on PID 1 nodes, agent hangs are unrecoverable without external monitoring. Would need a lightweight watchdog shim process (like tini) as actual PID 1. |
| A-RI1: cgroup v2 unified hierarchy | All of Resource Isolation context | If some nodes run cgroup v1 or hybrid, the entire cgroup management layer needs dual implementation. PactSupervisor becomes significantly more complex. |
| A-I1: Diskless compute nodes | Identity Mapping, Network Management, Bootstrap | If nodes have persistent local storage, the "no state survives reboot" assumption breaks. UidMap caching, network config persistence, and boot sequence all change. |
| A-NM2: Network config deterministic from overlay | NM1, NM2, PB3 Phase 2 | If nodes need DHCP or dynamic discovery, ConfigureNetwork can't be a simple "apply overlay" phase. Would need a DHCP client as a supervised service, changing boot ordering. |

### High (would require significant rework)

| Assumption | Decision it supports | What breaks if wrong |
|-----------|---------------------|---------------------|
| A-I7: SPIRE pre-existing | PB4, PB5, N10: bootstrap identity → SVID | If SPIRE is NOT deployed, the SVID rotation path never activates. Not fatal (PB5 says no hard dependency), but the mTLS story is weaker — bootstrap identity or journal-signed certs only. |
| A-Id1: NFS requires POSIX UID/GID | All of Identity Mapping context | If storage migrates to pure S3/NFSv4 with string identifiers, the entire pact-nss crate and UidMap machinery becomes unnecessary. This is the desired end state — the shim is deliberately disposable. |
| A-Int6: Lattice works independently | hpc-core shared kernel design | If lattice becomes tightly coupled to pact (can't run without it), the hpc-core contracts are over-designed. Both systems implementing provider AND consumer becomes unnecessary. |
| A-PS2: cgroup.kill reliable | PS3: immediate child cleanup | If cgroup.kill is unreliable on target kernels, child process cleanup becomes best-effort. Orphaned processes could accumulate. |

### Low (manageable rework)

| Assumption | Decision it supports | What breaks if wrong |
|-----------|---------------------|---------------------|
| A-Id5: Stride 10,000 sufficient | IM2, IM3: precursor ranges | If an org has >10,000 users, stride must be increased. Requires UID remapping for that org. Operationally painful but not architecturally breaking. |
| A-Int8: libnss 0.9.0 suitable | Identity Mapping implementation | If crate is abandoned or incompatible, write NSS module manually. Small C shim + Rust FFI. ~200 lines of code. |
| A-WI3: Hold timer default TBD | WI3: mount caching | Need benchmarking. Wrong default = suboptimal performance, not correctness issue. |
| A-Hw4: GH200 unified memory via standard interfaces | MemoryBackend: no vendor-specific parsing | If GH200 reports memory differently, need vendor-specific detection path for memory type. Total bytes still from /proc/meminfo. |
| A-Hw5: MI300A HBM via standard interfaces | MemoryBackend: no vendor-specific parsing | Same as A-Hw4 — fallback to vendor-specific detection for memory type only. |
| A-Hw6: Slingshot uses cxi driver | NetworkBackend: fabric classification | If driver name changes, update the driver-to-fabric mapping table. Single string comparison. |
| A-Hw7: /proc and /sys sufficient | All hardware backends: no feature flags | If a hardware category needs vendor libraries, add a feature flag for that category (same pattern as GPU). |

---

## Diagnostic Log Retrieval Assumptions

### A-Log1: dmesg ring buffer available [Accepted]
dmesg ring buffer is available via /dev/kmsg (read) or `dmesg` command on all compute nodes. Buffer size is kernel-configured (typically 256KB-1MB). Reading /dev/kmsg does not require root when pact-agent runs as PID 1.

### A-Log2: Service stdout/stderr captured to log files [Accepted]
In PactSupervisor mode, service stdout/stderr is captured to `/run/pact/logs/{service_name}.log` (rotated by pact-agent). In systemd mode, `journalctl -u {service}` is used. Log files are on tmpfs and do not survive reboot (diskless nodes).

### A-Log3: Syslog path varies by distribution [Accepted]
Syslog is at /var/log/syslog (Debian/Ubuntu) or /var/log/messages (RHEL/SLES). Agent checks both paths, uses whichever exists. If neither exists, syslog source is skipped (F43).

### Open Unknowns

| ID | Question | Impact if wrong | Next step |
|----|---------|----------------|-----------|
| A-Int2 | OpenCHAMI Rust client exists? | Delegation commands remain stubbed | Check OpenCHAMI API, assess effort |
| A-WI3 | Mount hold timer optimal value? | Performance only | Benchmark uenv mount latency |
| NEW | dbus-daemon actually needed for DCGM? | If not needed, one fewer service | Test nv-hostengine standalone mode |
| NEW | rpcbind needed for NFSv4? | If NFSv4 only, rpcbind can be dropped | Check VAST NFS version in use |
| NEW | SPIRE Workload API socket path on HPE Cray? | Needed for N10 integration | Check /run/spire/agent.sock or equivalent |

---

## mTLS Architecture (RESOLVED)

### A-mTLS1: SPIRE is the primary mTLS provider, journal self-signed fallback [Resolved]

**Resolution:**
- **Primary path**: pact-agent gets SVIDs from SPIRE (workload attestation). SPIRE handles rotation.
- **Fallback path**: Journal generates an ephemeral CA at startup (or loads from disk). Agents generate keypairs, submit CSRs, journal signs locally. No external dependency (no Vault).
- **Bootstrap**: OpenCHAMI provisioned identity (bootstrap cert in SquashFS) used for initial journal auth before SPIRE is reachable.
- **Revocation**: Revoked cert serials stored in Raft state (revocation registry). No external CRL service.

**What survives in ADR-008:**
- Bootstrap identity concept (first auth before SPIRE)
- Enrollment registry (hardware identity → domain membership)
- Dual-channel rotation pattern (applicable to SVID rotation too)
- EnrollmentState machine (Registered/Active/Inactive/Revoked)

**Lattice also needs mTLS:**
- Lattice-node-agent → lattice-quorum communication uses mTLS
- If SPIRE is deployed, lattice should also obtain SVIDs
- If SPIRE is NOT deployed, lattice needs its own cert management
- This is a shared concern → belongs in hpc-core

**Resolved via: hpc-identity crate in hpc-core**

| Trait | Purpose |
|-------|---------|
| `IdentityProvider` | Trait for obtaining workload identity. Implementations: SpireProvider (SVID), SelfSignedProvider (ADR-008 style), StaticProvider (bootstrap) |
| `CertRotator` | Trait for certificate rotation. Dual-channel pattern as default impl. |
| `WorkloadIdentity` | Type holding cert + key + trust bundle, regardless of source |

Both pact and lattice implement `IdentityProvider`:
- When SPIRE available: use `SpireProvider`
- When no SPIRE: pact uses `SelfSignedProvider` (journal CA), lattice uses its own equivalent
- On first boot: both use `StaticProvider` (bootstrap cert from OpenCHAMI)
