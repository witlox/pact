# ADR-013: Two-Person Approval as Stateful Raft Entries

## Status

Accepted

## Context

Regulated vClusters (e.g., sensitive-data, compliance-governed workloads) require
two-person approval for state-changing operations. This must work within pact's
Raft-based journal architecture and produce an immutable audit trail.

## Decision

Model two-person approval as a **state machine stored in Raft**:

```
Pending → Approved (by different identity)
Pending → Rejected (by any authorized identity)
Pending → Expired (by timeout)
```

### Key rules

1. **Distinct identities (PAuth5)**: the approver's token identity must differ
   from the requester's. Same-identity approval is rejected regardless of token
   freshness.

2. **Configurable timeout (P5)**: pending requests expire after a configurable
   timeout (default 30 minutes). Expired requests cannot be approved — the
   requester must re-submit. This prevents stale approvals from being
   rubber-stamped days later.

3. **Raft persistence**: `PendingApproval` records are written to Raft via
   `CreateApproval` and decided via `DecideApproval`. This gives them the same
   durability and replication guarantees as config entries.

4. **Fail-closed during partition (P7, F1)**: two-person approval requires Raft
   writes. During quorum loss or policy unreachability, approval requests are
   denied — not deferred.

### Scope

Two-person approval applies when `VClusterPolicy.two_person_approval = true`.
Operations covered: commit, rollback, exec (on regulated vClusters), emergency
mode start/end.

## Consequences

- Full audit trail: every approval request, decision, and timeout is a Raft
  entry visible in `pact log`.
- No approval possible during partitions — operators must use emergency mode
  (ADR-004) which has its own audit trail.
- Timeout prevents stale approvals but requires requester to be present for
  re-submission.
- Distinct-identity check prevents a single compromised credential from
  self-approving.

## Alternatives Considered

- **External approval system (Slack bot, PagerDuty)**: rejected — adds external
  dependency to the critical path; pact should be self-contained for core
  operations.
- **Deferred approval during partition**: rejected — cannot verify second
  identity without journal; deferred approval could be replayed after the
  security context has changed.

## References

- `specs/invariants.md` (P4, P5, PAuth5)
- `specs/failure-modes.md` (F1: Journal quorum loss)
- `specs/domain-model.md` (PendingApproval entity, ApprovalStatus enum)
