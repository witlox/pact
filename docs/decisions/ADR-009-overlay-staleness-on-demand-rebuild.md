# ADR-009: Overlay Staleness Detection and On-Demand Rebuild

## Status

Accepted

## Context

Boot overlays are pre-computed compressed config bundles served to agents during
boot. When a vCluster config is committed, the overlay must be rebuilt. If a node
boots between the config commit and the overlay rebuild, it would receive stale
configuration.

At scale (1000+ nodes booting simultaneously after a power event), overlay
freshness is critical to boot correctness.

## Decision

Use a **hybrid proactive + reactive** overlay rebuild strategy:

1. **Proactive**: rebuild overlay on every config commit (covers steady-state).
2. **Reactive**: detect staleness at serve time by comparing overlay version to
   latest config sequence. If stale, trigger on-demand rebuild before serving.

Staleness detection is cheap (version comparison). On-demand rebuild adds latency
only for the first boot after a config change; subsequent boots use the cached
rebuild.

### Overlay consistency (J5)

`BootOverlay.checksum` is a deterministic hash of `BootOverlay.data`. The Raft
state machine validates the checksum on `SetOverlay` — any mismatch is rejected.
This ensures all replicas serve identical overlays.

## Consequences

- First boot after config commit may be slightly slower (~50-100ms rebuild).
- All subsequent boots use cached overlay (no penalty).
- No window where stale config can be served to a booting node.
- Overlay rebuild is idempotent — concurrent rebuild requests produce the same
  result.

## References

- `specs/failure-modes.md` (F9: Stale overlay)
- `specs/invariants.md` (J5: Overlay consistency)
- `specs/architecture/enforcement-map.md` (J5 row)
