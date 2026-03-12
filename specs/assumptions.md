# Pact Assumptions

Assumptions underlying pact's design. Each is classified as **Validated** (confirmed true), **Accepted** (team decision, not externally verified), or **Unknown** (needs investigation).

---

## Infrastructure Assumptions

### A-I1: Diskless compute nodes [Validated]
Compute nodes boot from SquashFS images provisioned by OpenCHAMI. No persistent local storage. All persistent state lives in the journal.

### A-I2: mTLS certificates provisioned by OpenCHAMI [Accepted]
Base images include mTLS certificates for pact-agent → journal authentication. Certificate provisioning is OpenCHAMI's responsibility. Certificate rotation is out of scope for initial implementation.

### A-I3: 3-5 journal nodes available [Accepted]
Raft quorum requires 3 (tolerates 1 failure) or 5 (tolerates 2 failures) nodes. These are either dedicated or co-located with lattice management nodes.

### A-I4: Network fabric available at boot [Accepted]
pact-agent can reach journal nodes via network at boot time. If network is unavailable, agent falls back to cached config (degraded boot).

### A-I5: cgroup v2 filesystem available [Validated]
PactSupervisor uses cgroup v2 for service isolation. Modern kernels (5.x+) have cgroup v2 by default. SquashFS images include cgroup2 mount.

### A-I6: 4-7 services per node [Validated]
Diskless HPC nodes run 4-7 services (chronyd, nvidia-persistenced, metrics, lattice-node-agent, etc.). This is far fewer than general-purpose servers, making PactSupervisor viable over systemd.

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

---

## Development Assumptions

### A-D1: macOS development, Linux production [Validated]
Developers work on macOS. Production runs on Linux. Three-tier strategy: feature-gate, mocks, devcontainer.

### A-D2: Stable Rust toolchain [Validated]
No nightly features used. `imports_granularity` and `group_imports` are nightly-only in rustfmt — not available.

### A-D3: openraft 0.10.0-alpha.14 is stable enough [Accepted]
Pinned alpha version. Loose `^0.10` caused breakage in lattice. Version pinned explicitly.

---

## Open Questions (Unknown Status)

### A-Q1: Cross-vCluster atomic operations [Unknown]
Can platform-admin apply config to multiple vClusters atomically? Current design is single-vCluster per operation.

### A-Q2: Partition conflict replay mechanism [Unknown]
How exactly are conflicting changes replayed after partition heals? Deduplication and conflict detection need specification.

### A-Q3: Node delta TTL bounds [Unknown]
Should there be min/max bounds on TTL? Preventing TTL=1s or TTL=10y is not currently specified.

### A-Q4: Snapshot retention during replication [Unknown]
What if a new snapshot is created while a follower is catching up from a previous snapshot? May need snapshot pinning.
