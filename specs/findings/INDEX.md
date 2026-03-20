# Adversarial Findings

Last sweep: 2026-03-20
Status: COMPLETE (7/7 chunks, 24 findings, 21 resolved)

## Summary

| Severity | Count | Resolved | Open |
|----------|-------|----------|------|
| Critical | 2 | 2 | 0 |
| High | 2 | 2 | 0 |
| Medium | 13 | 12 | 1 |
| Low | 7 | 5 | 2 |
| **Total** | **24** | **21** | **3** |

## Open findings

| # | Title | Severity | Status |
|---|-------|----------|--------|
| F14 | Ephemeral CA key lost on journal restart | Medium | By design — SPIRE deployment eliminates this |
| F23 | Config entries never pruned from BTreeMap | Medium | Enhancement — needs Raft snapshot/compaction |
| F29 | Grace period uses wall-clock, not monotonic | Low | Accepted — HPC nodes run NTP/PTP |

## Resolved findings

| # | Title | Severity | Resolution |
|---|-------|----------|------------|
| F9 | Role extraction via x-pact-role header | Critical | JWT validation via HmacTokenValidator |
| F10 | require_auth() no validation | Critical | validate_token() with sig/expiry/aud/iss |
| F5 | No argument validation | High | validate_args() blocks sensitive paths |
| F11 | CSR not parsed | High | rcgen x509-parser from_der() |
| F1 | sysctl read-only | Medium | Reclassified state_changing: true |
| F2 | ip read-only | Medium | Reclassified state_changing: true |
| F3 | mount read-only | Medium | Reclassified state_changing: true |
| F8 | pact_role claim trust | Medium | Role pattern validation + warning on unknown roles |
| F12 | Rate limiter per-process | Medium | Per-IP rate limiting with global + per-IP buckets |
| F15 | Raw JWT logged as principal | Medium | Identity from JWT sub claim |
| F16 | AI agent not vCluster-scoped | Medium | ai_exec_allowed policy field (default false) |
| F19 | Approval timeout not enforced | Medium | Heartbeat monitor expires pending approvals |
| F20 | Wildcard bypasses P8 emergency | Medium | Emergency requires explicit binding |
| F21 | Approver role not validated | Medium | RBAC check: approver needs ops access to target vCluster |
| F22 | Audit log unbounded | Medium | Capped at 100k entries |
| F24 | DecideApproval no self-check | Medium | Self-approval check at Raft layer |
| F30 | No cargo-deny in CI | Medium | Was stale — job exists |
| F31 | gRPC no resource limits | Medium | concurrency_limit + window sizes |
| F4 | exec PATH /usr/sbin | Low | PATH restricted to /usr/bin:/bin |
| F32 | Overlay not size-limited | Low | Max 10 MB in SetOverlay |
| F6 | HOME=/tmp | Low | Accepted — minor, exec is short-lived |
| F7 | JWKS cache unbounded | Low | Accepted — bounded by IdP key count |
| F13 | TOCTOU enrollment | Low | Mitigated by Raft — by design |
| F17 | Service agent bypasses scope | Low | Intentional — agents serve multiple vClusters |
| F25 | Inactive→Active no hw re-check | Low | Low risk — Raft is internal |
| F28 | 60s window clamp | Low | Test limitation, not production issue |
