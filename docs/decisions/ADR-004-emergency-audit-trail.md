# ADR-004: Emergency Mode Preserves Audit Trail

## Status: Accepted

## Decision

Emergency mode: extended commit window (configurable, default 4h) + suspended
auto-rollback + continuous logging + mandatory commit-or-rollback at session end.

Stale emergency (timer expires without --end): alert, scheduling hold, no auto-rollback.

Audit trail is never interrupted, including during emergencies.
