# Adversarial Findings

Last sweep: 2026-03-20
Status: COMPLETE (7/7 chunks)

## Summary

| Severity | Count | Resolved | Open |
|----------|-------|----------|------|
| Critical | 2 | **2** | 0 |
| High | 2 | **1** | 1 |
| Medium | 13 | **6** | 7 |
| Low | 7 | **1** | 6 |
| **Total** | **24** | **9** | **15** |

## Open findings (sorted by severity)

| # | Title | Severity | Category | Location | Chunk |
|---|-------|----------|----------|----------|-------|
| F9 | Role extraction uses x-pact-role header without JWT validation | **Critical** | Security > Auth | enrollment_service.rs:107 | 2 |
| F10 | require_auth() only checks token presence, not validity | **Critical** | Security > Auth | enrollment_service.rs:53 | 2 |
| F5 | No argument validation on whitelisted commands | High | Security > Input | whitelist.rs:124 | 1 |
| F11 | CSR public key not extracted — server generates keypair | High | Security > Crypto | ca.rs:114 | 2 |
| F1 | sysctl classified as read-only | Medium | Correctness | whitelist.rs:103 | 1 |
| F2 | ip command can modify network state | Medium | Security > Input | whitelist.rs:85 | 1 |
| F3 | mount classified as read-only | Medium | Security > Input | whitelist.rs:78 | 1 |
| F8 | pact_role JWT claim trusted without cross-validation | Medium | Security > Auth | auth.rs:355 | 1 |
| F12 | Rate limiter is per-process, not per-source-IP | Medium | Security > Input | rate_limiter.rs:7 | 2 |
| F14 | Ephemeral CA key lost on journal restart | Medium | Robustness | ca.rs:23 | 2 |
| F15 | extract_principal() logs raw JWT as actor identity | Medium | Security > Secrets | enrollment_service.rs:96 | 2 |
| F16 | pact-service-ai can exec on any vCluster (no scope) | Medium | Security > Auth | rbac/mod.rs:171 | 3 |
| F19 | Two-person approval timeout not enforced periodically | Medium | Correctness | rules/mod.rs:218 | 3 |
| F20 | Wildcard role binding bypasses P8 emergency restriction | Medium | Security > Auth | rbac/mod.rs:185 | 3 |
| F21 | Approval approver role not validated | Medium | Security > Auth | rules/mod.rs:162 | 3 |
| F22 | Audit log unbounded Vec with no pruning | Medium | Robustness | raft/state.rs:31 | 4 |
| F23 | Config entries never pruned from BTreeMap | Medium | Robustness | raft/state.rs:21 | 4 |
| F24 | DecideApproval does not validate approver (Raft layer) | Medium | Security > Auth | raft/state.rs:243 | 4 |
| F30 | No cargo-deny or cargo-audit in CI | Medium | Security > Supply chain | justfile | 7 |
| F31 | gRPC services have no connection/message size limits | Medium | Robustness | main.rs | 7 |
| F4 | exec PATH includes /usr/sbin | Low | Security > Trust | exec.rs:80 | 1 |
| F6 | HOME=/tmp in exec environment | Low | Security > Config | exec.rs:81 | 1 |
| F7 | JWKS cache unbounded key count | Low | Robustness | auth.rs:129 | 1 |
| F13 | TOCTOU in enrollment (mitigated by Raft) | Low | Correctness | enrollment_service.rs:200 | 2 |
| F17 | pact-service-agent bypasses vCluster scope | Low | Security > Auth | rbac/mod.rs:161 | 3 |
| F25 | Inactive→Active without hw re-validation | Low | Correctness | raft/state.rs:289 | 4 |
| F28 | 60s minimum window clamp prevents testing | Low | Correctness | commit/mod.rs:56 | 6 |
| F29 | Grace period uses wall-clock, not monotonic | Low | Correctness | conflict/mod.rs:126 | 6 |
| F32 | Boot overlay data not size-limited | Low | Robustness | raft/state.rs:216 | 7 |

## Resolved findings

| # | Title | Severity | Resolution | Resolved in |
|---|-------|----------|------------|-------------|
| F9 | Role extraction bypass | Critical | Replaced with JWT validation via HmacTokenValidator | enrollment_service.rs refactor |
| F10 | require_auth no validation | Critical | validate_token() now decodes JWT, checks sig/expiry/aud/iss | enrollment_service.rs refactor |
| F15 | Raw JWT logged as principal | Medium | extract_principal removed; Identity.principal comes from JWT sub claim | enrollment_service.rs refactor |
| F1 | sysctl read-only | Medium | Reclassified as state_changing: true | whitelist.rs |
| F2 | ip read-only | Medium | Reclassified as state_changing: true | whitelist.rs |
| F3 | mount read-only | Medium | Reclassified as state_changing: true | whitelist.rs |
| F4 | exec PATH includes /usr/sbin | Low | PATH restricted to /usr/bin:/bin | exec.rs |
| F5 | No argument validation | High | validate_args() blocks sensitive paths + path traversal | whitelist.rs, mod.rs |
| F24 | DecideApproval no self-check | Medium | Added self-approval check at Raft layer | raft/state.rs |
| F31 | gRPC no resource limits | Medium | Added concurrency_limit_per_connection + window sizes | journal main.rs |

## Priority fix order

1. **F9 + F10 (Critical)** — wire JWT validation into enrollment service. The `shell/auth.rs` module already has `validate_token()` and `validate_token_with_jwks()`. Reuse for enrollment auth.
2. **F5 (High)** — add argument-level restrictions or file path allowlist for whitelisted commands.
3. **F11 (High)** — implement CSR parsing to extract agent's public key.
4. **F1-F3 (Medium)** — reclassify sysctl/ip/mount as state-changing.
5. **F24 + F21 (Medium)** — add approver identity validation at Raft layer.
6. **F31 (Medium)** — add gRPC resource limits.
