# Pact Assumptions

Assumptions underlying pact's design. Each is classified as **Validated** (confirmed true), **Accepted** (team decision, not externally verified), or **Unknown** (needs investigation).

---

## Infrastructure Assumptions

### A-I1: Diskless compute nodes [Validated]
Compute nodes boot from SquashFS images provisioned by OpenCHAMI. No persistent local storage. All persistent state lives in the journal.

### A-I2: mTLS certificates managed by pact via CSR + journal intermediate CA [Accepted — supersedes original A-I2]
Certificate lifecycle is pact's responsibility (ADR-008). Vault issues an intermediate CA cert to journal nodes. Agents generate their own keypairs at boot and submit CSRs to the journal, which signs them locally (~1ms, CPU only). No private key material is stored in Raft or transmitted over the wire. Vault is never on the boot path or renewal path for individual agent certs — only for journal CA management. Certificate rotation uses dual-channel swap (3-day default lifetime, renewal at 2/3). OpenCHAMI/Manta is not involved in certificate provisioning.

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

## Homogeneity Assumption

### A-H1: vCluster node homogeneity [Accepted]
All nodes within a vCluster converge to the same overlay configuration. Per-node deltas are temporary exceptions (bounded by TTL, see A-Q3) that should be either promoted to the vCluster overlay or reverted. The system:
- **Warns** when per-node deltas exist beyond their TTL or when nodes within a vCluster have divergent configurations.
- **Reports** heterogeneity in status/diff output so operators can see which nodes deviate.
- **Does not enforce** homogeneity automatically — it flags the condition for admin decision.
Rationale: HPC vClusters assume uniform node configuration for scheduling correctness. Heterogeneous nodes can cause job failures if the scheduler assumes capability uniformity.
