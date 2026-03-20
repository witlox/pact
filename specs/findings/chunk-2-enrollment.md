# Chunk 2: Enrollment Attack Surface

Reviewed: 2026-03-20
Files: enrollment_service.rs, ca.rs, rate_limiter.rs, raft/state.rs (enrollment state machine)

---

## Finding: F9 — `extract_role()` uses heuristic token inspection instead of JWT decoding
Severity: Critical
Category: Security > Authentication & authorization
Location: `crates/pact-journal/src/enrollment_service.rs:107-129`
Spec reference: OIDC role model (CLAUDE.md)
Description: `extract_role()` extracts the user's role via two mechanisms: (1) an `x-pact-role` metadata header, or (2) string pattern matching on the Bearer token value. Neither decodes the JWT or validates the signature. This means:
- **Any client can set `x-pact-role: pact-platform-admin`** in gRPC metadata and gain full admin access to enrollment operations (RegisterNode, DecommissionNode, AssignNode).
- The token pattern matching (`token.contains("platform-admin")`) would match any token containing that substring, regardless of whether it's a valid JWT.
- `require_platform_admin()` calls `require_auth()` first, which only checks that a Bearer token *exists* — it does not validate it.

Evidence: Setting gRPC metadata `x-pact-role: pact-platform-admin` bypasses all RBAC checks. No JWT signature validation occurs.
Suggested resolution: Decode and validate the JWT (using `shell/auth.rs::validate_token()` which already exists), then extract the role from verified claims. Remove `x-pact-role` header trust or restrict it to internal-only (e.g., from a validated auth gateway).

---

## Finding: F10 — `require_auth()` only checks token presence, not validity
Severity: Critical
Category: Security > Authentication
Location: `crates/pact-journal/src/enrollment_service.rs:53-65`
Spec reference: P1 (every operation authenticated)
Description: `require_auth()` checks that the request has an `authorization` header starting with `Bearer `, but does NOT decode, verify signature, check expiry, or validate the token in any way. Any string starting with `Bearer ` passes authentication. This affects all authenticated enrollment RPCs: RegisterNode, AssignNode, MoveNode, UnassignNode, DecommissionNode, BatchRegister, ListNodes, InspectNode.
Evidence: `require_auth()` returns `Ok(())` for `Bearer anything-at-all`.
Suggested resolution: Wire `require_auth()` through the real token validation in `shell/auth.rs` (which does JWT decoding, signature verification, expiry, audience, and issuer checks).

---

## Finding: F11 — CSR public key not extracted from agent's CSR
Severity: High
Category: Security > Cryptography
Location: `crates/pact-journal/src/ca.rs:114-152`
Spec reference: ADR-008 (E4: CSR, private keys never in journal)
Description: `sign_csr()` receives the agent's CSR (`_csr_der`) but ignores it entirely. Instead, it generates a new keypair server-side (line 130-131) and creates a certificate with that key. This means:
1. The agent's private key is NOT the key embedded in the certificate — the agent cannot use the returned cert for mTLS because it doesn't have the corresponding private key.
2. A new private key is generated on the journal server and immediately discarded (line 130) — nobody has the private key for the certificate.
3. The comment on line 108-113 acknowledges this as a limitation of rcgen 0.13's CSR parsing.

This is a known limitation, not a vulnerability in the current state (the cert can't be used for mTLS anyway), but it means the enrollment flow is fundamentally incomplete — agents cannot establish mTLS connections with the returned certificates.
Evidence: `_csr_der` parameter is unused. `node_key = KeyPair::generate()` on line 130-131.
Suggested resolution: Implement CSR parsing (rcgen 0.13 `CertificateSigningRequestParams::from_der()` or switch to `x509-cert` crate) to extract the agent's public key and embed it in the signed certificate.

---

## Finding: F12 — Rate limiter is per-process, not per-source-IP
Severity: Medium
Category: Security > Input validation
Location: `crates/pact-journal/src/rate_limiter.rs:7-47`
Spec reference: Enrollment endpoint rate limiting (node_enrollment.feature)
Description: The `RateLimiter` uses a single global token bucket. All enrollment requests share the same bucket regardless of source. A legitimate burst of 100 enrollments from one subnet would exhaust the bucket and block legitimate enrollments from other subnets. This is a DoS amplification vector — one attacker can block all enrollment.
Evidence: Single `Mutex<RateLimiterInner>` with no IP-based bucketing.
Suggested resolution: Implement per-source-IP rate limiting (e.g., `HashMap<IpAddr, RateLimiterInner>`), or use a two-tier approach (global limit + per-IP limit).

---

## Finding: F13 — TOCTOU in enrollment state check
Severity: Low
Category: Correctness > Concurrency
Location: `crates/pact-journal/src/enrollment_service.rs:200-221`
Spec reference: ADR-008 (once-Active rejection)
Description: The enrollment state is checked on lines 200-221 by reading from `state.read()`, then dropped, then a Raft write is issued. Between the read and the write, another concurrent enrollment could change the state. However, the code explicitly handles this (comment on line 223-225): the Raft state machine performs the authoritative check, so if two concurrent enrollments race, only the Raft winner succeeds. The pre-flight check is an optimization to avoid unnecessary Raft writes.
Evidence: Lines 200-221 read state, drop lock, then write to Raft on 228-237.
Suggested resolution: None needed — the Raft state machine is the authoritative check. The comment documents this intentionally.

---

## Finding: F14 — Ephemeral CA key lost on journal restart
Severity: Medium
Category: Robustness > Failure cascades
Location: `crates/pact-journal/src/ca.rs:23-49`
Spec reference: ADR-008 (certificate lifecycle)
Description: When using `CaKeyManager::generate()` (default mode), the CA key exists only in memory. On journal restart, a new CA is generated. This means:
1. All certificates signed by the previous CA become invalid.
2. All agents must re-enroll after a journal restart.
3. During Raft leader failover, the new leader generates a different CA, so certificates signed by the old leader are invalid on the new leader.

The code acknowledges this (comment lines 27-29) and says "agents re-enroll on reconnect, so this is safe." But during a rolling journal update, agents connected to different journal nodes would have certificates from different CAs, breaking cross-node mTLS.
Evidence: `generate()` creates new CA on every call. No CA key persistence or distribution across Raft members.
Suggested resolution: Either persist the CA key to the Raft log (encrypted) so all members share it, or use the `load()` path with a pre-provisioned CA key distributed to all journal nodes. SPIRE deployment eliminates this issue.

---

## Finding: F15 — `extract_principal()` returns the raw token as the principal name
Severity: Medium
Category: Security > Secrets & configuration
Location: `crates/pact-journal/src/enrollment_service.rs:96-101`
Spec reference: O3 (audit continuity)
Description: `extract_principal()` strips the "Bearer " prefix and returns the rest of the token as the principal name. If this is used in audit logs, the raw JWT (which contains claims, signature, and potentially sensitive information) is logged as the actor identity.
Evidence: Line 100: `s.trim_start_matches("Bearer ").to_string()` — this is the full JWT.
Suggested resolution: Decode the JWT to extract `sub` (subject) claim, or use a hash of the token as the principal identifier. Never log raw tokens.

---

## Summary

| Severity | Count |
|----------|-------|
| Critical | 2 (F9: role extraction, F10: auth validation) |
| High | 1 (F11: CSR not parsed) |
| Medium | 3 (F12: rate limiter, F14: ephemeral CA, F15: token in logs) |
| Low | 1 (F13: TOCTOU — mitigated by design) |
| **Total** | **7** |

Highest-risk finding: **F9 + F10 combined** — any client can set `x-pact-role: pact-platform-admin` in gRPC metadata and gain full administrative access to enrollment operations, including enrolling nodes, assigning vClusters, and decommissioning nodes. The Bearer token is not validated.
