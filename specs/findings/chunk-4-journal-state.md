# Chunk 4: Journal State Machine

Reviewed: 2026-03-20
Files: pact-journal/src/raft/state.rs, pact-journal/src/raft/types.rs

---

## Finding: F22 — Audit log is an unbounded Vec with no pruning
Severity: Medium
Category: Robustness > Resource exhaustion
Location: `crates/pact-journal/src/raft/state.rs:31`
Spec reference: O3 (audit continuity)
Description: `audit_log: Vec<AdminOperation>` grows unboundedly. Every exec, shell session, service operation, enrollment attempt appends to this Vec. Over the lifetime of a deployment (months/years), this will consume increasing memory and make Raft snapshots progressively larger. There's no rotation, compaction, or archival mechanism.
Evidence: `RecordOperation(op)` (line 230-232) pushes to `self.audit_log` unconditionally.
Suggested resolution: Add a retention limit (e.g., keep last 10,000 entries in Raft state, archive older entries to Loki/external storage). Or implement log compaction in Raft snapshots.

---

## Finding: F23 — Config entries never pruned from BTreeMap
Severity: Medium
Category: Robustness > Resource exhaustion
Location: `crates/pact-journal/src/raft/state.rs:21`
Spec reference: J2 (immutability)
Description: `entries: BTreeMap<EntrySeq, ConfigEntry>` preserves all entries forever. This is by design (immutable log, J2), but without compaction or snapshotting, memory grows linearly with operations. A busy cluster producing 100 entries/hour would accumulate ~2.6M entries/year.
Evidence: No code path removes entries from the BTreeMap.
Suggested resolution: Implement Raft log compaction via snapshots. The immutable log can be archived to disk while only keeping recent entries in memory. This is a standard Raft optimization and doesn't violate J2 (entries are preserved, just not all in memory).

---

## Finding: F24 — DecideApproval does not validate approver role
Severity: Medium
Category: Security > Authorization
Location: `crates/pact-journal/src/raft/state.rs:243-261`
Spec reference: ADR-013 (two-person approval)
Description: `DecideApproval` in the Raft state machine accepts any `approver` Identity and applies the decision without checking:
1. Whether the approver is the same as the requester (self-approval)
2. Whether the approver has sufficient privileges

The self-approval check exists in `DefaultPolicyEngine::approve()` but NOT in the Raft command handler. If a client submits `DecideApproval` directly to Raft (bypassing the PolicyService), self-approval or unprivileged approval would succeed.
Evidence: Lines 243-261 — `approval.approver = Some(approver)` with no identity checks.
Suggested resolution: Add self-approval check in the Raft apply handler: `if approval.requester.principal == approver.principal { return ValidationError }`. The Raft layer should enforce invariants independently of the service layer.

---

## Finding: F25 — Enrollment state machine allows Inactive→Active without re-validation
Severity: Low
Category: Correctness > Specification compliance
Location: `crates/pact-journal/src/raft/state.rs:289-319`
Spec reference: ADR-008 (enrollment lifecycle)
Description: `ActivateNode` transitions nodes from `Registered` or `Inactive` to `Active`. It checks for `Revoked` and `Active` states (rejecting both). But there's no re-validation of hardware identity during re-activation from `Inactive` state — the `ActivateNode` command doesn't receive hardware identity, only `node_id`. The enrollment service pre-checks hardware identity before submitting the Raft command, so this is safe in normal flow. But a direct Raft client could activate any node by ID without presenting matching hardware.
Evidence: `ActivateNode { node_id, cert_serial, cert_expires_at }` — no hardware identity in the command.
Suggested resolution: Accept as low risk — Raft clients are internal (journal quorum). But consider adding an optional `hw_key` field to `ActivateNode` for defense-in-depth.

---

## Summary

| Severity | Count |
|----------|-------|
| Critical | 0 |
| High | 0 |
| Medium | 3 (F22: audit log, F23: entries, F24: approval bypass) |
| Low | 1 (F25: re-activation) |
| **Total** | **4** |

The journal state machine is solid — it validates authorship (J3), parent chains (J4), TTL bounds (ND1/ND2), overlay checksums (J5), and enrollment states (E1/E2/E7). The main gaps are resource management (unbounded growth) and an invariant enforcement gap in DecideApproval (self-approval check missing at Raft layer).
