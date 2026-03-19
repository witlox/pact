# Dependency Graph

Module dependencies with justification. Each dependency traces to a spec requirement.

---

## Compile-Time Dependencies

```
pact-journal ──▶ pact-common    (shared types for JournalState, ConfigEntry, VClusterPolicy)
pact-journal ──▶ pact-policy    (library crate: PolicyService logic hosted in journal, ADR-003)
pact-journal ──▶ hpc-audit      (AuditEvent, AuditSink for journal audit log)
pact-journal ──▶ hpc-identity   (SelfSignedProvider for CSR signing, CertRotator — ADR-008 fallback)
pact-agent   ──▶ pact-common    (shared types for CapabilityReport, ServiceDecl, DriftVector)
pact-agent   ──▶ hpc-node       (CgroupManager, NamespaceProvider, MountManager, ReadinessGate traits)
pact-agent   ──▶ hpc-audit      (AuditEvent, AuditSink for agent audit log)
pact-agent   ──▶ hpc-identity   (IdentityProvider, IdentityCascade, CertRotator)
pact-policy  ──▶ pact-common    (shared types for Identity, Scope, VClusterPolicy)
pact-cli     ──▶ pact-common    (shared types for display/formatting)
pact-cli     ──▶ hpc-auth       (OAuth2 login/logout/token refresh, Auth1-Auth8)
pact-cli     ──▶ hpc-audit      (AuditEvent types for CLI audit logging)
pact-test-harness ──▶ pact-common (builders construct pact-common types)
pact-acceptance   ──▶ pact-common (step impls use pact-common types)
```

## Runtime Dependencies

```
pact-agent ──gRPC──▶ pact-journal   (ConfigService, BootConfigService, PolicyService)
pact-cli   ──gRPC──▶ pact-journal   (ConfigService, BootConfigService, PolicyService)
pact-cli   ──gRPC──▶ pact-agent     (ShellService: exec + shell)
pact-journal ──REST──▶ OPA sidecar  (localhost:8181, feature-gated, ADR-003)
pact-journal ──HTTP──▶ Loki         (event forwarding, optional)
pact-journal ──mTLS──▶ Sovra        (federation sync, feature-gated)
pact-agent ──tmpfs──▶ lattice-node-agent (CapabilityReport manifest)
pact-agent ──unix socket──▶ lattice-node-agent (namespace handoff, SCM_RIGHTS — N7)
pact-agent ──unix socket──▶ SPIRE agent (SVID acquisition — N10)
pact-cli   ──gRPC──▶ lattice        (drain/cordon delegation)
pact-cli   ──REST──▶ OpenCHAMI      (reboot/reimage delegation, stubbed)
```

## Justification Table

| Dependency | Justification |
|-----------|---------------|
| journal → common | JournalState stores ConfigEntry, VClusterPolicy, BootOverlay (domain-model.md: Configuration Management context) |
| journal → policy | ADR-003: PolicyService hosted in journal process. pact-policy is a library crate. |
| agent → common | Agent constructs CapabilityReport, evaluates DriftVector, manages ServiceDecl (domain-model.md: Node Management context) |
| policy → common | Policy evaluates against Identity, Scope, VClusterPolicy (invariants P1-P8) |
| cli → common | CLI formats and displays all domain types |
| cli → hpc-auth | OAuth2 token acquisition for CLI commands (Auth1: no unauth commands) |
| agent →(gRPC) journal | Boot config streaming (I2), config subscription (I1), commit/rollback (I3), audit logging (I4) |
| cli →(gRPC) journal | Config queries (I5): status, diff, log, apply, overlay |
| cli →(gRPC) agent | Exec/shell (I6): remote command execution, interactive sessions |
| journal →(REST) OPA | Policy evaluation delegation (I7), ADR-003 |
| agent →(gRPC) journal EnrollmentService | Boot enrollment (unauthenticated), cert renewal (mTLS) (ADR-008, E1) |
| agent →(tmpfs) lattice | Capability delivery (E1), assumption A-Int4 |
| agent → hpc-node | Cgroup, namespace, mount contracts (RI1-6, WI1-6, domain-model §2b/2f) |
| agent → hpc-audit | Audit event emission (O3, cross-cutting concern) |
| agent → hpc-identity | Workload identity acquisition (PB4-5, A-mTLS1) |
| journal → hpc-audit | Audit event emission for journal operations (O3) |
| journal → hpc-identity | CSR signing as SelfSignedProvider fallback (ADR-008) |
| agent →(unix socket) lattice | Namespace FD handoff (WI1, interaction N7) |
| agent →(unix socket) SPIRE | SVID acquisition (PB4-5, interaction N10) |

## Capability Detection Dependencies

The expanded hardware detection backends (cpu.rs, memory.rs, network.rs, storage.rs)
introduce **no new external crate dependencies**. All Linux backends use:

- `std::fs` — reading `/proc/cpuinfo`, `/proc/meminfo`, `/proc/mounts`, `/proc/modules`,
  `/sys/class/net/*/`, `/sys/devices/system/cpu/`, `/sys/devices/system/node/`, `/sys/block/*/`
- `nix::sys::statvfs::statvfs()` — real filesystem capacity per mount (CAP4).
  The `nix` crate is already a workspace dependency.
- `std::env::consts::ARCH` — CPU architecture detection for `CpuArchitecture` enum

Optional (graceful fallback if unavailable):
- `dmidecode --type 17` — memory type detection (DDR4/DDR5/HBM). Spawned via
  `tokio::process::Command` (same pattern as nvidia-smi/rocm-smi). Falls back to
  `MemoryType::Unknown` if dmidecode is not installed or not running as root.

Mock backends have zero dependencies beyond `std` and `async-trait`.

No changes to the compile-time or runtime dependency graph structure.

---

## Cycle Analysis

**No cycles.** Dependency graph is a DAG:
- `pact-common` is a leaf (no internal dependencies)
- `hpc-node`, `hpc-audit`, `hpc-identity` are leaves (no internal dependencies, no pact dependencies)
- `pact-policy` depends only on `pact-common`
- `pact-journal` depends on `pact-common` + `pact-policy` + `hpc-audit` + `hpc-identity`
- `pact-agent` depends on `pact-common` + `hpc-node` + `hpc-audit` + `hpc-identity` (runtime gRPC to journal)
- `pact-cli` depends on `pact-common` + `hpc-auth` + `hpc-audit`
- hpc-core crates have no dependencies on pact crates — shared kernel pattern

## God Module Check

| Module | Direct Compile Dependencies | Status |
|--------|----------------------------|--------|
| pact-common | 0 internal | OK — leaf |
| hpc-node | 0 internal | OK — leaf (hpc-core) |
| hpc-audit | 0 internal | OK — leaf (hpc-core) |
| hpc-identity | 0 internal | OK — leaf (hpc-core) |
| pact-policy | 1 (common) | OK |
| pact-journal | 4 (common, policy, hpc-audit, hpc-identity) | OK |
| pact-agent | 4 (common, hpc-node, hpc-audit, hpc-identity) | OK |
| pact-cli | 3 (common, hpc-auth, hpc-audit) | OK |
| pact-test-harness | 1 (common) | OK |

No module exceeds 5 direct dependencies. pact-journal and pact-agent are at 4 each — acceptable given their scope (control plane core and node management core respectively).
