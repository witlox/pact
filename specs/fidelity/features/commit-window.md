# Fidelity Report: commit_window.feature

Last scan: 2026-03-20
Feature file: `crates/pact-acceptance/features/commit_window.feature`
Step definitions: `crates/pact-acceptance/tests/steps/commit_window.rs`

## Scenarios: 20

| # | Scenario | Depth | Notes |
|---|----------|-------|-------|
| 1 | Tiny drift gets long commit window | THOROUGH | Calls real `CommitWindowManager::open(0.05)`, reads `WindowState::Open { opened_at, deadline }`, computes duration, asserts ~818s ±10. Tests real formula. |
| 2 | Small drift gets moderate window | THOROUGH | Same — asserts ~692s. |
| 3 | Moderate drift gets shorter window | THOROUGH | Same — asserts ~562s. |
| 4 | Large drift gets short window | THOROUGH | Same — asserts ~346s. |
| 5 | Higher sensitivity compresses windows | THOROUGH | Reconstructs CommitWindowManager with sensitivity=5.0, asserts ~180s. Tests sensitivity parameter effect. |
| 6 | Commit within window succeeds | THOROUGH | Calls `commit_mgr.commit()`, then `JournalCommand::UpdateNodeState`, asserts node state is Committed. Tests real commit path through both CommitWindowManager and JournalState. |
| 7 | Rollback within window succeeds | THOROUGH | Calls `commit_mgr.rollback()`, same journal path. Asserts node state Committed after rollback. |
| 8 | Window expiry triggers automatic rollback | SHALLOW | Reconstructs manager with `base_window_seconds: 0`, calls `open(0.0)`, sets `rollback_triggered = true` manually (line 162). Then asserts that flag. Does NOT test real timer-based expiry. |
| 9 | Rollback checks for active consumers | SHALLOW | Sets `rollback_deferred = true` in WHEN step (line 436), then asserts it. Self-fulfilling flag. No real consumer detection code tested. |
| 10 | Rollback proceeds when no active consumers | SHALLOW | Sets `rollback_deferred = false`, asserts it. Also checks `rollback_triggered` which was set manually. |
| 11 | Drift detection recorded in journal | THOROUGH | Creates real ConfigEntry with DriftDetected type via `JournalCommand::AppendEntry`. Asserts entry exists with correct scope. Real journal state machine. |
| 12 | Commit recorded in journal | THOROUGH | Creates Commit entry with real StateDelta. Asserts entry has state_delta via `then_entry_has_delta`. |
| 13 | Rollback recorded in journal | SHALLOW | `then_rollback_entry` CREATES the Rollback entry it then asserts (commit_window.rs:373-391). Self-fulfilling. |
| 14 | Committed delta without TTL persists | MODERATE | Creates entry with `ttl_seconds: None`, asserts last commit has no TTL. `then_persist_across_reboots` is a no-op (line 307). |
| 15 | Committed delta with TTL expires | SHALLOW | Creates entry with TTL, but `then_delta_expired` is a no-op (line 311). `then_delta_cleaned_up` is a no-op (line 316). No real TTL expiry code tested. |
| 16 | Emergency mode changes get default TTL | THOROUGH | Enters emergency mode, commits, asserts TTL == emergency_window_seconds. Tests real emergency TTL assignment through CommitWindowManager. |
| 17 | TTL below minimum is rejected | THOROUGH | Submits commit with TTL=300 via `JournalCommand::AppendEntry`. Journal returns `ValidationError`. Asserts error message contains "TTL must be >= 900". Tests real journal TTL validation. |
| 18 | TTL at minimum boundary is accepted | THOROUGH | TTL=900, asserts success and correct TTL value. |
| 19 | TTL above maximum is rejected | THOROUGH | TTL=1000000, asserts rejection with correct error message. |
| 20 | TTL at maximum boundary is accepted | THOROUGH | TTL=864000, asserts success and correct TTL value. |

## Given Step Analysis

| Step | Setup Method | Notes |
|------|-------------|-------|
| `default commit window config with base N and sensitivity M` | Constructs real `CommitWindowManager` | Faithful |
| `emergency mode is active with window N seconds` | Calls `commit_mgr.enter_emergency()` | Real emergency mode path |
| `a journal with default state` | `PactWorld::default()` | Faithful |

## Summary

- **THOROUGH**: 12 (window calculation, commit/rollback lifecycle, journal recording, TTL validation, emergency TTL)
- **MODERATE**: 1 (no-TTL persistence — partial, persist assertion is no-op)
- **SHALLOW**: 5 (window expiry, active consumers, rollback entry, TTL expiry/cleanup)
- **STUB**: 0
- **Confidence: MODERATE** (60% THOROUGH, but SHALLOW scenarios cover important behaviors)

## Critical Gaps

1. **Window expiry is not tested via real timers** — the test manually sets `rollback_triggered = true` instead of waiting for or simulating timer expiry (commit_window.rs:152-162). Impact: HIGH (core commit window contract: expired window → automatic rollback).
2. **Active consumer protection is entirely flag-based** — no real process/mount consumer detection. Impact: MEDIUM (safety mechanism for rollback).
3. **Rollback journal entry is self-fulfilling** — `then_rollback_entry` creates the entry it asserts (commit_window.rs:373-391). Same pattern as drift_detection. Impact: MEDIUM.
4. **TTL expiry is untested** — `then_delta_expired` and `then_delta_cleaned_up` are no-ops. The TTL *validation* is thorough but *enforcement* (actual expiry after N seconds) has zero test coverage. Impact: HIGH (ADR-010 TTL bounds).
