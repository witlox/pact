# Testing Strategy

## Four Levels

### Level 1: Unit Tests (in-module)

Located in `#[cfg(test)]` modules within source files. Test critical paths:
config deserialization, state machines, drift computation, serialization roundtrips.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_magnitude_zero_when_no_drift() { ... }
}
```

### Level 2: Integration Tests (crate-level)

Located in `crates/*/tests/`. Use builders from `pact-test-harness` for setup.

```rust
use pact_test_harness::fixtures::ConfigEntryBuilder;

#[tokio::test]
async fn config_entry_roundtrip() {
    let entry = ConfigEntryBuilder::new()
        .vcluster("ml-training")
        .author("admin@example.org")
        .build();
    // ...
}
```

### Level 3: BDD Acceptance Tests

End-to-end scenarios using `cucumber` crate. Located in a dedicated
acceptance crate (when added). Scenarios cover: boot config streaming,
drift detection → commit/rollback, shell session lifecycle, emergency mode.

### Level 4: Chaos Tests

Adversarial inputs and concurrent operations. Verify invariants:
- No duplicate config entries after concurrent commits
- Journal consistency after Raft leader failover
- Drift detection accuracy under filesystem churn

## Test Infrastructure

### pact-test-harness

Shared crate (`publish = false`) providing:
- **Fixtures**: `ConfigEntryBuilder`, `ServiceDeclBuilder` — fluent builders
- **Mocks**: `MockJournalClient`, `MockPolicyEngine` — `Arc<Mutex<Vec<MockCall>>>` for call recording

### Conventions

- Unit tests: `#[test]` or `#[tokio::test]`
- Slow tests: marked `#[ignore]`, run with `just test-all`
- Feature-gated tests: `#[cfg(feature = "ebpf")]` etc.
- All mocks use `async_trait` and record calls for assertions
- Property tests with `proptest` for type invariants

## CI Pipeline

```
On every commit:
  cargo fmt --check
  cargo clippy --all-targets
  cargo nextest run (Level 1 + Level 2)
  cargo deny check

On release:
  All levels must pass
  Coverage report → codecov
```

## Running Tests

```bash
just test          # Fast: unit + integration (skips #[ignore])
just test-all      # Full: includes slow tests
just test-slow     # Only slow tests
```
