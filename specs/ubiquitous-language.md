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
| **ServiceDecl** | Declaration of a service: binary, args, restart policy, dependencies, order, cgroup limits, health check. |
| **PactSupervisor** | Built-in process supervisor. Default backend. Direct fork/exec, cgroup v2 isolation, health checks, dependency ordering. |
| **SystemdBackend** | Fallback supervisor. Generates unit files, delegates to systemd via D-Bus. |
| **ServiceManager trait** | Interface implemented by both PactSupervisor and SystemdBackend: start, stop, restart, status, health. |
| **Dependency ordering** | Services started in `order` field sequence, dependencies satisfied first. Shutdown in reverse order. |

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
| **OpenCHAMI** | Hardware discovery, boot provisioning, DHCP, BMC management. Below pact in the stack. Handles reboot, re-image, firmware. |
| **Lattice** | Workload scheduler. Beside pact. Handles drain, cordon, job management. pact starts lattice-node-agent as a supervised service. |
| **Sovra** | Federated key management and cross-org trust. Above pact. Federates policy templates, not config state. |
| **BMC console** | Out-of-band fallback when pact-agent is down. Unrestricted bash via Redfish. Changes detected as unattributed drift. |

## Node Enrollment & Certificate Lifecycle (ADR-008)

| Term | Definition |
|------|-----------|
| **Domain** | A logical pact instance: one journal quorum, one Vault intermediate CA, one enrollment registry. A physical site may have multiple domains. |
| **Domain membership** | "This node is allowed to exist in this pact instance." Controls certificate trust. Independent of vCluster assignment. |
| **Enrollment** | Admin-initiated registration of a node in the domain's enrollment registry. Requires hardware identity (MAC, BMC serial). Platform-admin only. |
| **EnrollmentState** | Lifecycle state of a node's domain membership: Registered (enrolled, not yet booted), Active (booted, mTLS up), Inactive (heartbeat timeout), Revoked (decommissioned). |
| **Hardware identity** | MAC addresses + BMC serial (SMBIOS/DMI). Used as bootstrap credential during boot enrollment. Optionally includes TPM endorsement key hash. |
| **CSR (Certificate Signing Request)** | PKCS#10 request generated by the agent at boot. Contains the agent's public key. Signed by the journal's intermediate CA. Private key never leaves the agent. |
| **Intermediate CA** | Journal nodes hold a Vault-delegated intermediate CA signing key. Signs agent CSRs locally (~1ms, CPU only). No Vault traffic per boot or renewal. |
| **Dual-channel rotation** | Agent cert renewal strategy: build passive gRPC channel with new cert, health-check, atomically swap with active channel. No operational disruption. |
| **Maintenance mode** | An enrolled, active node with no vCluster assignment. Runs domain-default config only (time sync, agent telemetry). Not schedulable. Shell/exec available to platform-admin. |
| **Domain-default config** | Minimal VClusterPolicy applied to unassigned nodes: observe-only enforcement, empty whitelists (platform-admin bypass), no regulated flags. |
| **Heartbeat** | Node liveness detected via config subscription stream. Disconnect + grace period (default 5 minutes) → Active to Inactive transition. |
| **Decommission** | Permanent removal of a node from the domain. Sets enrollment state to Revoked, publishes cert serial to Vault CRL. Requires `--force` if active sessions exist. |

## Deployment

| Term | Definition |
|------|-----------|
| **Standalone mode** | Journal runs on dedicated nodes (3-5), separate from lattice quorum. |
| **Co-located mode** | Journal and lattice quorum on same physical nodes but independent Raft groups, separate ports, separate state. |
| **Observe-only mode** | Initial deployment mode. Drift detected and logged but not enforced. `enforcement_mode = "observe"`. |
| **Enforce mode** | Production mode. Drift triggers commit windows and auto-rollback. `enforcement_mode = "enforce"`. |
