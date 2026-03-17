# ADR-014: Optimistic Concurrency with Commit Windows

## Status

Accepted

## Context

HPC clusters require low-latency configuration changes. Traditional config
management (Puppet/Ansible) applies changes synchronously — the operator waits
for convergence before proceeding. For pact, this would block interactive
workflows on nodes that should feel "local."

At the same time, unapplied changes must not drift indefinitely. There must be
a mechanism that forces resolution — either commit the change or roll it back.

## Decision

Use **optimistic concurrency** with time-bounded commit windows:

### Apply immediately

Configuration changes take effect on the node immediately. The operator does
not wait for consensus or convergence. This gives pact shell the responsiveness
of being "on the box."

### Commit window opens

When drift is detected (a change has been applied but not committed to the
journal), a commit window opens. The window duration is:

```
window_seconds = base_window / (1 + drift_magnitude * sensitivity)
```

- `base_window`: default 900 seconds (15 minutes)
- `drift_magnitude`: weighted L2 norm of the drift vector
- `sensitivity`: default 2.0

Larger drift = shorter window. This creates urgency proportional to how much
the node has deviated from declared state.

### Auto-rollback on expiry (A4)

If the commit window expires without an explicit `pact commit`, the system
automatically rolls back to declared state.

**Exception**: emergency mode (ADR-004) suspends auto-rollback. The emergency
window (default 4 hours) replaces the commit window.

### Rollback safety (F5)

Before rolling back, the system checks for active consumers (open file handles,
running processes using the affected resources). If consumers are active, the
rollback fails and the node remains drifted — the admin must resolve manually.

## Consequences

- Interactive config changes feel instant — no waiting for Raft round-trip.
- Time pressure prevents drift accumulation: either commit or lose the change.
- Larger changes get shorter windows, preventing large unreviewed drift.
- Emergency mode provides an escape hatch for extended debugging sessions.
- Active consumer check prevents data loss from premature rollback.

## Alternatives Considered

- **Synchronous apply-after-commit**: rejected — too slow for interactive HPC
  admin workflows; would require SSH-like latency which pact replaces.
- **No auto-rollback (manual only)**: rejected — drift accumulates silently;
  operators forget to commit; nodes diverge from declared state.
- **Fixed window duration**: rejected — a 1-line sysctl change and a 50-mount
  reconfiguration shouldn't have the same urgency.

## References

- `specs/invariants.md` (A3: Commit window formula, A4: Auto-rollback)
- `specs/failure-modes.md` (F5: Rollback with active consumers)
- `CLAUDE.md` ("Optimistic concurrency — changes apply immediately, commit within
  time window")
