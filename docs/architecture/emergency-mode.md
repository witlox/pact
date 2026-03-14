# Emergency Mode Design

## Purpose

Emergency mode provides an extended operational window when immediate human
intervention is required on a node. It suspends automatic drift rollback
while maintaining full audit logging (ADR-004).

## Lifecycle

```
Normal → EmergencyStart(reason, admin) → Emergency Active → EmergencyEnd(admin) → Normal
                                              │
                                         Extended commit window (4h default)
                                         Auto-converge suspended
                                         All actions still logged
                                         Shell whitelist NOT expanded
```

## Entry Conditions

| Actor | Can Enter? | Can Exit? |
|-------|------------|-----------|
| Human admin (ops/platform) | Yes | Own emergency or with --force |
| AI agent (pact-service-ai) | **No** (P8) | **No** (P8) |
| Service agent | **No** | **No** |

## Configuration

```toml
[agent.commit_window]
emergency_window_seconds = 14400  # 4 hours (default)
```

## Audit Trail

Both entry and exit are recorded as immutable journal entries:

```
EntryType::EmergencyStart { reason, admin_identity, timestamp }
EntryType::EmergencyEnd { admin_identity, timestamp }
```

## Stale Emergency Detection

If an emergency exceeds its window without being resolved:
- Alert generated for platform admins
- Emergency remains active (no auto-exit)
- Only platform admin can force-end

## What Emergency Mode Does NOT Do

- Does **not** expand the shell whitelist (security invariant)
- Does **not** bypass RBAC authorization
- Does **not** suppress audit logging
- Does **not** allow untracked changes
- Does **not** grant additional privileges
