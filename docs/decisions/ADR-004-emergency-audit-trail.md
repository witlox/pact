# ADR-004: Emergency Mode Preserves Audit Trail

## Status: Accepted

## Decision

Emergency mode: extended commit window (configurable, default 4h) + suspended
auto-rollback + continuous logging + mandatory commit-or-rollback at session end.

Stale emergency (timer expires without --end): alert (Loki event + Grafana
alert rule), scheduling hold (lattice cordon via API), no auto-rollback.
Another admin with `pact-ops-{vcluster}` or `pact-platform-admin` role can
force-end a stale emergency with `pact emergency --end --force`.

Audit trail is never interrupted, including during emergencies.

## Shell Restrictions During Emergency

Emergency mode does **not** expand the pact shell whitelist (PATH restriction).
The restricted bash environment remains the same as normal operation. If the
admin needs binaries outside the whitelist, they have two options:

1. Use `pact exec` for specific non-whitelisted commands (platform admins
   can bypass the exec whitelist, though still logged)
2. Access the node via BMC console, which provides unrestricted bash

Emergency mode changes default to a TTL matching the emergency window duration.
When the TTL expires, uncommitted changes are rolled back.
