# Chunk 3: RBAC/Policy Attack Surface

Reviewed: 2026-03-20
Files: pact-policy/src/rbac/mod.rs, pact-policy/src/rules/mod.rs, pact-journal/src/policy_service.rs

---

## Finding: F16 — `pact-service-ai` can exec on any vCluster without scope check
Severity: Medium
Category: Security > Authorization
Location: `crates/pact-policy/src/rbac/mod.rs:171-179`
Spec reference: P3 (role scoped to vCluster)
Description: The AI agent role `pact-service-ai` is not scoped to a specific vCluster. The vCluster scope check (lines 96-108) only applies to roles with a parseable vCluster suffix (`pact-ops-{vc}`, `pact-viewer-{vc}`, `pact-regulated-{vc}`). Since `pact-service-ai` doesn't match any of these prefixes, it skips the scope check entirely and proceeds to the role-specific block (line 171) which allows `exec` and all read actions on ANY vCluster.
Evidence: `extract_vcluster_from_role("pact-service-ai")` returns `None`. AI agent can exec on any vCluster.
Suggested resolution: Either scope AI agents per-vCluster (`pact-service-ai-{vc}`) or add explicit scope checking for service roles in the policy config.

---

## Finding: F17 — `pact-service-agent` bypasses vCluster scope check
Severity: Low
Category: Security > Authorization
Location: `crates/pact-policy/src/rbac/mod.rs:161-169`
Spec reference: P3 (role scoping)
Description: Same issue as F16 but for `pact-service-agent`. Agent machine identity can read status and logs from any vCluster. This is likely intentional (agents may serve multiple vClusters during boot), but it means a compromised agent can read all vCluster states.
Evidence: `extract_vcluster_from_role("pact-service-agent")` returns `None`.
Suggested resolution: Accept as intentional, but document in the RBAC engine that service roles bypass scope.

---

## Finding: F18 — Custom role bindings bypass vCluster scope check
Severity: Medium
Category: Security > Authorization
Location: `crates/pact-policy/src/rbac/mod.rs:182-189`
Spec reference: P3 (role scoping)
Description: Custom role bindings in `VClusterPolicy.role_bindings` are checked at the bottom of the evaluation (line 182-189) AFTER the vCluster scope check (lines 96-108) has already passed or been skipped. But the scope check at line 96-108 uses `extract_vcluster_from_role()` which returns `None` for custom role names — they don't match `pact-ops-*`, `pact-viewer-*`, or `pact-regulated-*`. So a custom role binding in vCluster "ml-training" can be used against scope `Scope::VCluster("other-vc")` because the scope check was skipped (role doesn't have a vCluster suffix).

Actually, re-reading lines 86-94: if `role_vcluster` is `None` (custom role), the scope check is skipped. Then it falls through to the role-specific blocks (viewer, regulated, ops — all fail for custom roles) and finally hits the role binding check. The binding check (line 182) checks that `binding.role == identity.role` and `binding.principals.contains(principal)` — but does NOT check that the scope matches the policy's vCluster. The binding exists on one VClusterPolicy, but the scope in the request could be a different vCluster.

Wait — the `policy` parameter is resolved from the request scope's vCluster (lines 96-100 in evaluate_sync). So the binding is checked against the correct vCluster's policy. If the custom role binding doesn't exist in the target vCluster's policy, it won't match. This is actually safe.

Severity revised: **Not a finding** — the policy is already scoped by the request's vCluster.

---

## Finding: F19 — Two-person approval timeout only checked on approve, not on access
Severity: Medium
Category: Correctness > Specification compliance
Location: `crates/pact-policy/src/rules/mod.rs:162-198`
Spec reference: P5 (approval timeout), ADR-013
Description: `approve()` checks expiry (line 172-176) before allowing approval. But there's no periodic cleanup — `expire_approvals()` exists (line 218-231) but must be called externally. If the journal never calls `expire_approvals()`, pending approvals remain in `Pending` state indefinitely. A stale approval could be approved long after the context changed.
Evidence: `expire_approvals()` is not called automatically. No timer or background task.
Suggested resolution: Wire `expire_approvals()` into the journal's periodic tick (e.g., Raft heartbeat or supervision loop). Or check expiry in `approve()` (already done — but also reject stale pending ops on next policy evaluation).

---

## Finding: F20 — Wildcard role binding (`*`) bypasses P8 emergency restriction
Severity: Medium
Category: Security > Authorization
Location: `crates/pact-policy/src/rbac/mod.rs:185, 76-83`
Spec reference: P8 (AI agents cannot emergency)
Description: The P8 check (lines 76-83) only fires for `pact-service-ai` role. A custom role with wildcard binding (`allowed_actions: ["*"]`) would allow emergency actions for any principal, even if the intent was to restrict emergency access. The wildcard check (line 185) doesn't exclude emergency actions.
Evidence: Unit test `wildcard_binding_allows_all_actions` (line 563) confirms wildcard allows `EMERGENCY_START`. A custom role binding could grant emergency access to non-ops users.
Suggested resolution: Exclude `emergency_start` and `emergency_end` from wildcard matching, or require explicit listing of emergency actions in bindings.

---

## Finding: F21 — Approval approver role not validated
Severity: Medium
Category: Security > Authorization
Location: `crates/pact-policy/src/rules/mod.rs:162-198`
Spec reference: ADR-013 (two-person approval)
Description: `approve()` checks that `approver.principal != requester.principal` (self-approval prevention, P4). But it does NOT check:
1. Whether the approver has the right role (e.g., ops or higher for the target vCluster)
2. Whether the approver is authenticated
3. Whether the approver is in the same vCluster scope

Any identity with ANY role can approve a pending operation, as long as they're not the requester. A viewer or even a service agent could approve a regulated commit.
Evidence: `approve()` parameter is just `&Identity`. No role check.
Suggested resolution: Add role validation: approver must have at least ops-level access for the target vCluster.

---

## Summary

| Severity | Count |
|----------|-------|
| Critical | 0 |
| High | 0 |
| Medium | 3 (F16: AI scope, F19: approval timeout, F20: wildcard emergency, F21: approver role) |
| Low | 1 (F17: agent scope) |
| Not a finding | 1 (F18: custom binding scope — safe by design) |
| **Total** | **4** |

The RBAC engine is well-structured with good invariant coverage (P1-P8). The main gaps are in the two-person approval workflow (approver validation, timeout enforcement) and service role scoping.
