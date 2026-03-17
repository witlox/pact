# ADR-011: Degraded-Mode Policy Evaluation

## Status

Accepted

## Context

When the agent loses connectivity to the journal (network partition, journal
overload), it cannot perform full OPA policy evaluation. The system must decide
which operations to allow and which to deny during the partition.

The tension is between availability (let operators work on nodes during outages)
and security (don't allow unauthorized operations just because the policy engine
is unreachable).

## Decision

Adopt a **tiered fail-closed** strategy based on operation complexity:

| Operation Type | Degraded Behavior | Rationale |
|---|---|---|
| Whitelist commands (exec) | **Allowed** — cached whitelist honored | Low risk, needed for diagnostics |
| Basic RBAC checks | **Allowed** — cached role bindings honored | Enables routine ops during partition |
| Platform admin operations | **Allowed** — cached role check (logged) | Admin must be able to act in emergencies |
| Two-person approval | **Denied** — fail-closed | Cannot verify second approver identity |
| Complex OPA rules | **Denied** — fail-closed | Cannot evaluate external policy state |

All degraded-mode authorization decisions are logged locally. On reconnect,
local logs are replayed to the journal for audit continuity.

## Consequences

- Operators can run diagnostics and basic ops during partitions.
- Regulated operations (two-person approval) are blocked — operators must wait
  for connectivity or use emergency mode (which has its own audit trail).
- No silent privilege escalation: complex rules default to deny, not allow.
- Audit trail has no gaps: local logging bridges the partition.

## Alternatives Considered

- **Fail-open for everything**: rejected — security risk for regulated vClusters.
- **Fail-closed for everything**: rejected — operators locked out during outages,
  which is when they most need access.
- **Cache full OPA state locally**: rejected — OPA rule evaluation may depend on
  external data sources (Sovra, compliance databases) that are also unreachable.

## References

- `specs/invariants.md` (P7: Degraded mode restrictions)
- `specs/failure-modes.md` (F2: PolicyService unreachable)
- `specs/assumptions.md` (A-C3: Cached policy sufficiency)
