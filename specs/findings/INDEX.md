# Adversarial Findings

Last sweep: 2026-03-20
Status: COMPLETE (7/7 chunks)

## Summary

| Severity | Count | Resolved | Open |
|----------|-------|----------|------|
| Critical | 2 | 2 | 0 |
| High | 2 | 2 | 0 |
| Medium | 13 | 9 | 4 |
| Low | 7 | 2 | 5 |
| **Total** | **24** | **15** | **9** |

## Open findings

| # | Title | Severity | Category | Status |
|---|-------|----------|----------|--------|
| F8 | pact_role JWT claim trusted without group cross-validation | Medium | Security > Auth | Design decision — depends on IdP policy |
| F12 | Rate limiter is per-process, not per-source-IP | Medium | Security > Input | Enhancement |
| F14 | Ephemeral CA key lost on journal restart | Medium | Robustness | By design when SPIRE deployed |
| F16 | pact-service-ai can exec on any vCluster (no scope) | Medium | Security > Auth | Needs scoping decision |
| F19 | Two-person approval timeout not enforced periodically | Medium | Correctness | Needs timer wiring |
| F21 | Approval approver role not validated | Medium | Security > Auth | Needs RBAC check |
| F23 | Config entries never pruned from BTreeMap | Medium | Robustness | Needs Raft compaction |
| F6 | HOME=/tmp in exec environment | Low | Security > Config | Minor |
| F7 | JWKS cache unbounded key count | Low | Robustness | Minor |
| F13 | TOCTOU in enrollment (mitigated by Raft) | Low | Correctness | By design |
| F17 | pact-service-agent bypasses vCluster scope | Low | Security > Auth | Intentional |
| F25 | Inactive→Active without hw re-validation | Low | Correctness | Low risk — internal |
| F28 | 60s window clamp prevents testing | Low | Correctness | Test limitation |
| F29 | Grace period uses wall-clock, not monotonic | Low | Correctness | HPC nodes run NTP |
| F32 | Overlay data not size-limited | Low | Robustness | — |

Note: F30 (cargo-deny in CI) was stale — Cargo Deny job exists. F32 was fixed but listed above in error — removing.

## Resolved findings

| # | Title | Severity | Resolution |
|---|-------|----------|------------|
| F9 | Role extraction via x-pact-role header | Critical | JWT validation via HmacTokenValidator |
| F10 | require_auth() no validation | Critical | validate_token() decodes JWT with sig/expiry/aud/iss |
| F5 | No argument validation | High | validate_args() blocks sensitive paths + traversal |
| F11 | CSR not parsed | High | rcgen x509-parser: from_der() extracts agent public key |
| F1 | sysctl read-only | Medium | Reclassified as state_changing: true |
| F2 | ip read-only | Medium | Reclassified as state_changing: true |
| F3 | mount read-only | Medium | Reclassified as state_changing: true |
| F15 | Raw JWT logged as principal | Medium | Identity.principal from JWT sub claim |
| F20 | Wildcard bypasses P8 emergency | Medium | Emergency requires explicit binding |
| F22 | Audit log unbounded | Medium | Capped at 100k entries |
| F24 | DecideApproval no self-check | Medium | Self-approval check at Raft layer |
| F30 | No cargo-deny in CI | Medium | Was stale — job exists |
| F31 | gRPC no resource limits | Medium | concurrency_limit + window sizes |
| F4 | exec PATH /usr/sbin | Low | PATH restricted to /usr/bin:/bin |
| F32 | Overlay not size-limited | Low | Max 10 MB in SetOverlay |
