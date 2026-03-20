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

### Level 3: BDD Acceptance Tests (`pact-acceptance`)

555 scenarios across 31 feature files using the `cucumber` crate. Covers all
bounded contexts: boot config streaming, drift detection → commit/rollback,
shell session lifecycle, emergency mode, enrollment, RBAC, policy evaluation,
overlay management, partition resilience, identity mapping, workload integration,
and 24 cross-context integration scenarios.

**Runs on all platforms** — uses real domain logic (JournalState, DriftEvaluator,
CommitWindowManager, PactSupervisor) but stubs OS-level interactions.

### Level 4: E2E Container Tests (`pact-e2e`)

Integration tests using `testcontainers` with real services:
- **Raft cluster**: 3-node in-process cluster (consensus, failover, replication)
- **OPA**: Real Rego policy evaluation via OPA container
- **Keycloak**: Real OAuth2/OIDC flows (discovery, credentials, password, refresh, JWKS)
- **Loki**: Structured event forwarding
- **Prometheus**: Metrics scraping
- **SPIRE**: Identity cascade and SVID acquisition
- **Linux privileged**: Real cgroup/namespace tests (requires root)
- **Full CLI E2E**: All CLI commands against real journal + agent gRPC

### Level 5: Fidelity & Adversary Sweeps

Automated quality assessment (not runtime tests):
- **Fidelity sweep**: measures assertion depth per scenario (THOROUGH/MODERATE/SHALLOW/STUB)
- **Adversary sweep**: systematic security review across attack surfaces
- Results in `specs/fidelity/` and `specs/findings/`

## Cross-Platform Testing Strategy

Three tiers matching the development model:

1. **macOS (local dev)**: Unit tests + BDD acceptance + e2e containers (Docker).
   Feature-gated Linux-only code compiles as stubs. Mocks (`MockGpuBackend`,
   `MockCpuBackend`, etc.) simulate hardware detection for testing.

2. **CI (GitHub Actions)**: 4-stage pipeline: fmt/clippy/deny → feature checks →
   test/BDD/e2e/linux-privileged → coverage. Runs on Linux runners.

3. **Devcontainer (Linux)**: Full integration + acceptance + chaos tests.
   Real PactSupervisor with cgroup v2, real eBPF probes, real PTY allocation.
   BDD/cucumber scenarios run here. CI uses this for release gates.

## Test Infrastructure

### pact-test-harness

Shared crate (`publish = false`) providing:
- **Fixtures**: `ConfigEntryBuilder`, `ServiceDeclBuilder` — fluent builders
- **Mocks**: `MockJournalClient`, `MockPolicyEngine`, `MockSupervisor`,
  `MockObserver`, `MockGpuBackend` — `Arc<Mutex<Vec<MockCall>>>` for call recording

### Conventions

- Unit tests: `#[test]` or `#[tokio::test]`
- Slow tests: marked `#[ignore]`, run with `just test-all`
- Feature-gated tests: `#[cfg(feature = "ebpf")]` etc.
- Linux-only tests: `#[cfg(target_os = "linux")]`
- All mocks use `async_trait` and record calls for assertions
- Property tests with `proptest` for type invariants
- Coverage target: >= 80% per crate

## CI Pipeline

```
On every commit:
  cargo fmt --check
  cargo clippy --all-targets
  cargo nextest run (Level 1 + Level 2)
  cargo deny check

On release:
  All levels must pass (including devcontainer Level 3 + Level 4)
  Coverage report → codecov
```

## Running Tests

```bash
just test          # Fast: unit + integration (skips #[ignore])
just test-all      # Full: includes slow tests
just test-slow     # Only slow tests
just test-linux    # Linux-only tests (in devcontainer)
just test-accept   # BDD acceptance tests (in devcontainer)
```
