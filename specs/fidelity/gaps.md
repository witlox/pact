# Cross-Cutting Gaps

Last scan: 2026-04-02 (post-fix update)

## Dead Specs

_Feature files with no step definitions._

None. All 32 features have matching step definitions. 584/584 scenarios pass.

## Orphan Tests

_Test files that don't map to any feature spec._

None. All 26 step modules correspond to at least one feature file.

## Stale Specs

_Specs whose language doesn't match current code._

| Feature | Issue | Impact |
|---------|-------|--------|
| cross_context.feature | 9 scenarios use step wording that doesn't match existing regexes | LOW — logic exists, phrasing mismatch only |

Mismatched steps (9):
- "a CapabilityReport is written to tmpfs" vs "should be written"
- "the commit window expires" vs "the window expires without action"
- "admin executes 'rm /tmp/test'" — args with spaces
- "admin 'ops-lead@...' force-ends" — email format
- "the network partition heals" vs "connectivity restored"
- "pact cleans up alloc-02's namespaces" — apostrophe
- "admin enters emergency mode with reason" — missing "on node"
- "researcher@... authenticates via OIDC" — duplicate removed

## Uncovered Modules

_Source modules with no BDD or integration coverage._

| Module | Has Unit Tests | BDD Coverage | Gap |
|--------|---------------|--------------|-----|
| pact-journal/src/boot_service.rs | No | Simulated | No gRPC-level test |
| pact-journal/src/policy_service.rs | No | Simulated | No gRPC-level test |
| pact-nss/ (LGPL crate) | Separate | Conceptual | NSS module not loaded |
| pact-agent/src/identity_cascade/spire.rs | Yes | Conceptual | SPIRE integration untested |

Note: `pact-journal/src/telemetry.rs` removed from this list — now has unit tests + Raft metrics wired.

## Feature Flag Gaps

_Code behind feature flags with no gated test scenarios._

| Flag | Crate | Modules | Has Gated Tests | Gap |
|------|-------|---------|-----------------|-----|
| ebpf | pact-agent | observer/ | No | Events simulated via ObserverEvent |
| spire | pact-agent | identity_cascade/ | No | SVID acquisition conceptual |
| nvidia | pact-agent | capability/ | No | MockGpuBackend used |
| amd | pact-agent | capability/ | No | MockGpuBackend used |

Note: `systemd` removed — runtime config dispatch, not a feature flag gap.
Note: `opa` removed — e2e test in pact-e2e covers OPA via real container.
Note: `federation` removed — gates `dep:reqwest`, working as designed.

**Note:** Feature-gated code requires external services. The `pact-e2e` crate has Docker-based OPA tests. Other flags need similar infrastructure.

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Dead specs | 0 | Clean (was 1 — node-mgmt-delegation now fully wired) |
| Orphan tests | 0 | Clean |
| Stale specs | 9 steps | Minor wording fixes |
| Uncovered modules | 4 | gRPC services + NSS (was 5 — telemetry now covered) |
| Feature flag gaps | 4 flags | ebpf, spire, nvidia, amd — need integration infra |
| Cross-context stubs | ~8 | Kernel/e2e-only (was 38 — 30 wired with conditional assertions) |
