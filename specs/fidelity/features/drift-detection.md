# Fidelity Report: drift_detection.feature

Last scan: 2026-03-20
Feature file: `crates/pact-acceptance/features/drift_detection.feature`
Step definitions: `crates/pact-acceptance/tests/steps/drift.rs`

## Scenarios: 21

| # | Scenario | Depth | Notes |
|---|----------|-------|-------|
| 1 | Changes in blacklisted paths are ignored | THOROUGH | Feeds `/tmp/scratch/data.bin` through real `DriftEvaluator::process_event()`. Compares files dimension before/after. Real blacklist filtering via `BlacklistConfig`. |
| 2 | Changes in /var/log are ignored | THOROUGH | Same real evaluator path. |
| 3 | Changes in /proc are ignored | THOROUGH | Same real evaluator path. |
| 4 | Changes outside blacklist trigger drift | THOROUGH | Processes event for `/etc/pact/agent.toml`, asserts `magnitude() > 0.0` and `files` dimension > 0.0. Real evaluator, real dimension accumulation. |
| 5 | Custom blacklist patterns respected | THOROUGH | Rebuilds `DriftEvaluator` with appended custom pattern. Feeds matching path, asserts filtered. Tests real pattern matching. |
| 6 | Mount change → mounts dimension | THOROUGH | Processes mount event, asserts `mounts` > 0.0 and other dims zero. Real evaluator. |
| 7 | Kernel parameter change → kernel dimension | THOROUGH | Real evaluator, asserts `kernel` > 0.0. |
| 8 | Service state change → services dimension | THOROUGH | Real evaluator, asserts `services` > 0.0. |
| 9 | Network change → network dimension | THOROUGH | Real evaluator, asserts `network` > 0.0. |
| 10 | GPU state change → gpu dimension | THOROUGH | Real evaluator, asserts `gpu` > 0.0. |
| 11 | Kernel drift weighted higher than file drift | THOROUGH | Constructs two `DriftVector`s with single dimensions at 1.0, calls `magnitude(weights)`, asserts kernel > files. Tests real weight application. |
| 12 | GPU drift weighted higher than mount drift | THOROUGH | Same pattern, tests gpu > mounts. |
| 13 | Zero drift produces zero magnitude | THOROUGH | Default vector + real weights → asserts magnitude == 0.0. |
| 14 | Multi-dimension drift compounds magnitude | THOROUGH | Sets kernel=0.5 + gpu=0.5, asserts compound > single 0.5. Tests real magnitude calculation with real weights. |
| 15 | Observe-only mode logs drift without enforcement | MODERATE | Sets `enforcement_mode = "observe"`. After drift: asserts drift logged (magnitude > 0 or journal entry exists). Asserts `rollback_triggered == false`. Mode check is via flag, not real enforcement code path. |
| 16 | Enforce mode triggers commit window on drift | MODERATE | Sets `enforcement_mode = "enforce"`. WHEN step opens commit window via `commit_mgr.open()`. Then asserts commit window opened + drift logged. Tests real `CommitWindowManager::open()` but mode-to-action routing is manual in step def. |
| 17 | eBPF-detected syscall → kernel drift | THOROUGH | Processes event with category "kernel", path "sethostname". Asserts kernel dimension > 0.0. Same real evaluator path. (Note: doesn't test real eBPF probe, just the event processing.) |
| 18 | inotify file watch → files drift | THOROUGH | Same pattern for file event. |
| 19 | netlink interface change → network drift | THOROUGH | Same pattern for network event. |
| 20 | DriftDetected entry recorded in journal | SHALLOW | `then_drift_entry` (line 254) CREATES the entry if not present, then asserts it exists. Self-fulfilling — doesn't test that drift detection produces a journal write. |
| 21 | Commit window should be opened (from enforce mode) | MODERATE | See #16. |

## Given Step Analysis

| Step | Setup Method | Notes |
|------|-------------|-------|
| `default drift weights` | No-op — evaluator default | Faithful |
| `a custom blacklist pattern X` | Rebuilds evaluator with appended pattern | Faithful — goes through real DriftEvaluator constructor |
| `enforcement mode is "observe/enforce"` | Sets `world.enforcement_mode` string | Flag-based, not wired to real enforcement dispatcher |
| `a drift vector with X magnitude Y` | Writes to `drift_vector_override` | Bypasses DriftEvaluator — needed because evaluator only supports 1.0 increments. Documented in header comment. |

## Summary

- **THOROUGH**: 16 (blacklist filtering, dimension routing, weighted magnitude, observer sources)
- **MODERATE**: 3 (observe/enforce mode routing, commit window integration)
- **SHALLOW**: 1 (DriftDetected journal entry — self-fulfilling)
- **STUB**: 0
- **None**: 1 (the DriftDetected entry test doesn't verify the claim)
- **Confidence: HIGH** (76% THOROUGH — borderline, but the MODERATE scenarios are real code)

**Revised Confidence: HIGH** (THOROUGH + MODERATE = 90%+)

## Critical Gaps

1. **DriftDetected journal write is self-fulfilling** — `then_drift_entry` creates the entry it then asserts exists (drift.rs:256-276). This means the test doesn't verify that drift detection actually writes to the journal. Impact: HIGH (audit trail for drift is a core invariant).
2. **Enforcement mode routing is flag-based** — the WHEN step manually checks the string and calls `commit_mgr.open()`. This doesn't test the real enforcement dispatcher that should map drift → mode check → action. Impact: MEDIUM (the unit logic is tested, but the routing is untested).
3. **Observer sources don't test real eBPF/inotify/netlink** — events are manually constructed with `make_event()`. Impact: LOW for unit tests (integration concern). Feature-gated eBPF tests would be INTEGRATION depth.
