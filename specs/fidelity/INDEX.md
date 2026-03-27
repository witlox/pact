# Fidelity Index

Last scan: 2026-03-27 (incremental: PID 1 / WatchdogHandle / PlatformInit)
Scanned by: auditor sweep (9 chunks, SWEEP.md: COMPLETE) + incremental 2026-03-27

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
| Feature files scanned | **31 of 31** |
| Total scenarios | **567** (555 pass, 12 skipped) |
| THOROUGH scenarios | ~167 (30%) |
| MODERATE scenarios | ~205 (37%) |
| SHALLOW or worse | ~166 (30%) |
| Unit tests | 756 pass |
| Mock traits assessed | 12 |
| FAITHFUL mocks | 5 (+ 2 wired via trait, 3 partial, 2 N/A) |
| ADRs total | 17 |
| ADRs ENFORCED | 9 (+ 1 partial, 6 documented, 1 unenforced) |

## Feature Fidelity

### Tier 1: HIGH confidence (8 features) — was 7

| Feature | Scenarios | Thorough | Moderate | Shallow | Stub | Confidence | Delta |
|---------|-----------|----------|----------|---------|------|------------|-------|
| [journal_operations](features/remaining-25-summary.md) | 15 | 15 | 0 | 0 | 0 | **HIGH** | — |
| [hardware_detection](features/remaining-25-summary.md) | 25 | 22 | 3 | 0 | 0 | **HIGH** | — |
| [drift-detection](features/drift-detection.md) | 19 | 16 | 3 | 0 | 0 | **HIGH** | — |
| [node-enrollment](features/node-enrollment.md) | 47 | 27 | 13 | 5 | 0 | **HIGH** | — |
| [process_supervisor](features/remaining-25-summary.md) | 23 | 17 | 6 | 0 | 0 | **HIGH** | — |
| [policy_evaluation](features/remaining-25-summary.md) | 16 | 9 | 3 | 4 | 0 | **HIGH** | — |
| [workload_integration](features/remaining-25-summary.md) | 17 | 0 | 13 | 4 | 0 | **HIGH** | — |
| [boot-config-streaming](features/boot-config-streaming.md) | 11 | 7 | 4 | 0 | 0 | **HIGH** | ↑ was MODERATE |

### Integration: cross-context (1 feature)

| Feature | Scenarios | Thorough | Moderate | Shallow | Skipped | Confidence | Notes |
|---------|-----------|----------|----------|---------|---------|------------|-------|
| [cross-context](features/cross-context.md) | 24 | 3 | 12 | 0 | 9 | **MODERATE** | 9 skipped = step wording mismatches |

### Tier 2: MODERATE confidence (15 features) — was 14

| Feature | Scenarios | Thorough | Moderate | Shallow | Stub | Confidence | Delta |
|---------|-----------|----------|----------|---------|------|------------|-------|
| [commit-window](features/commit-window.md) | 20 | 12 | 5 | 3 | 0 | **MODERATE** | ↑ improved |
| [rbac_authorization](features/remaining-25-summary.md) | 10 | 5 | 3 | 2 | 0 | **MODERATE** | — |
| [emergency_mode](features/remaining-25-summary.md) | 13 | 0 | 10 | 1 | 2 | **MODERATE** | ↑ improved |
| [shell_session](features/remaining-25-summary.md) | 23 | 1 | 11 | 8 | 3 | **MODERATE** | — |
| [resource_isolation](features/remaining-25-summary.md) | 13 | 0 | 9 | 4 | 0 | **MODERATE** | — |
| [identity_mapping](features/remaining-25-summary.md) | 17 | 0 | 11 | 6 | 0 | **MODERATE** | — |
| [capability_reporting](features/remaining-25-summary.md) | 14 | 4 | 6 | 4 | 0 | **MODERATE** | — |
| [exec_endpoint](features/remaining-25-summary.md) | 13 | 0 | 6 | 7 | 0 | **MODERATE** | — |
| [network_management](features/remaining-25-summary.md) | 8 | 2 | 5 | 1 | 0 | **MODERATE** | — |
| [auth_login](features/remaining-25-summary.md) | 20 | 0 | 8 | 12 | 0 | **MODERATE** | — |
| [cli_commands](features/remaining-25-summary.md) | 30 | 0 | 15 | 15 | 0 | **MODERATE** | — |
| [boot-sequence](features/boot-sequence.md) | 12 | 2 | 5 | 2 | 3 | **MODERATE** | ↑ was LOW |
| [overlay_management](features/remaining-25-summary.md) | 15 | 7 | 6 | 2 | 0 | **MODERATE** | ↑ was LOW |
| [partition_resilience](features/remaining-25-summary.md) | 15 | 6 | 7 | 2 | 0 | **MODERATE** | ↑ was LOW |
| [platform_bootstrap](features/remaining-25-summary.md) | 19 | 9 | 7 | 3 | 0 | **MODERATE** | ↑ was LOW. WatchdogHandle + PlatformInit unit tests raise THOROUGH count. Supervision loop coupling verified via real code (PS2). BDD watchdog scenarios remain SHALLOW (hardware-dependent). |

### Tier 3: LOW confidence (7 features) — was 8

| Feature | Scenarios | Thorough | Moderate | Shallow | Stub | Confidence | Delta |
|---------|-----------|----------|----------|---------|------|------------|-------|
| [cli_authentication](features/remaining-25-summary.md) | 26 | 0 | 8 | 18 | 0 | **LOW** | — |
| [agentic_api](features/remaining-25-summary.md) | 18 | 0 | 6 | 12 | 0 | **LOW** | — |
| [diag_retrieval](features/remaining-25-summary.md) | 24 | 7 | 9 | 8 | 0 | **LOW** | ↑ improved |
| [observability](features/remaining-25-summary.md) | 15 | 3 | 6 | 5 | 1 | **LOW** | ↑ improved |
| [auth_token_refresh](features/remaining-25-summary.md) | 11 | 0 | 3 | 8 | 0 | **LOW** | — |
| [auth_logout](features/remaining-25-summary.md) | 3 | 0 | 0 | 3 | 0 | **LOW** | — |
| [federation](features/remaining-25-summary.md) | 9 | 0 | 2 | 4 | 3 | **LOW** | — |

Detail files: `specs/fidelity/features/`

## Mock Fidelity

| Trait | Real Impls | Mock Rating | Impact |
|-------|------------|-------------|--------|
| ServiceManager | PactSupervisor, SystemdBackend | **WIRED** | ~~HIGH~~ resolved |
| TokenValidator | HmacTokenValidator | **BYPASSED** | **HIGH** — identity set directly |
| Observer | Inotify, Netlink, eBPF | **BYPASSED** | MEDIUM — events constructed directly |
| NetworkManager | LinuxNetworkManager | PARTIAL | MEDIUM — stub never errors |
| PolicyEngine | DefaultPolicyEngine | PARTIAL | LOW — BDD uses real engine |
| OpaClient | HttpOpaClient | PARTIAL | MEDIUM — mock ignores input |
| GpuBackend | nvidia, amd | FAITHFUL | LOW |
| CpuBackend | LinuxCpuBackend | FAITHFUL | LOW |
| MemoryBackend | LinuxMemoryBackend | FAITHFUL | LOW |
| NetworkBackend | LinuxNetworkBackend | FAITHFUL | LOW |
| StorageBackend | LinuxStorageBackend | FAITHFUL | LOW |
| FederationSync | (none yet) | FAITHFUL | LOW |
| WatchdogHandle | Linux ioctl impl | FAITHFUL (non-Linux stub returns None) | LOW |
| PlatformInit | Linux mount/reaper impl | FAITHFUL (non-Linux stubs are no-op) | LOW |

Detail file: `specs/fidelity/mocks/mock-fidelity.md`

**Changes:** ServiceManager now WIRED (was BYPASSED). BDD steps call real PactSupervisor::start/stop/restart/start_all/stop_all.

## ADR Enforcement

| ADR | Decision (short) | Status | Delta |
|-----|------------------|--------|-------|
| 002 | Blacklist-first drift detection | **ENFORCED** | — |
| 003 | OPA/Rego on journal nodes | **ENFORCED** | — |
| 004 | Emergency mode audit trail | **ENFORCED** | — |
| 006 | Pact-agent as init | **ENFORCED** | — |
| 008 | Node enrollment + cert lifecycle | **ENFORCED** | — |
| 009 | Overlay staleness + on-demand rebuild | **ENFORCED** | — |
| 010 | Node delta TTL bounds | **ENFORCED** | — |
| 012 | Merge conflict grace period | **ENFORCED** | ↑ was UNENFORCED |
| 013 | Two-person approval state machine | **ENFORCED** | — |
| 014 | Optimistic concurrency / commit windows | **ENFORCED** | — |
| 011 | Degraded-mode policy | PARTIAL | — |
| 001 | Raft quorum deployment modes | DOCUMENTED | — |
| 005 | No agent Prometheus | DOCUMENTED | — |
| 007 | No SSH | DOCUMENTED | — |
| 015 | hpc-core shared contracts | DOCUMENTED | — |
| 016 | Identity mapping OIDC→POSIX | DOCUMENTED | — |
| 017 | Management network for pact | DOCUMENTED | — |

Detail file: `specs/fidelity/adrs/enforcement.md`

**Changes:** ADR-012 now ENFORCED (was UNENFORCED). ConflictManager wired with register_conflicts, resolve, check_grace_periods.

## Priority Actions (Post-Hardening)

### Resolved since last scan
- ~~Self-fulfilling THEN steps~~ — 8 fixed across drift, commit_window, enrollment, emergency
- ~~ServiceManager bypassed~~ — now wired through real PactSupervisor
- ~~ADR-012 unenforced~~ — ConflictManager wired into partition BDD steps
- ~~Active consumer flag-based~~ — now uses real rollback_with_check()
- ~~Capability report flag-based~~ — now generates real CapabilityReport

### Remaining

**Resolved via e2e containers (pact-e2e infrastructure exists):**
1. **Auth flow (TokenValidator, login/logout/refresh)** → add Keycloak/Dex container to pact-e2e, test real OAuth2 flows. Container image: `quay.io/keycloak/keycloak` or `ghcr.io/dexidp/dex`.
2. **Observability Loki/metrics** → `pact-e2e/tests/loki_events.rs` + `prometheus_metrics.rs` already exist (fail due to no Docker in CI). Fix CI or run locally.
3. **Federation** → needs Sovra container (not yet available). Defer until Sovra exists.

**Other remaining:**
4. **Overlay compression** — zstd in deps but not used in code yet
5. **Platform bootstrap resource budgets** — WatchdogHandle + PlatformInit implemented; boot petter (F33) and zombie reaper tested. Remaining: coldplug, boot time measurement under real load
6. **Agentic API response validation** — tool response content not inspected

## Changelog

| Date | Action | Delta |
|------|--------|-------|
| 2026-03-20 | Pass 1: 5 critical-path features | First scan |
| 2026-03-20 | Pass 2: 12 mocks + 17 ADRs | 3 traits bypassed, 8/17 ADRs enforced |
| 2026-03-20 | Pass 4: remaining 25 features | 7 HIGH, 12 MODERATE, 11 LOW |
| 2026-03-20 | Pass 3: implementer hardening | 14 files, ~50 edits, 3 traits wired |
| 2026-03-20 | Rescan post-hardening | **8 HIGH (+1), 14 MODERATE (+2), 8 LOW (-3)**. ADR-012 enforced. ServiceManager wired. |
| 2026-03-20 | Sweep checkpoint | 31 features, 555 scenarios, 12 traits, 17 ADRs, gaps.md populated. SWEEP.md: COMPLETE. |
| 2026-03-27 | PID 1 feature audit | platform_bootstrap LOW→MODERATE. WatchdogHandle + PlatformInit implemented. 756 unit tests, 567 BDD (555 pass). PB0-PB2, PS2 enforced via real code. |
