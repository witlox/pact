# Cross-Cutting Gaps

Last scan: 2026-03-20 (sweep chunk 9)

## Dead Specs

_Feature files with no step definitions._

None. All 31 features have matching step definitions. 546/555 scenarios pass.

## Orphan Tests

_Test files that don't map to any feature spec._

None. All 23 step modules correspond to at least one feature file.

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
| pact-journal/src/telemetry.rs | No | Hardcoded strings | Metrics endpoint untested |
| pact-nss/ (LGPL crate) | Separate | Conceptual | NSS module not loaded |
| pact-agent/src/identity_cascade/spire.rs | Yes | Conceptual | SPIRE integration untested |

## Feature Flag Gaps

_Code behind feature flags with no gated test scenarios._

| Flag | Crate | Modules | Has Gated Tests | Gap |
|------|-------|---------|-----------------|-----|
| ebpf | pact-agent | observer/ | No | Events simulated via ObserverEvent |
| spire | pact-agent | identity_cascade/ | No | SVID acquisition conceptual |
| systemd | pact-agent | supervisor/ | Partial (flag check) | No real D-Bus calls |
| nvidia | pact-agent | capability/ | No | MockGpuBackend used |
| amd | pact-agent | capability/ | No | MockGpuBackend used |
| opa | pact-policy | rules/opa.rs | Partial (MockOpaClient) | e2e test in pact-e2e |
| federation | pact-policy | federation/ | No | MockFederationSync used |

**Note:** Feature-gated code requires external services. The `pact-e2e` crate has Docker-based OPA tests. Other flags need similar infrastructure.

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Dead specs | 0 | Clean |
| Orphan tests | 0 | Clean |
| Stale specs | 9 steps | Minor wording fixes |
| Uncovered modules | 5 | gRPC services + NSS |
| Feature flag gaps | 7 flags | Expected — need integration infra |
