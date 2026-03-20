# Chunk 6: Commit Window + Conflict Resolution

Reviewed: 2026-03-20
Files: pact-agent/src/commit/mod.rs, pact-agent/src/conflict/mod.rs

---

## Finding: F28 — Commit window minimum clamp at 60s prevents testing sub-minute windows
Severity: Low
Category: Correctness > Edge cases
Location: `crates/pact-agent/src/commit/mod.rs:56`
Spec reference: ADR-014 (commit windows)
Description: `calculate_window_seconds()` clamps the result to at least 60 seconds: `window.max(60.0) as u32`. This means even with very high drift magnitude, the window never goes below 1 minute. This is a reasonable safety measure for production but prevents testing real expiry behavior in BDD tests (the test would need to wait 60s).
Evidence: Line 56: `window.max(60.0) as u32`.
Suggested resolution: Consider a test-mode flag that allows sub-minute windows, or accept this as a design constraint and test the formula + expiry logic separately (as currently done).

---

## Finding: F29 — ConflictManager grace period is wall-clock based, not monotonic
Severity: Low
Category: Correctness > Edge cases
Location: `crates/pact-agent/src/conflict/mod.rs:126`
Spec reference: ADR-012 (merge conflict grace period)
Description: `check_grace_periods()` compares `Utc::now()` against `entry.detected_at + grace_period`. If the system clock is adjusted backward (e.g., NTP correction), conflicts could appear to never expire. Conversely, a forward clock jump could expire conflicts prematurely.
Evidence: Line 126: `now >= entry.detected_at + self.grace_period`.
Suggested resolution: Use `tokio::time::Instant` (monotonic) for local timing, and `DateTime<Utc>` only for journal-persisted timestamps. This is a minor concern since HPC nodes typically run NTP/PTP with minimal clock skew.

---

## Summary

| Severity | Count |
|----------|-------|
| Findings | 2 (both Low) |

The commit window and conflict resolution code is well-structured. The formula, lifecycle, and grace period logic are correct. Unit tests cover all edge cases (empty, boundary, partial resolution, mixed expiry). No concurrency issues — both `CommitWindowManager` and `ConflictManager` are single-threaded (not `Send+Sync`, used within agent's main loop).
