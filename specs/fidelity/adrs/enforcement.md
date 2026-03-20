# ADR Enforcement Report

Last scan: 2026-03-20

## Enforcement Status

| ADR | Decision (short) | Status | Evidence |
|-----|------------------|--------|----------|
| ADR-001 | Raft quorum: standalone or co-located | DOCUMENTED | Design pattern, no test verifies co-location vs standalone behavior differs correctly |
| ADR-002 | Blacklist-first drift detection, observe-only bootstrap | **ENFORCED** | `drift_detection.feature` scenarios 1-5 test blacklist filtering through real `DriftEvaluator`. `boot.rs:224` sets ObserveOnly at boot. Feature scenarios 15-16 test observe/enforce modes. |
| ADR-003 | OPA/Rego via localhost REST on journal | **ENFORCED** | `policy_evaluation.feature` tests OPA via `MockOpaClient`. `pact-e2e/tests/opa_policy.rs` tests real OPA container. `DefaultPolicyEngine` calls OPA on RBAC Defer. |
| ADR-004 | Emergency mode preserves audit trail, no whitelist expansion | **ENFORCED** | `emergency_mode.feature` tests emergency start/end journal entries. `emergency.rs:305` asserts whitelist unchanged during emergency. `emergency.rs:310` comments ADR-004. |
| ADR-005 | No agent-level Prometheus metrics | DOCUMENTED | `observability.rs:307` has a comment "ADR-005: agents don't expose /metrics" but the THEN step is a no-op. No test verifies an agent actually lacks a `/metrics` endpoint. |
| ADR-006 | pact-agent as init with systemd fallback | **ENFORCED** | `platform_bootstrap.feature` tests full boot phase sequence. `boot.rs` tests PactSupervisor mode (6 phases) and systemd mode (3 phases skipped). Phase ordering enforced by `execute_boot_phases()`. |
| ADR-007 | No SSH — pact shell replaces remote access | DOCUMENTED | Shell/exec features tested in `shell_session.feature` and `exec_endpoint.feature`. Whitelist enforcement tested. But no test verifies SSH is absent or that pact shell is the ONLY access path. |
| ADR-008 | Node enrollment, hardware identity, CSR signing | **ENFORCED** | `node_enrollment.feature` (47 scenarios) covers enrollment states, hardware identity matching, CSR flow, cert rotation, decommissioning. State machine transitions go through real `JournalState`. |
| ADR-009 | Overlay staleness detection + on-demand rebuild | **ENFORCED** | `boot_config_streaming.feature` scenarios 6-8 test overlay rebuild on commit and on-demand build when not cached. Real `JournalCommand::SetOverlay` path. |
| ADR-010 | Node delta TTL bounds (15 min – 10 days) | **ENFORCED** | `commit_window.feature` scenarios 17-20 test TTL below minimum (rejected), at minimum (accepted), above maximum (rejected), at maximum (accepted). Real `JournalState` TTL validation. |
| ADR-011 | Degraded-mode policy: cached whitelist, fail-closed | PARTIAL | `policy_evaluation.feature` tests OPA unavailability via `MockOpaClient::unavailable()`. `DefaultPolicyEngine` falls back to warn+allow on OPA failure (rules/mod.rs:295). But fail-closed for two-person approval during partition is not tested. |
| ADR-012 | Merge conflict grace period, journal-wins fallback | UNENFORCED | No test verifies the three-phase conflict resolution (feed back drift, pause convergence, grace period, journal-wins). |
| ADR-013 | Two-person approval as stateful Raft entries | **ENFORCED** | `rbac_authorization.feature` tests two-person approval flow. `DefaultPolicyEngine::evaluate_sync` creates `PendingApproval` entries. Approval resolution tested (approve, reject, timeout). Distinct identity requirement (approver ≠ requester) tested. |
| ADR-014 | Optimistic concurrency with commit windows | **ENFORCED** | `commit_window.feature` scenarios 1-7 test the formula, commit/rollback lifecycle. Window expiry assertion is shallow (flag-based), but the formula and lifecycle are thorough. |
| ADR-015 | hpc-core shared contracts (hpc-node, hpc-audit, hpc-identity) | DOCUMENTED | Trait definitions exist. No test verifies that pact correctly implements hpc-core contracts or that the contracts are satisfied. |
| ADR-016 | Identity mapping OIDC→POSIX via NSS shim | DOCUMENTED | `identity_mapping.feature` exists with BDD tests. Real `UidMapManager` tested. But the NSS module (`pact-nss`) is a separate crate outside the workspace — its integration is not tested. |
| ADR-017 | Management network for pact, HSN for lattice | DOCUMENTED | Network topology is a deployment concern. No test verifies network binding or traffic isolation. |

## Summary

| Status | Count | ADRs |
|--------|-------|------|
| **ENFORCED** | 8 | 002, 003, 004, 006, 008, 009, 010, 013, 014 |
| PARTIAL | 1 | 011 |
| DOCUMENTED | 6 | 001, 005, 007, 015, 016, 017 |
| UNENFORCED | 1 | 012 |

Note: ADR-014 counted as ENFORCED despite the shallow expiry test because the formula and commit/rollback lifecycle are thoroughly tested.

## Priority Actions

1. **ADR-012 (merge conflict grace period)**: Only UNENFORCED ADR. The three-phase conflict resolution is not tested at all. This is the most complex cross-feature interaction and affects data integrity during network partitions.
2. **ADR-011 (degraded-mode policy)**: Two-person approval during partition (fail-closed) is untested. Only OPA unavailability path tested.
3. **ADR-005 (no agent Prometheus)**: Easy to enforce — add a test that verifies no HTTP listener on the standard Prometheus port.
4. **ADR-007 (no SSH)**: Hard to enforce in BDD. Could add a test verifying pact-agent doesn't start sshd and that shell whitelist doesn't include ssh-related commands.
