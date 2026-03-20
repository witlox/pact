# Fidelity Report: boot_config_streaming.feature

Last scan: 2026-03-20
Feature file: `crates/pact-acceptance/features/boot_config_streaming.feature`
Step definitions: `crates/pact-acceptance/tests/steps/boot.rs`

## Scenarios: 11

| # | Scenario | Depth | Notes |
|---|----------|-------|-------|
| 1 | Stream base overlay for a vCluster | MODERATE | `simulate_boot_stream` reads real `JournalState.overlays`, produces `BootStreamChunk::BaseOverlay`. Asserts chunk exists and data non-empty. Does NOT assert data matches the specific string "sysctl config" from the Given step. |
| 2 | Overlay includes version and checksum | THOROUGH | Asserts `overlay version == 3` from chunk. Asserts checksum is non-empty. `BootOverlay::new` computes a real SHA-256 checksum. Tests real version propagation and real checksum generation. |
| 3 | Stream node delta after overlay | THOROUGH | Creates overlay + committed node delta entry. Asserts both BaseOverlay and NodeDelta chunks present. Delta data contains real kernel change key/value from the committed entry. |
| 4 | Node without committed deltas gets overlay only | THOROUGH | Asserts BaseOverlay present and NodeDelta absent. Tests the branching logic in `simulate_boot_stream` correctly. |
| 5 | Boot stream ends with ConfigComplete message | THOROUGH | Asserts last chunk is `BootStreamChunk::Complete`. Real structural verification. |
| 6 | ConfigComplete includes base version | THOROUGH | Asserts `base_version > 0` from the Complete chunk. Tests version propagation through the stream. |
| 7 | Overlay rebuilt when config committed | THOROUGH | Appends a real Commit entry, rebuilds overlay with incremented version. Asserts overlay exists and version > 1. Tests the real rebuild/version-increment path via `JournalCommand::SetOverlay`. |
| 8 | Overlay built on demand when not cached | THOROUGH | Starts with no overlay. `when_node_requests_boot` creates one on demand via `JournalCommand::SetOverlay`. Asserts it now exists. Tests the real on-demand build path. |
| 9 | Agent receives config update after boot | MODERATE | Subscription inserted by setup. Update appended and pushed to `received_updates` Vec. Asserts subscription exists and updates non-empty. The notification is simulated (not real gRPC streaming). |
| 10 | Agent reconnects with last known sequence | MODERATE | Sets subscription `from_sequence = 5`, appends 3 updates. Asserts subscription has correct sequence and updates include sequence >= 5. Subscription filtering logic not tested (just Vec push). |
| 11 | Policy/blacklist change delivered | MODERATE | Same pattern as #9 — event type distinguished by string tag. Asserts correct `update_type` string. Verifies event categorization but not real delivery. |

## Given Step Analysis

| Step | Setup Method | Notes |
|------|-------------|-------|
| `a boot overlay for vCluster X version N with data Y` | `JournalCommand::SetOverlay` | Real journal state machine, real BootOverlay construction with SHA-256 checksum |
| `a committed node delta for node X with kernel change Y to Z` | `JournalCommand::AppendEntry` | Real ConfigEntry with real StateDelta |
| `node X is subscribed to config updates from sequence N` | Direct insert into `world.subscriptions` | Bypasses real subscription setup |

## Summary

- **THOROUGH**: 7 (version, checksum, delta presence/absence, stream structure, overlay lifecycle)
- **MODERATE**: 4 (overlay data match, subscription notifications)
- **SHALLOW**: 0
- **STUB**: 0
- **Confidence: HIGH** (64% THOROUGH — technically MODERATE by the >80% threshold, but the MODERATE scenarios test real data flow, just not real gRPC transport)

**Revised Confidence: MODERATE** (applying strict >80% threshold)

## Critical Gaps

1. **Config subscription is simulated, not tested via real streaming** — updates are manually pushed to a Vec, not received through gRPC. Impact: MEDIUM (integration concern, not unit-level).
2. **Overlay data content not validated** — scenario 1 provides "sysctl config" data but `then_overlay_data_matches` only checks `!data.is_empty()`. Impact: LOW (structural tests are solid, this is a minor assertion gap).
