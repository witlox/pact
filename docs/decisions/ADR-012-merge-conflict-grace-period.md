# ADR-012: Merge Conflict Grace Period with Journal-Wins Fallback

## Status

Accepted

## Context

During network partitions, agents may accumulate local state changes (admin
reconfigurations via pact shell, emergency mode, manual intervention). On
reconnect, these local changes may conflict with journal state on the same
config keys.

The system must balance correctness (don't silently overwrite admin work) with
availability (the node must eventually converge to declared state).

## Decision

Implement a **three-phase conflict resolution** protocol:

### Phase 1: Feed-back (CR1)
On reconnect, the agent reports unpromoted local drift to the journal BEFORE
accepting the journal's current state. This ensures no local changes are lost.

### Phase 2: Pause and flag (CR2)
If local changes conflict with journal state on the same keys, the agent pauses
convergence for those keys and flags a merge conflict. Non-conflicting keys sync
normally. The node remains operational but not fully converged.

### Phase 3: Grace period with fallback (CR3)
Admin has a grace period (default: commit window duration) to resolve conflicts
via `pact diff` and `pact commit`. If unresolved within the grace period, the
system falls back to **journal-wins**: the journal's declared state overwrites
local changes. All overwritten values are logged for audit.

### Admin notification (CR5)
If an admin has an active CLI session when their changes are overwritten (by
grace period timeout or by a concurrent promote), they are notified in-session.

### Promote integration (CR4)
When promoting node-level changes to a vCluster overlay, conflicting keys on
target nodes require explicit acknowledgment: accept (keep local as per-node
delta) or overwrite (apply promoted value).

## Consequences

- No silent data loss: local changes are always fed back and logged before any
  overwrite.
- Availability preserved: grace period timeout ensures convergence eventually
  happens even without admin intervention.
- Admin agency: operators get time to review and decide, not just informed after
  the fact.
- Complexity: agent must track per-key conflict state and grace period timers.

## Alternatives Considered

- **Immediate journal-wins**: rejected — silently discards admin work done during
  partition, violates trust in the audit trail.
- **Require manual resolution always**: rejected — node never converges if admin
  is unavailable (vacation, off-hours).
- **Last-write-wins by timestamp**: rejected — clock skew between agent and
  journal makes this unreliable; admin-committed changes should take precedence
  over auto-converge.

## References

- `specs/invariants.md` (CR1–CR5)
- `specs/failure-modes.md` (F13: Merge conflict on reconnect, F14: Promote conflicts)
- `specs/assumptions.md` (A-Q2: Partition conflict replay, A-C2: Timestamp ordering)
