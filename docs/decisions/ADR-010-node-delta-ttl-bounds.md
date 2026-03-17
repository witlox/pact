# ADR-010: Per-Node Delta TTL Bounds (15 minutes – 10 days)

## Status

Accepted

## Context

Nodes within a vCluster are expected to be homogeneous (A-H1). Per-node deltas
are temporary exceptions — debugging overrides, hardware-specific workarounds,
or staged rollouts. Without time bounds, deltas accumulate indefinitely, eroding
homogeneity and causing scheduling correctness issues.

## Decision

Enforce hard TTL bounds on per-node configuration deltas:

- **Minimum: 15 minutes** — long enough for a debugging session, short enough
  to force a decision before the next shift.
- **Maximum: 10 days** — carries over weekends with margin, forces periodic
  review, prevents forgotten deltas from silently accumulating.

TTL is validated at `AppendEntry` time in the Raft state machine. Values outside
bounds are rejected with a descriptive error (ND1, ND2).

### Homogeneity warning (ND3)

The system **warns** (does not enforce) when per-node deltas cause nodes within
a vCluster to diverge from the overlay. Heterogeneity is surfaced in `pact status`
and `pact diff` output. Operators decide whether to promote (make vCluster-wide)
or revert.

## Consequences

- Deltas cannot persist indefinitely — operators must commit, promote, or let
  them expire.
- Scheduling correctness is protected: lattice can assume vCluster homogeneity
  within the TTL window.
- Promote workflow (F14) must handle conflicts when target nodes have local
  changes on the same keys.
- Warning-only for heterogeneity avoids blocking legitimate node-specific config
  (e.g., GPU firmware workarounds).

## References

- `specs/invariants.md` (ND1, ND2, ND3)
- `specs/assumptions.md` (A-Q3, A-H1)
- `specs/failure-modes.md` (F14: Promote conflicts)
