# Dependency Graph

Module dependencies with justification. Each dependency traces to a spec requirement.

---

## Compile-Time Dependencies

```
pact-journal ──▶ pact-common    (shared types for JournalState, ConfigEntry, VClusterPolicy)
pact-journal ──▶ pact-policy    (library crate: PolicyService logic hosted in journal, ADR-003)
pact-agent   ──▶ pact-common    (shared types for CapabilityReport, ServiceDecl, DriftVector)
pact-policy  ──▶ pact-common    (shared types for Identity, Scope, VClusterPolicy)
pact-cli     ──▶ pact-common    (shared types for display/formatting)
pact-cli     ──▶ hpc-auth       (OAuth2 login/logout/token refresh, Auth1-Auth8)
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
| agent →(tmpfs) lattice | Capability delivery (E1), assumption A-Int4 |

## Cycle Analysis

**No cycles.** Dependency graph is a DAG:
- `pact-common` is a leaf (no internal dependencies)
- `pact-policy` depends only on `pact-common`
- `pact-journal` depends on `pact-common` + `pact-policy`
- `pact-agent` depends only on `pact-common` (runtime gRPC to journal)
- `pact-cli` depends only on `pact-common` (runtime gRPC to journal + agent)

## God Module Check

| Module | Direct Dependencies | Status |
|--------|-------------------|--------|
| pact-common | 0 internal | OK — leaf |
| pact-policy | 1 (common) | OK |
| pact-journal | 2 (common, policy) | OK |
| pact-agent | 1 (common) | OK |
| pact-cli | 2 (common, hpc-auth) | OK |
| pact-test-harness | 1 (common) | OK |

No module exceeds 5 direct dependencies.
