# Fidelity Index

Last scan: 2026-04-02 (verification pass — corrected false positives + undercounts)
Scanned by: auditor verification

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
| Total BDD scenarios | **584** (584 pass, 0 skipped, 0 failed) |
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
| ADRs ENFORCED | 16 (13 full + 3 partial) |
| ADRs DOCUMENTED | 1 (001: e2e needed — multi-process Raft topology) |
| ADRs UNENFORCED | 0 |

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
| partition_resilience | 16 | 13 | 7 | 8 | 0 | **MODERATE** | ↑ was LOW — 9 self-fulfilling scenarios wired to real policy engine, JournalState, quorum math |
| cli_commands | 38 | 6 | 25 | 7 | 0 | **MODERATE** | ↑ was LOW — 12 skipped scenarios wired (delegation stubs) |
| cli_authentication | 27 | 3 | 18 | 3 | 2 | **LOW** | — |
| diag_retrieval | 22 | 5 | 17 | 0 | 0 | **MODERATE** | ↑ was LOW — fleet-wide assertions verify prefixes, node count, truncation |
| observability | 15 | 4 | 14 | 1 | 0 | **MODERATE** | ↑ was LOW — real Prometheus gather() replaces hardcoded strings. Raft metrics registered as planned-but-unwired. |
| auth_logout | 3 | 0 | 2 | 3 | 0 | **LOW** | — |
| federation | 10 | 0 | 3 | 7 | 7 | **LOW** | — (no real impl, site-local unverified) |

### Tier 4: Previously NONE/DEAD — now resolved

| Feature | Scenarios | Status | Notes |
|---------|-----------|--------|-------|
| cross_context | 22 | **LOW→MODERATE** | 38 empty Then stubs replaced with conditional assertions. ~8 remain as kernel/e2e-only (documented). |
| node-management-delegation | 16 | **NONE→HIGH** | 16 scenarios pass via axum mock HTTP server. Real CsmBackend + OpenChamiBackend calls. |

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
| 001 | Raft quorum deployment modes | DOCUMENTED (e2e needed) |
| 002 | Blacklist-first drift detection | **ENFORCED** |
| 003 | OPA/Rego on journal nodes | **ENFORCED** |
| 004 | Emergency mode audit trail | **ENFORCED** (partial) |
| 005 | No agent Prometheus | **ENFORCED** |
| 006 | Pact-agent as init | **ENFORCED** |
| 007 | No SSH — pact shell | **ENFORCED** (partial) |
| 008 | Node enrollment + cert lifecycle | **ENFORCED** |
| 009 | Overlay staleness + on-demand rebuild | **ENFORCED** |
| 010 | Node delta TTL bounds | **ENFORCED** |
| 011 | Degraded-mode policy | **ENFORCED** |
| 012 | Merge conflict grace period | **ENFORCED** |
| 013 | Two-person approval state machine | **ENFORCED** |
| 014 | Optimistic concurrency / commit windows | **ENFORCED** |
| 015 | hpc-core shared contracts | **ENFORCED** (compile-time + unit tests) |
| 016 | Identity mapping OIDC→POSIX | **ENFORCED** |
| 017 | Management network for pact | **ENFORCED** (boot ordering) |

## Cross-Cutting Findings

### Dead specs
- `node-management-delegation.feature` — 16 scenarios, zero step definitions. All FAIL on Background step (not silently skipped — visible in CI).

### Feature flag findings (CORRECTED 2026-04-02)
- ~~**`systemd`**: dead flag~~ **FALSE POSITIVE.** `systemd` flag exists but `SystemdBackend` is intentionally selected at runtime via `SupervisorBackend::Systemd` config enum (`boot/mod.rs:206-218`). Both backends compile unconditionally for deployment flexibility. Not a dead flag.
- ~~**`federation`**: dead flag~~ **FALSE POSITIVE.** `federation = ["dep:reqwest"]` gates the optional HTTP dependency for Sovra sync. Module always compiles but network calls are feature-gated. Working as designed.
- **`jwks`**: no test exercises the JWKS-enabled code path specifically.

### Pervasive self-fulfilling pattern
Multiple features share a pattern where WHEN steps set world-state flags and THEN steps read them back:
- ~~**partition_resilience**~~: FIXED (2026-04-02) — 9 scenarios now wire to real policy engine, JournalState, quorum math
- **platform_bootstrap**: watchdog, SPIRE, adaptive supervision, coldplug
- **cli_commands**: all delegation commands
- ~~**observability**~~: FIXED (2026-04-02) — real Prometheus gather() replaces hardcoded strings
- **cross_context**: auto-rollback WRITES state in THEN step

### Silent skip risk
Cucumber-rs silently skips unmatched scenarios. The 12 skipped CLI scenarios are not flagged as errors. The 16 node-management-delegation scenarios FAIL on Background (visible), but other unmatched scenarios could still silently skip. No mechanism detects new skips from step regex drift.

## Priority Actions

### Critical
1. **Wire node-management-delegation BDD** — 16 scenarios fail on undefined Background step + zero scenario-level step defs. Need Background fixture + step module with wiremock for CSM/OpenCHAMI HTTP mocking.
2. **Fix partition_resilience self-fulfilling tests** — 7 of 18 scenarios read back flags set by WHEN. Worst: leader failover THEN step *assigns* new leader instead of verifying election. Merge conflict scenarios (10-13) are fine. Use real Raft cluster (e2e exists) or real agent boot logic.

### High
3. ~~**Fix `systemd` feature flag**~~ REMOVED — false positive (runtime config dispatch, not dead).
4. ~~**Fix `federation` feature flag**~~ REMOVED — false positive (gates `dep:reqwest`, working as designed).
5. **Add skip detection** — CI step that asserts exact skip count (currently 12 CLI skips). Node-mgmt scenarios fail visibly. Any new skips = CI failure.
6. **Fix cross_context stubs** — **38** NONE-depth Then steps (was undercounted as 22). All have comment-only bodies with zero assertions.

### Medium
7. **Wire diag fleet-wide assertions** — 12 Then steps only check exit_code==0. Verify fan-out, prefixes, truncation.
8. **Wire cli_commands delegation** — **12** scenarios SKIPPED (undrain, dag×3, budget×2, backup×4, nodes×2). Need When step defs.
9. **Add NodeManagementBackend mock** — wiremock or similar to verify URL paths, request bodies, auth headers. No async method testing exists.
10. **Wire observability metrics** — fully self-fulfilling: WHEN seeds hardcoded strings, THEN reads them back. No real Prometheus scrape.

### Low
11. ~~**ADR-002 enforcement**~~ DONE — drift_detection.feature blacklist filtering already tests this
12. ~~**ADR-011 enforcement**~~ DONE — partition_resilience wired to real policy engine (Group 2)
13. ~~**ADR-017 enforcement**~~ DONE — boot ordering scenario verifies PullOverlay before StartServices

## Changelog

| Date | Action | Delta |
|------|--------|-------|
| 2026-03-20 | Initial sweep (9 chunks) | First checkpoint |
| 2026-03-27 | PID 1 feature audit | platform_bootstrap LOW→MODERATE |
| 2026-03-28 | Auth e2e + GCP deployment | TokenValidator WIRED, V2+V4 validated |
| 2026-04-01 | **Full re-sweep** (9 chunks, 32 features, 15 mocks, 17 ADRs) | 9 HIGH (+1), 14 MODERATE, 7 LOW (-1), 2 DEAD/NONE. node-management-delegation.feature added but unwired. identity_mapping↑HIGH, drift↑HIGH, rbac↑HIGH, commit_window↑HIGH. partition_resilience↓LOW. Feature flag gaps found (systemd, federation). 777 unit tests (+21), 50 e2e (+8). |
| 2026-04-02 | **Verification pass** — corrected false positives + undercounts | `systemd` and `federation` feature flags: NOT dead (false positives removed). cross_context stubs: 38 not 22 (undercount corrected). cli_commands skips: 12 not 11. node-mgmt-delegation: scenarios FAIL on Background, not silently skip. |
| 2026-04-02 | **Group 1+2 fixes** — node-mgmt wired, self-fulfilling tests replaced | node-mgmt-delegation: NONE→HIGH (16 scenarios, axum mock, real HTTP). partition_resilience: LOW→MODERATE (9 scenarios wired to real policy engine + JournalState). observability: LOW→MODERATE (real Prometheus gather). Raft metrics gap exposed (planned but unwired). Two-person approval finding: only regulated roles trigger Defer (P4). |
| 2026-04-02 | **Group 3 fixes** — stubs wired, skips eliminated | cli_commands: LOW→MODERATE (12 skipped scenarios wired). diag_retrieval: LOW→MODERATE (14 exit-code-only → real output assertions). **583/583 scenarios pass, 0 skipped.** |
| 2026-04-02 | **Groups 4+5** — CI guard + ADR enforcement + cross_context stubs | CI skip guard added. cross_context: 38 stubs → conditional assertions. ADR-002/011/017 all ENFORCED (drift blacklist, degraded policy, boot ordering). **584/584 pass, 12/17 ADRs enforced, 0 UNENFORCED.** |
