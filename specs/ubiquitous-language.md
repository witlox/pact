# Pact Ubiquitous Language

Precise definitions for all terms used across pact documentation, code, and specs. When a term appears in code, tests, or conversation, it means exactly what is defined here.

---

## Core Concepts

| Term | Definition |
|------|-----------|
| **pact** | The configuration management and admin operations system for HPC/AI infrastructure. Replaces Ansible/Puppet/Salt AND SSH. |
| **vCluster** | A logical grouping of nodes with shared configuration policy. Each vCluster has its own scheduler policy, drift thresholds, whitelists, and role bindings. Not a Kubernetes vCluster. |
| **Node** | A single compute node managed by pact-agent. Identified by NodeId (string). |
| **Overlay** | A pre-computed, zstd-compressed config bundle for a vCluster. Streamed to nodes at boot (Phase 1). |
| **Delta** | A node-specific configuration change that supplements the vCluster overlay. Streamed at boot (Phase 2). Committed deltas persist across reboots. |
| **Drift** | Any deviation between declared state (in journal) and actual state (on node). Measured across 7 dimensions. |
| **Commit** | Accepting current actual state as the new declared state. Records a ConfigEntry in the journal. |
| **Rollback** | Reverting actual state to match declared state. May fail if active consumers hold resources. |
| **Promote** | Converting committed node-level deltas into vCluster overlay configuration. |

## Components

| Term | Definition |
|------|-----------|
| **pact-agent** | Per-node daemon. Init system on diskless nodes. Supervises services, observes state, detects drift, manages commit windows, provides shell/exec access. |
| **pact-journal** | Distributed immutable log. 3-5 node Raft quorum. Single source of truth for config state. Hosts PolicyService. |
| **pact-policy** | Library crate linked into pact-journal. Implements OIDC auth, RBAC, OPA policy evaluation. Not a standalone service. |
| **pact CLI** | Admin command-line tool. Connects to journal (config ops) and agent (exec/shell). |
| **pact MCP server** | Optional AI agent interface. Wraps gRPC APIs as MCP tools. |

## Configuration Model

| Term | Definition |
|------|-----------|
| **ConfigEntry** | An immutable record in the journal. Has sequence number, timestamp, entry type, scope, author, optional state delta, optional TTL. |
| **EntrySeq** | Monotonically increasing sequence number (u64). Assigned by Raft consensus. No gaps. |
| **Scope** | Target of a config entry: Global (all nodes), VCluster(id) (one vCluster), Node(id) (one node). |
| **StateDelta** | The actual changes in a config entry, across 7 dimensions: mounts, files, network, services, kernel, packages, gpu. |
| **DriftVector** | Per-dimension drift magnitudes (f64). Computed by comparing actual vs declared state. |
| **DriftWeights** | Per-dimension weights for magnitude computation. Kernel and GPU default to 2x. |
| **Drift magnitude** | Weighted Euclidean norm of the DriftVector. Higher = more deviation = shorter commit window. |
| **Blacklist** | Paths excluded from drift detection. Default: /tmp, /var/log, /proc, /sys, /dev, /run/user. Per-vCluster customizable. |

## Commit Window Model

| Term | Definition |
|------|-----------|
| **Commit window** | Time-limited window after drift detection. Formula: `base_window / (1 + magnitude * sensitivity)`. Default: base=900s, sensitivity=2.0. |
| **Base window** | Starting duration before drift scaling. Default 900 seconds (15 minutes). |
| **Sensitivity** | How aggressively drift compresses the window. Higher = shorter windows. Default 2.0. |
| **Auto-rollback** | Automatic reversion when commit window expires without commit. Suspended during emergency mode. |
| **Active consumer check** | Before rollback, verify no processes hold resources (e.g., open file handles on a mount). Rollback fails if consumers exist. |
| **TTL** | Time-to-live on a committed delta. After expiry, the delta is lazily removed. Emergency changes default to TTL = emergency window. |

## Emergency Mode

| Term | Definition |
|------|-----------|
| **Emergency mode** | Extended commit window (default 4 hours), suspended auto-rollback, full audit logging. Must end with explicit commit or rollback. (ADR-004) |
| **Stale emergency** | Emergency that exceeds its window without being ended. Triggers alert + scheduling hold. |
| **Force-end** | Another admin (ops or platform) can force-end a stale emergency. |

## Process Supervision

| Term | Definition |
|------|-----------|
| **ServiceDecl** | Declaration of a service: binary, args, restart policy, dependencies, order, resource limits, health check config. |
| **PactSupervisor** | Built-in process supervisor. Default backend. Direct fork/exec, cgroup v2 isolation, health checks, dependency ordering, background supervision loop. |
| **SystemdBackend** | Fallback supervisor. Generates unit files, delegates to systemd via D-Bus. No pact supervision loop — systemd handles restarts natively. |
| **ServiceManager trait** | Interface implemented by both PactSupervisor and SystemdBackend: start, stop, restart, status, health, start_all, stop_all. Strategy pattern. |
| **Supervision loop** | Background tokio task in PactSupervisor that polls process status and triggers restarts per RestartPolicy. Only runs in PactSupervisor mode, not SystemdBackend. Adaptive interval: faster when idle (deeper inspections, eBPF checks), slower when workloads active (minimize disturbance). Coupled to hardware watchdog — each tick pets `/dev/watchdog`. |
| **Dependency ordering** | Services started in `order` field sequence, dependencies satisfied first. Shutdown in reverse order. |
| **ProcessState** | Runtime state of a supervised process: ServiceState, PID, child handle, restart count, last exit code. |

## Resource Isolation

| Term | Definition |
|------|-----------|
| **CgroupSlice** | A cgroup v2 slice directory (e.g., `pact.slice/gpu.slice`). Created at boot, persists until shutdown. Defines ownership boundary. |
| **CgroupScope** | A cgroup v2 scope within a slice for a specific service instance. Contains resource limits. Created on service start, destroyed on stop. |
| **CgroupHandle** | Reference to a created CgroupScope, returned to Process Supervision via callback for process placement. |
| **ResourceLimits** | Memory max, CPU weight, IO limits applied to a CgroupScope. Defined in ServiceDecl. |
| **SliceOwnership** | Enum: Pact / Workload. Determines which system has exclusive write access to a cgroup subtree. |
| **Emergency override** | In declared emergency mode, pact can freeze/kill processes in workload.slice/ (lattice's subtree). Requires audit trail and OIDC authorization. |
| **OOMScoreAdj** | Linux OOM killer priority. pact-agent itself runs with OOMScoreAdj=-1000 (last to be killed). |
| **NamespaceSet** | Set of Linux namespaces (pid, net, mount) created for a lattice allocation via `unshare(2)`. |
| **NamespaceFd** | File descriptor for a namespace, passed to lattice-node-agent for workload placement. |

## Identity Mapping

| Term | Definition |
|------|-----------|
| **UidMap** | Complete mapping table: OIDC subjects → POSIX UID/GID entries. Stored in journal (Raft-committed), cached on agents as tmpfs files. |
| **UidEntry** | Single mapping: OIDC subject, uid, gid, username, home dir, shell, org. Immutable once assigned. |
| **GroupEntry** | Group mapping: name, gid, member usernames. Full supplementary group resolution. |
| **OrgIndex** | Sequential index assigned to a federated org on join (0=local, 1, 2, ...). Raft-committed. Reclaimable on departure after GC of that org's UidEntries. |
| **Stride** | Max users per org in the UID space. Configurable per site, default 10,000. Determines precursor spacing: `precursor = base_uid + org_index * stride`. |
| **Precursor** | Computed UID base for an org: `base_uid + org_index * stride`. All UIDs for that org fall in `[precursor, precursor + stride)`. |
| **pact-nss** | NSS module (`libnss_pact.so.2`) using `libnss` 0.9.0 crate. Reads from `/run/pact/passwd.db` and `/run/pact/group.db` via mmap. Only active when pact is init + NFS in use. |
| **On-demand assignment** | Default UID assignment model: unknown OIDC subject → pact-policy checks IdP → assigns UID from org range → Raft-commits to journal → propagates. |
| **Pre-provisioned assignment** | Regulated UID assignment model: all users pre-provisioned by admin. Unknown subjects rejected. Required for sensitive vClusters. Configurable. |
| **Precursor range** | Federation UID deconfliction strategy. Each org gets a computed non-overlapping range via precursor = base_uid + org_index * stride. Sequential assignment within range. Collision impossible by construction. Reclaimable on federation departure. |

## Network Management

| Term | Definition |
|------|-----------|
| **Netlink** | Linux kernel interface for network configuration. pact-agent uses netlink directly (via `nix` crate) to configure interfaces when in PactSupervisor mode. Replaces wickedd/NetworkManager. |
| **Coldplug** | One-time device setup at boot (device nodes, permissions, symlinks) without a persistent hotplug daemon. Replaces udevd on diskless compute nodes. |

## Platform Bootstrap

| Term | Definition |
|------|-----------|
| **BootPhase** | Stages of node initialization: InitHardware → ConfigureNetwork → LoadIdentity → PullOverlay → StartServices → Ready. |
| **BootSequence** | Ordered execution of boot phases with dependency enforcement. |
| **Hardware watchdog** | `/dev/watchdog` timer. pact-agent (as PID 1) periodically pets the watchdog. If pact-agent hangs/crashes and stops petting, the watchdog expires and BMC triggers a hard reboot. Only active when pact is PID 1 on BMC-equipped nodes. |
| **BootstrapIdentity** | Temporary credential from OpenCHAMI provisioning (in SquashFS image or kernel cmdline). Used for initial journal authentication before SPIRE is available. |
| **SpireSvid** | SPIFFE Verifiable Identity Document from SPIRE agent. Replaces bootstrap identity for production mTLS. pact works without SPIRE (standalone mode). |
| **ReadinessSignal** | Signal emitted when node is fully initialized and ready for workloads. Consumed by lattice-node-agent. |

## Workload Integration

| Term | Definition |
|------|-----------|
| **AllocationEnvironment** | Prepared node-side environment for a lattice allocation: namespace FDs, mount points, cgroup scope. |
| **NamespaceHandoff** | Protocol for passing namespace FDs from pact-agent to lattice-node-agent over a unix socket. Defined in hpc-core `hpc-node` crate. |
| **MountRef** | Reference-counted mount of a uenv SquashFS image. Multiple allocations share one mount. Lazy unmount with configurable cache hold time. |
| **ReadinessGate** | Signal from pact to lattice that the node environment is ready for allocations. |
| **hpc-node** | Shared crate in hpc-core defining contracts: cgroup slice naming, namespace handoff protocol, mount conventions, readiness signaling. Both pact and lattice implement independently. |
| **hpc-audit** | Shared crate in hpc-core defining audit types: AuditEvent, AuditSink trait, CompliancePolicy. Loose coupling, high coherence. |
| **hpc-identity** | Shared crate in hpc-core (or extension of hpc-auth) defining workload identity traits: IdentityProvider, CertRotator, WorkloadIdentity. Abstracts SPIRE vs self-signed vs bootstrap cert sources. Both pact and lattice implement. |
| **IdentityProvider** | Trait for obtaining workload identity. Implementations: SpireProvider (SVID from SPIRE), SelfSignedProvider (journal/quorum CA signing), StaticProvider (bootstrap cert from OpenCHAMI). |
| **WorkloadIdentity** | Type holding cert + private key + trust bundle, regardless of source (SPIRE SVID or self-signed). Used for mTLS channel construction. |

## Shell & Exec

| Term | Definition |
|------|-----------|
| **pact exec** | Single command execution. Pact controls full lifecycle. Command validated against whitelist, fork/exec'd directly (no shell interpretation). |
| **pact shell** | Interactive restricted bash session. NOT a custom shell — spawns bash with restricted PATH, PROMPT_COMMAND audit, optional mount namespace, cgroup limits. |
| **Restricted bash (rbash)** | Bash in restricted mode. Cannot change PATH, run absolute paths, or redirect output to files. |
| **Whitelist** | Set of allowed commands for exec and shell. Implemented via PATH restriction (symlinks to whitelisted binaries). Per-vCluster configurable. |
| **Learning mode** | When a command is not found, the event is captured and can be used to suggest whitelist additions. |
| **Shell escape vector** | A whitelisted binary that can spawn unrestricted shells (e.g., vi → `:!bash`, python → `os.system()`). Excluded from defaults. |

## Identity & Policy

| Term | Definition |
|------|-----------|
| **OIDC** | OpenID Connect. Authentication mechanism. Every operation carries a Bearer token in gRPC metadata. |
| **Principal** | The authenticated entity (email, service account). |
| **PrincipalType** | Human, Agent (AI), or Service (machine). |
| **Role** | Authorization scope. One of: pact-platform-admin, pact-ops-{vcluster}, pact-viewer-{vcluster}, pact-regulated-{vcluster}, pact-service-agent, pact-service-ai. |
| **RoleBinding** | Maps a role to principals and allowed actions within a VClusterPolicy. |
| **Two-person approval** | For regulated vClusters: state-changing operations require a second admin's approval before execution. |
| **OPA** | Open Policy Agent. Rego policy evaluation via REST sidecar on journal nodes (ADR-003). |
| **Degraded mode** | When PolicyService is unreachable: agent uses cached VClusterPolicy. Whitelists honored, two-person denied, complex rules denied. |

## Observability

| Term | Definition |
|------|-----------|
| **Journal metrics** | Prometheus metrics from journal servers (3-5 scrape targets). Raft health, entry counts, stream counts. |
| **Loki events** | Structured JSON events streamed from journal to Loki. Config commits, admin ops, emergencies. |
| **Agent health** | Reported via lattice-node-agent eBPF to existing Prometheus. No per-agent scrape target (ADR-005). |

## Integration Points

| Term | Definition |
|------|-----------|
| **Node management backend** | Pluggable system below pact for hardware discovery, boot provisioning, DHCP, BMC management. Two implementations: CSM (CAPMC + BOS + HSM) and OpenCHAMI (SMD Redfish + BSS + HSM). One per deployment (NM-I1). |
| **OpenCHAMI** | Open-source node management backend (forked from CSM). Uses SMD Redfish for power, BSS for boot, HSM for inventory. Future strategic direction. |
| **CSM** | Cray System Management — HPE's proprietary node management stack. Uses CAPMC for power, BOS for boot, HSM for inventory. Currently deployed. |
| **Lattice** | Workload scheduler. Beside pact. Handles drain, cordon, job management. pact starts lattice-node-agent as a supervised service. Integration via hpc-core shared contracts (cgroup, namespace handoff, mount conventions). Lattice works independently of pact but gains capabilities ("supercharged") when pact is init. |
| **Sovra** | Federated key management and cross-org trust. Above pact. Federates policy templates, not config state. |
| **BMC console** | Out-of-band fallback when pact-agent is down. Unrestricted bash via Redfish. Changes detected as unattributed drift. |
| **SPIRE** | SPIFFE Runtime Environment. Provides mTLS workload attestation. Pre-existing in HPE Cray infrastructure. pact integrates via SPIRE agent socket to obtain SVIDs. Not a pact-supervised service — a dependency. |
| **hpc-core** | Shared crate workspace (`../hpc-core`). Contains `raft-hpc-core`, `hpc-scheduler-core`, `hpc-auth`, and (new) `hpc-node`, `hpc-audit`. Trait-based contracts both pact and lattice implement independently. |

## Supervised Services (from real compute node analysis)

Typical services pact-agent supervises on diskless compute nodes. Derived from real `ps aux` output of HPE Cray EX compute nodes.

| Term | Definition |
|------|-----------|
| **chronyd** | Time synchronization daemon (NTP/PTP). Must start first — correct time required for mTLS cert validation and audit timestamps. |
| **cxi_rh** | CXI resource handler for Slingshot NIC. One instance per CXI device (typically 4 per node). Manages Slingshot fabric. |
| **nvidia-persistenced** | NVIDIA GPU persistence daemon. Keeps GPU driver loaded between workloads. Prevents cold-start latency. |
| **nv-hostengine** | NVIDIA DCGM (Data Center GPU Manager) host engine. GPU health monitoring, telemetry, ECC error tracking. |
| **rasdaemon** | Hardware Reliability, Availability, Serviceability daemon. Logs hardware errors (ECC, PCIe, thermal). Feeds into pact health monitoring. |
| **dbus-daemon** | System D-Bus. Required by DCGM. May be droppable if DCGM configured for standalone mode (site-specific). |
| **rpcbind / rpc.statd** | NFS RPC services. Required for NFS lock management (NFSv3). Keep if NFS storage used. |
| **auditd** | Linux audit framework daemon. Required for regulated/sensitive vClusters. |
| **audit-forwarder** | Forwards audit events to external SIEM. hpc-audit AuditSink implementation. Regulated vClusters only. |
| **lattice-node-agent** | Lattice workload management daemon. Last to start (depends on all infra services). |
| **atomd** | HPE Application Task Orchestration and Management daemon. **Replaced by pact.** |

**Replaces on compute nodes:** systemd (PID 1), atomd (HPE ATOM), nomad + executors, slurmd, munged, sssd, ldmsd, nrpe, hb_ref, rsyslogd, wickedd, udevd, haveged, DVS-IPC, agetty, bos.reporter.

## Node Enrollment & Certificate Lifecycle (ADR-008)

| Term | Definition |
|------|-----------|
| **Domain** | A logical pact instance: one journal quorum, one enrollment CA, one enrollment registry. A physical site may have multiple domains. |
| **Domain membership** | "This node is allowed to exist in this pact instance." Controls certificate trust. Independent of vCluster assignment. |
| **Enrollment** | Admin-initiated registration of a node in the domain's enrollment registry. Requires hardware identity (MAC, BMC serial). Platform-admin only. |
| **EnrollmentState** | Lifecycle state of a node's domain membership: Registered (enrolled, not yet booted), Active (booted, mTLS up), Inactive (heartbeat timeout), Revoked (decommissioned). |
| **Hardware identity** | MAC addresses + BMC serial (SMBIOS/DMI). Used as bootstrap credential during boot enrollment. Optionally includes TPM endorsement key hash. |
| **CSR (Certificate Signing Request)** | PKCS#10 request generated by the agent at boot. Contains the agent's public key. Signed by the journal's intermediate CA. Private key never leaves the agent. |
| **Intermediate CA** | Self-generated CA signing key. Generated at journal startup or loaded from disk. Signs agent CSRs locally (~1ms, CPU only). |
| **Dual-channel rotation** | Agent cert renewal strategy: build passive gRPC channel with new cert, health-check, atomically swap with active channel. No operational disruption. |
| **Maintenance mode** | An enrolled, active node with no vCluster assignment. Runs domain-default config only (time sync, agent telemetry). Not schedulable. Shell/exec available to platform-admin. |
| **Domain-default config** | Minimal VClusterPolicy applied to unassigned nodes: observe-only enforcement, empty whitelists (platform-admin bypass), no regulated flags. |
| **Heartbeat** | Node liveness detected via config subscription stream. Disconnect + grace period (default 5 minutes) → Active to Inactive transition. |
| **Decommission** | Permanent removal of a node from the domain. Sets enrollment state to Revoked, adds cert serial to Raft revocation registry. Requires `--force` if active sessions exist. |

## Network Topology

| Term | Definition |
|------|-----------|
| **Management network** | 1G Ethernet network for control plane traffic. Carries PXE boot (OpenCHAMI), BMC/IPMI, pact journal gRPC, admin CLI, SPIRE server. Always available — it is the PXE boot network. All pact traffic runs here. |
| **High-speed network (HSN)** | Slingshot/Ultra Ethernet (200G+) for workload and lattice traffic. Carries MPI/NCCL, storage data plane, lattice quorum Raft, lattice node-agent communication. Requires `cxi_rh` to be running — comes up after pact starts it as a supervised service. |
| **Network-agnostic identity** | X.509 certificates (SPIRE SVIDs) authenticate identity, not network interfaces. The same SVID works on both management and HSN. SPIRE agent is node-local (unix socket) — no network dependency for identity acquisition. |

## Deployment

| Term | Definition |
|------|-----------|
| **Standalone mode** | Journal runs on dedicated nodes (3-5), separate from lattice quorum. |
| **Co-located mode** | Journal and lattice quorum on same physical nodes but independent Raft groups, separate ports, separate networks. Pact journal on management net (:9443/:9444), lattice quorum on HSN (:50051/:9000). |
| **Observe-only mode** | Initial deployment mode. Drift detected and logged but not enforced. `enforcement_mode = "observe"`. |
| **Enforce mode** | Production mode. Drift triggers commit windows and auto-rollback. `enforcement_mode = "enforce"`. |
