# Fidelity Report: boot_sequence.feature

Last scan: 2026-03-20
Feature file: `crates/pact-acceptance/features/boot_sequence.feature`
Step definitions: `crates/pact-acceptance/tests/steps/boot.rs`

## Scenarios: 12

| # | Scenario | Depth | Notes |
|---|----------|-------|-------|
| 1 | Agent starts and authenticates to journal | SHALLOW | Checks `boot_phases_completed.contains("auth")` — a flag set by the WHEN step itself. Not testing real mTLS auth. `then_auth_identity` is a no-op (line 343). |
| 2 | Agent pulls vCluster overlay (Phase 1) | MODERATE | `simulate_boot_stream` reads real `JournalState.overlays`, constructs `BootStreamChunk` enum, asserts overlay chunk exists. Overlay data checked non-empty but not verified as valid sysctl/mount config. |
| 3 | Agent pulls node delta (Phase 2) | MODERATE | Asserts `BootStreamChunk::NodeDelta` present after committed entry exists. Delta data is real (from journal entries), but no assertion that delta is actually applied to system state. |
| 4 | Agent starts services in dependency order | THOROUGH | Sorts `ServiceDecl` by `order` field and asserts first/second/last. Tests the real ordering logic against real `ServiceDecl` structs from the feature table. |
| 5 | Agent reports capabilities after services start | SHALLOW | Asserts `world.manifest_written` and `world.socket_available` — both set to `true` by the WHEN step (`when_agent_completes_boot`, line 213-214). Self-fulfilling. |
| 6 | Agent starts config subscription after boot | MODERATE | Asserts `world.subscriptions` is non-empty. Subscription is inserted by WHEN step with real sequence number from journal. From_sequence check is trivially true. |
| 7 | First boot for new vCluster triggers on-demand overlay | THOROUGH | Tests the on-demand path in `simulate_boot_stream`: when no overlay exists, one is built and stored via `JournalCommand::SetOverlay`. Then asserts it exists in `journal.overlays`. |
| 8 | Agent boots with cached config when journal unreachable | MODERATE | Sets `journal_reachable = false`, but `simulate_boot_stream` still reads from `world.journal.overlays` (which is in-memory). Doesn't test actual network failure behavior. The "retry in background" assertion just checks the flag is still false. |
| 9 | Committed node deltas persist across reboots | MODERATE | Sets up a commit entry with state_delta, reboots (clears boot_phases, re-streams), then checks delta content in stream chunks. Tests data flow through journal correctly but "persist across reboots" is not a real persistence test. |
| 10 | Agent stays within resource budget when active | STUB | `then_rss_limit` and `then_cpu_limit` are no-ops (lines 623-632). Comments say "not testable in-process." |
| 11 | Agent uses more CPU when idle | STUB | Same — no-op assertions. |
| 12 | Agent CPU during drift evaluation stays bounded | STUB | Same — no-op assertion. |

## Given Step Analysis

| Step | Setup Method | Notes |
|------|-------------|-------|
| `a journal with default state` | `PactWorld::default()` | Creates in-memory JournalState — faithful to real data structures |
| `a boot overlay for vCluster...` | `JournalCommand::SetOverlay` | Goes through real journal state machine |
| `a committed node delta for node...` | `JournalCommand::AppendEntry` | Real journal entry with real StateDelta |
| `a boot overlay with services:` | Direct `ServiceDecl` construction | Builds real ServiceDecl structs from table |
| `the journal is unreachable` | Sets flag `journal_reachable = false` | Does NOT disconnect anything — overlay data still accessible in-memory |
| `cached config exists...` | Inserts overlay into journal | Simulates cache as "overlay exists" — doesn't test real filesystem caching |

## Summary

- **THOROUGH**: 2 (service ordering, on-demand overlay)
- **MODERATE**: 5 (overlay streaming, delta streaming, subscription, cached boot, reboot persistence)
- **SHALLOW**: 2 (auth, capability report)
- **STUB**: 3 (resource budget — all no-ops)
- **Confidence: LOW** (17% THOROUGH+, 3 STUBs on resource budget contract)

## Critical Gaps

1. **mTLS authentication is not tested at all** — auth step just pushes a string to a Vec. Impact: HIGH (security-critical path).
2. **Resource budget scenarios are complete stubs** — the 50MB RSS / 0.5% CPU contracts have zero enforcement. Impact: MEDIUM (operational contract, not a correctness issue).
3. **"Overlay applied" just checks a flag** — no verification that sysctl/mount/module config was actually written to system state. Impact: MEDIUM (boot correctness).
4. **Cached config with journal unreachable** still reads from in-memory journal — doesn't test real partition behavior. Impact: HIGH (resilience-critical path, ties to invariant A9).
