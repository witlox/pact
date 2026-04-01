# Fidelity Index

Last scan: 2026-04-01 (full re-sweep, 9 chunks)
Scanned by: auditor sweep

## How to read this file

This file is the entry point for understanding what this project ACTUALLY verifies
versus what its specs CLAIM is verified. It is maintained by the auditor profile.

**Confidence levels:**
- **HIGH**: >80% of scenarios are THOROUGH or MODERATE depth
- **MODERATE**: >50% THOROUGH+, no critical gaps
- **LOW**: <50% THOROUGH+, or critical paths undertested
- **NONE**: no tests, or tests exist but assert nothing meaningful

**Assertion depth:**
- **INTEGRATION**: runs against real services (feature-gated)
- **THOROUGH**: asserts actual state through real or faithfully-mocked code
- **MODERATE**: asserts real return values but via mocked dependencies
- **SHALLOW**: asserts status codes, booleans, or mock invocation only
- **STUB**: step def exists but is empty / unimplemented
- **NONE**: no test exists for this criterion

## Summary

| Metric | Count |
|--------|-------|
| Feature files scanned | **32 of 32** |
| Total BDD scenarios | **583** (555 pass, 12 skipped, 16 dead) |
| THOROUGH scenarios | ~155 (27%) |
| MODERATE scenarios | ~215 (37%) |
| SHALLOW or worse | ~180 (31%) |
| NONE / STUB | ~33 (6%) |
| Unit tests | **777** pass |
| E2E integration tests | **50** (auth, Raft, OPA, Loki, Prometheus, SPIRE, CLI, supervisor, partition) |
| Mock traits assessed | 15 |
| WIRED mocks | 7 (ServiceManager, TokenValidator, WatchdogHandle, PlatformInit, CgroupManager + 2 capability) |
| FAITHFUL/CONVERGENT mocks | 6 (5 capability backends + MockOpaClient) |
| PARTIAL mocks | 2 (NetworkManager, MockPolicyEngine) |
| BYPASSED | 1 (Observer in BDD — wired in unit tests) |
| DIVERGENT | 1 (FederationSync — no real impl) |
| N/A | 1 (NodeManagementBackend — no mock, BDD all skipped) |
| ADRs total | 17 |
| ADRs ENFORCED | 9 (7 full + 2 partial) |
| ADRs DOCUMENTED | 7 |
| ADRs UNENFORCED | 1 (ADR-017) |

## Feature Fidelity

### Tier 1: HIGH confidence (9 features)

| Feature | Scenarios | Thorough | Moderate | Shallow | Stub/None | Confidence | Delta |
|---------|-----------|----------|----------|---------|-----------|------------|-------|
| journal_operations | 15 | 15 | 0 | 0 | 0 | **HIGH** | — |
| drift_detection | 20 | 13 | 4 | 0 | 0 | **HIGH** | ↑ was MODERATE-HIGH |
| identity_mapping | 16 | 14 | 16 | 1 | 1 | **HIGH** | ↑ was MODERATE |
| rbac_authorization | 11 | 7 | 2 | 0 | 0 | **HIGH** | ↑ was MODERATE |
| policy_evaluation | 19 | 9 | 5 | 1 | 3 | **HIGH** | — |
| hardware_detection | 30 | 0 | 2 | 25 | 4 | **HIGH** (unit) | BDD is LOW but 83 unit tests cover parsing THOROUGHLY |
| process_supervisor | 25 | 13 | 4 | 7 | 0 | **HIGH** | — (real PactSupervisor + 12 unit tests) |
| commit_window | 20 | 10 | 4 | 4 | 0 | **HIGH** | ↑ was MODERATE |
| node_enrollment | 42 | 8 | 18 | 14 | 0 | **HIGH** | ↑ (state machine wired, 183 step defs) |

### Tier 2: MODERATE confidence (14 features)

| Feature | Scenarios | Thorough | Moderate | Shallow | Stub/None | Confidence | Delta |
|---------|-----------|----------|----------|---------|-----------|------------|-------|
| boot_config_streaming | 11 | 5 | 5 | 3 | 0 | **MODERATE** | — |
| boot_sequence | 12 | 3 | 5 | 3 | 4 | **MODERATE** | — |
| emergency_mode | 14 | 7 | 4 | 1 | 1 | **MODERATE** | — |
| shell_session | 18 | 5 | 7 | 5 | 1 | **MODERATE** | — |
| exec_endpoint | 14 | 5 | 5 | 3 | 1 | **MODERATE** | — |
| workload_integration | 17 | 16 | 12 | 0 | 12 | **MODERATE** | — (MountRefManager wired but 12 NONE stubs) |
| resource_isolation | 13 | 0 | 18 | 3 | 1 | **MODERATE** | — |
| auth_login | 20 | 1 | 14 | 3 | 3 | **MODERATE** | — |
| auth_token_refresh | 11 | 1 | 7 | 3 | 0 | **MODERATE** | — |
| agentic_api | 12 | 4 | 4 | 4 | 0 | **MODERATE** | ↑ was LOW |
| network_management | 8 | 10 | 4 | 1 | 0 | **MODERATE** | — |
| platform_bootstrap | 17 | 5 | 7 | 12 | 0 | **MODERATE** | — (8 self-fulfilling clusters) |
| capability_reporting | 16 | 0 | 7 | 9 | 0 | **MODERATE** | — (unit tests compensate) |
| overlay_management | 16 | 6 | 7 | 7 | 0 | **MODERATE** | — |

### Tier 3: LOW confidence (7 features)

| Feature | Scenarios | Thorough | Moderate | Shallow | Stub/None | Confidence | Delta |
|---------|-----------|----------|----------|---------|-----------|------------|-------|
| partition_resilience | 16 | 7 | 7 | 14 | 0 | **LOW** | ↓ was MODERATE (self-fulfilling leader failover, cached boot) |
| cli_commands | 38 | 6 | 13 | 7 | 2+11skip | **LOW** | — (11 scenarios SKIPPED, delegation self-fulfilling) |
| cli_authentication | 27 | 3 | 18 | 3 | 2 | **LOW** | — |
| diag_retrieval | 22 | 5 | 5 | 12 | 0 | **LOW** | — (fleet-wide exit-code-only) |
| observability | 15 | 4 | 14 | 1 | 0 | **LOW** | — (metrics hardcoded in When steps) |
| auth_logout | 3 | 0 | 2 | 3 | 0 | **LOW** | — |
| federation | 10 | 0 | 3 | 7 | 7 | **LOW** | — (no real impl, site-local unverified) |

### Tier 4: NONE / DEAD (2 features)

| Feature | Scenarios | Status | Notes |
|---------|-----------|--------|-------|
| cross_context | 22 | **LOW** | 22 NONE-depth Then steps (stubs deferring to other features) |
| node-management-delegation | 16 | **NONE (BDD)** | All scenarios SKIPPED — no step definitions exist. 13 unit tests in delegate.rs cover factory + dispatch. |

## Mock Fidelity

| Trait | Real Impls | Mock Rating | Impact |
|-------|------------|-------------|--------|
| ServiceManager | PactSupervisor, SystemdBackend | **WIRED** | resolved |
| TokenValidator | JwksTokenValidator (HMAC+JWKS) | **WIRED** (e2e: Dex+interceptor) | resolved |
| Observer | Inotify, Netlink, eBPF | **BYPASSED** (BDD) / WIRED (unit) | MEDIUM |
| NetworkManager | LinuxNetworkManager | **PARTIAL** (stub never errors) | MEDIUM |
| PolicyEngine | DefaultPolicyEngine | **PARTIAL** (MockPolicyEngine doesn't impl trait) | LOW — BDD uses real engine |
| OpaClient | HttpOpaClient | **FAITHFUL** (MockOpaClient wired into real engine) | LOW |
| GpuBackend | nvidia, amd | **CONVERGENT** | LOW |
| CpuBackend | LinuxCpuBackend | **CONVERGENT** | LOW |
| MemoryBackend | LinuxMemoryBackend | **CONVERGENT** | LOW |
| NetworkBackend | LinuxNetworkBackend | **CONVERGENT** | LOW |
| StorageBackend | LinuxStorageBackend | **CONVERGENT** | LOW |
| FederationSync | (none — mock only) | **DIVERGENT** | LOW — no real impl exists |
| WatchdogHandle | Linux ioctl impl | **FAITHFUL** (non-Linux stub returns None) | LOW |
| PlatformInit | Linux mount/reaper impl | **FAITHFUL** (non-Linux stubs are no-op) | LOW |
| NodeManagementBackend | CsmBackend, OpenChamiBackend | **N/A** (no mock, BDD skipped) | MEDIUM — unit tests cover factory only |

## ADR Enforcement

| ADR | Decision (short) | Status |
|-----|------------------|--------|
| 001 | Raft quorum deployment modes | DOCUMENTED |
| 002 | Blacklist-first drift detection | DOCUMENTED |
| 003 | OPA/Rego on journal nodes | **ENFORCED** |
| 004 | Emergency mode audit trail | **ENFORCED** (partial) |
| 005 | No agent Prometheus | DOCUMENTED |
| 006 | Pact-agent as init | **ENFORCED** |
| 007 | No SSH — pact shell | **ENFORCED** (partial) |
| 008 | Node enrollment + cert lifecycle | **ENFORCED** |
| 009 | Overlay staleness + on-demand rebuild | DOCUMENTED |
| 010 | Node delta TTL bounds | **ENFORCED** |
| 011 | Degraded-mode policy | DOCUMENTED |
| 012 | Merge conflict grace period | **ENFORCED** |
| 013 | Two-person approval state machine | **ENFORCED** |
| 014 | Optimistic concurrency / commit windows | **ENFORCED** |
| 015 | hpc-core shared contracts | DOCUMENTED |
| 016 | Identity mapping OIDC→POSIX | DOCUMENTED |
| 017 | Management network for pact | UNENFORCED |

## Cross-Cutting Findings

### Dead specs
- `node-management-delegation.feature` — 16 scenarios, zero step definitions. All SKIPPED.

### Feature flag gaps
- **`systemd`**: declared but 0 `cfg(feature)` gates in code. `SystemdBackend` compiles unconditionally. Dead flag.
- **`federation`**: declared but 0 `cfg(feature)` gates. Federation code compiles unconditionally.
- **`jwks`**: no test exercises the JWKS-enabled code path specifically.

### Pervasive self-fulfilling pattern
Multiple features share a pattern where WHEN steps set world-state flags and THEN steps read them back:
- **partition_resilience**: leader failover, cached boot, subscription reconnect
- **platform_bootstrap**: watchdog, SPIRE, adaptive supervision, coldplug
- **cli_commands**: all delegation commands
- **observability**: Prometheus metrics hardcoded in WHEN
- **cross_context**: auto-rollback WRITES state in THEN step

### Silent skip risk
Cucumber-rs silently skips unmatched scenarios. The 12+16 skipped scenarios are not flagged as errors. No mechanism detects new skips from step regex drift.

## Priority Actions

### Critical
1. **Wire node-management-delegation BDD** — 16 scenarios with zero step defs. Need step module or mock HTTP server (wiremock).
2. **Fix partition_resilience self-fulfilling tests** — leader failover, cached boot, subscription reconnect all read back what WHEN set. Use real Raft cluster (e2e exists) or real agent boot logic.

### High
3. **Fix `systemd` feature flag** — either gate `SystemdBackend` behind `cfg(feature = "systemd")` or remove the dead flag.
4. **Fix `federation` feature flag** — same: gate behind feature or remove.
5. **Add skip detection** — CI step that asserts exact skip count (currently 12+16=28). Any new skips = CI failure.

### Medium
6. **Wire diag fleet-wide assertions** — 12 Then steps only check exit_code==0. Verify fan-out, prefixes, truncation.
7. **Wire cli_commands delegation** — 11 scenarios SKIPPED (undrain, dag, budget, backup, nodes). Need When step defs.
8. **Add NodeManagementBackend mock** — wiremock or similar to verify URL paths, request bodies, auth headers.
9. **Fix cross_context stubs** — 22 NONE-depth Then steps (empty bodies) for workload/namespace/cgroup operations.
10. **Wire observability metrics** — replace hardcoded metric strings with real Prometheus scrape.

### Low
11. **ADR-002 enforcement** — add test that blacklist-first mode prevents drift enforcement
12. **ADR-011 enforcement** — add test that OPA failure falls back to cached RBAC
13. **ADR-017 enforcement** — add test that pact traffic stays on management network

## Changelog

| Date | Action | Delta |
|------|--------|-------|
| 2026-03-20 | Initial sweep (9 chunks) | First checkpoint |
| 2026-03-27 | PID 1 feature audit | platform_bootstrap LOW→MODERATE |
| 2026-03-28 | Auth e2e + GCP deployment | TokenValidator WIRED, V2+V4 validated |
| 2026-04-01 | **Full re-sweep** (9 chunks, 32 features, 15 mocks, 17 ADRs) | 9 HIGH (+1), 14 MODERATE, 7 LOW (-1), 2 DEAD/NONE. node-management-delegation.feature added but unwired. identity_mapping↑HIGH, drift↑HIGH, rbac↑HIGH, commit_window↑HIGH. partition_resilience↓LOW. Feature flag gaps found (systemd, federation). 777 unit tests (+21), 50 e2e (+8). |
