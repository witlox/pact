# Chunk 7: Supply Chain + Resource Exhaustion + Cross-Cutting

Reviewed: 2026-03-20

---

## Finding: F30 — No cargo-deny or cargo-audit in CI
Severity: Medium
Category: Security > Supply chain
Location: `justfile` (CI pipeline)
Spec reference: None
Description: The `just ci` command runs `fmt + clippy + deny + test`, and `deny.toml` exists in the repo root, but `cargo-deny` is not installed in the current environment. If CI doesn't have it either, dependency vulnerability scanning is not enforced. The workspace has 36 direct dependencies for pact-agent alone.
Evidence: `cargo deny check advisories` returns "no such command".
Suggested resolution: Verify cargo-deny is installed in CI. If not, add it to the CI setup step.

---

## Finding: F31 — gRPC services have no connection limits or request size limits
Severity: Medium
Category: Robustness > Resource exhaustion
Location: `crates/pact-journal/src/main.rs`, `crates/pact-agent/src/main.rs`
Spec reference: None
Description: The tonic gRPC servers don't configure `max_concurrent_streams`, `max_frame_size`, `max_recv_message_size`, or connection limits. A malicious client could:
1. Open thousands of concurrent streams (memory exhaustion)
2. Send very large messages (e.g., giant CSR in enrollment, huge overlay data)
3. Open connections without sending data (connection exhaustion)

The enrollment endpoint is particularly exposed since it's server-TLS-only (not mTLS) and accessible to unauthenticated agents.
Evidence: `Server::builder()` in main.rs files — no resource limits configured.
Suggested resolution: Add tonic server configuration: `max_concurrent_streams(100)`, `max_recv_message_size(4 * 1024 * 1024)` (4MB), and connection idle timeout.

---

## Finding: F32 — Boot overlay data not size-limited
Severity: Low
Category: Robustness > Resource exhaustion
Location: `crates/pact-journal/src/raft/state.rs:216-228`
Spec reference: I2 (overlay size ~100-200KB)
Description: `SetOverlay` validates the checksum but not the data size. The spec states overlays should be ~100-200KB, but there's no enforcement. A platform admin could push a multi-GB overlay that would be replicated to all journal nodes via Raft and streamed to every booting agent.
Evidence: No size check in `SetOverlay` handler.
Suggested resolution: Add a maximum overlay size check (e.g., 10MB) with a clear error message.

---

## Summary

| Severity | Count |
|----------|-------|
| Medium | 2 (F30: supply chain, F31: gRPC limits) |
| Low | 1 (F32: overlay size) |
| **Total** | **3** |
