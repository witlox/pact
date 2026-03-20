# Mock Fidelity Report

Last scan: 2026-03-20

## Traits and Mock Implementations

### 1. ServiceManager (`pact-agent/src/supervisor/mod.rs:37`)

**Methods**: `start`, `stop`, `restart`, `status`, `health`, `start_all`, `stop_all`

**Real implementations**:
- `PactSupervisor` — direct process management via tokio + cgroup v2
- `SystemdBackend` — delegates to systemd via D-Bus (feature-gated: `systemd`)

**Mock usage in BDD**: Not used directly. BDD tests exercise service ordering through `ServiceDecl` sorting, not through `ServiceManager` trait calls. Boot steps build `ServiceDecl` vectors and sort by `order` field.

**Rating: N/A** — trait not mocked for tests, bypassed entirely. Service lifecycle (start/stop/restart) is never tested through the trait interface.

**Impact: HIGH** — ServiceManager is the core supervisor abstraction. No BDD scenario calls `start()`, `stop()`, or `health()` through the trait. All service ordering tests operate on `ServiceDecl` metadata, not actual process management.

---

### 2. GpuBackend (`pact-agent/src/capability/mod.rs:36`)

**Methods**: `detect`

**Real implementations**: NVIDIA (feature-gated: `nvidia`), AMD (feature-gated: `amd`)

**Mock**: `MockGpuBackend` — returns pre-configured `Vec<GpuCapability>` or empty vec.

**Rating: FAITHFUL** — the mock correctly models the detect → result flow. Returns configurable GPU data or empty (no GPUs). Never returns errors. The real impls can fail (nvidia-smi not found, permission denied).

**Impact: LOW** — capability detection is informational, not security-critical. The mock's inability to return errors is acceptable for BDD tests; unit tests cover error paths.

---

### 3. CpuBackend (`pact-agent/src/capability/cpu.rs:11`)

**Methods**: `detect`

**Real**: `LinuxCpuBackend` — reads `/proc/cpuinfo` + sysfs.
**Mock**: `MockCpuBackend` — returns pre-configured or default `CpuCapability`.

**Rating: FAITHFUL** — same pattern as GpuBackend. Returns configured data. Never errors. Acceptable for BDD.

**Impact: LOW**

---

### 4. MemoryBackend (`pact-agent/src/capability/memory.rs:11`)

**Methods**: `detect`

**Real**: `LinuxMemoryBackend` — reads `/proc/meminfo`, sysfs, dmidecode.
**Mock**: `MockMemoryBackend` — returns pre-configured or default (all zeros) `MemoryCapability`.

**Rating: FAITHFUL**

**Impact: LOW**

---

### 5. NetworkBackend (`pact-agent/src/capability/network.rs:11`)

**Methods**: `detect`

**Real**: `LinuxNetworkBackend` — reads `/sys/class/net/`.
**Mock**: `MockNetworkBackend` — returns pre-configured `Vec<NetworkInterface>`.

**Rating: FAITHFUL**

**Impact: LOW**

---

### 6. StorageBackend (`pact-agent/src/capability/storage.rs:13`)

**Methods**: `detect`

**Real**: `LinuxStorageBackend` — reads `/sys/block/`, `/proc/mounts`, statvfs.
**Mock**: `MockStorageBackend` — returns pre-configured `StorageCapability`.

**Rating: FAITHFUL**

**Impact: LOW**

---

### 7. NetworkManager (`pact-agent/src/network/mod.rs:49`)

**Methods**: `configure`

**Real**: `LinuxNetworkManager` — runs `ip` commands (Linux only).
**Mock/Stub**: `StubNetworkManager` — returns success with echo of input configs.

**Rating: PARTIAL** — the stub always succeeds and echoes back config as if applied. Real impl can fail (bad interface, permission denied, ip command not found). The stub is the production fallback for non-PactSupervisor mode, so its behavior (no-op) is correct for that use case, but it masks errors when used as a test seam.

**Impact: MEDIUM** — network configuration is a boot-critical path. No test verifies that `ip` commands are actually issued.

---

### 8. Observer (`pact-agent/src/observer/mod.rs:34`)

**Methods**: `start`, `stop`

**Real**: `InotifyObserver` (Linux), `NetlinkObserver` (Linux), eBPF observers (feature-gated).
**Mock usage**: Not mocked for BDD. Drift detection tests construct `ObserverEvent` directly and feed to `DriftEvaluator::process_event()`, bypassing the Observer trait entirely.

**Rating: N/A** — trait bypassed. Observer→event→evaluator pipeline not tested end-to-end.

**Impact: MEDIUM** — the Observer trait is the integration point between OS-level monitoring and drift detection. Bypassing it means we test drift evaluation but not event production.

---

### 9. TokenValidator (`pact-policy/src/iam/mod.rs:21`)

**Methods**: `validate`

**Real**: `HmacTokenValidator` — HMAC-based JWT validation with real jsonwebtoken crate.
**Mock usage in BDD**: BDD tests don't go through TokenValidator. Enrollment/auth steps set `world.current_identity` directly.

**Rating: N/A** — trait bypassed in BDD. However, `HmacTokenValidator` has unit tests with real JWT creation/validation. The OIDC token flow is tested at the unit level but not at the BDD level.

**Impact: HIGH** — identity validation is security-critical. BDD scenarios never exercise the token→identity pipeline. All auth assertions are based on pre-set `world.current_identity`.

---

### 10. PolicyEngine (`pact-policy/src/rules/mod.rs:45`)

**Methods**: `evaluate`, `get_effective_policy`

**Real**: `DefaultPolicyEngine` — RBAC + OPA + two-person approval.
**Mock**: `MockPolicyEngine` (in pact-test-harness) — configurable allow/deny/require-approval.

**Rating: PARTIAL** — `MockPolicyEngine` supports per-action overrides (deny specific actions, require approval for others), which mirrors the real engine's routing. However: it never evaluates RBAC rules, never checks scope, never consults OPA. It has no concept of vCluster-scoped roles.

BDD tests use the **real** `DefaultPolicyEngine` with `RbacEngine::evaluate()` for RBAC scenarios. The mock is only used in `pact-test-harness` unit tests, not in BDD acceptance tests. This is good — BDD exercises real RBAC.

**Impact: LOW for BDD** (real engine used), **MEDIUM for pact-test-harness** (mock skips RBAC).

---

### 11. OpaClient (`pact-policy/src/rules/opa.rs:61`)

**Methods**: `evaluate`, `health`

**Real**: `HttpOpaClient` (feature-gated: `opa`) — HTTP calls to OPA REST API.
**Mock**: `MockOpaClient` — always allow, always deny, or unhealthy. Ignores input.

**Rating: PARTIAL** — models healthy/unhealthy and allow/deny, which covers the degraded-mode path (ADR-011). But it ignores the input entirely — never evaluates Rego rules, never checks identity/scope/action. The mock correctly models OPA as a black box for integration testing.

**Impact: MEDIUM** — the mock is appropriate for testing the DefaultPolicyEngine's OPA integration (fallback behavior, degraded mode). Actual Rego rule evaluation requires the `opa` feature integration tests.

---

### 12. FederationSync (`pact-policy/src/federation/mod.rs:17`)

**Methods**: `sync`, `health`

**Real**: No production impl yet (Sovra integration pending).
**Mock**: `MockFederationSync` — healthy (returns templates) or unhealthy (returns error).

**Rating: FAITHFUL** — matches the trait contract. Since no production impl exists, the mock IS the only implementation. Models success (with configurable template list) and failure (with error).

**Impact: LOW** — federation is optional and the mock correctly exercises the FederationState management code.

---

## Mock Fidelity Summary

| Trait | Real Impls | Mock Rating | Impact | Detail |
|-------|------------|-------------|--------|--------|
| ServiceManager | PactSupervisor, SystemdBackend | N/A (bypassed) | **HIGH** | No BDD tests call through trait |
| GpuBackend | nvidia, amd (feature-gated) | FAITHFUL | LOW | |
| CpuBackend | LinuxCpuBackend | FAITHFUL | LOW | |
| MemoryBackend | LinuxMemoryBackend | FAITHFUL | LOW | |
| NetworkBackend | LinuxNetworkBackend | FAITHFUL | LOW | |
| StorageBackend | LinuxStorageBackend | FAITHFUL | LOW | |
| NetworkManager | LinuxNetworkManager | PARTIAL | MEDIUM | Stub never errors |
| Observer | Inotify, Netlink, eBPF | N/A (bypassed) | MEDIUM | Events constructed directly |
| TokenValidator | HmacTokenValidator | N/A (bypassed) | **HIGH** | Identity set directly in BDD |
| PolicyEngine | DefaultPolicyEngine | PARTIAL | LOW (BDD uses real) | Mock only in test-harness |
| OpaClient | HttpOpaClient (feature-gated) | PARTIAL | MEDIUM | Ignores input |
| FederationSync | (none yet) | FAITHFUL | LOW | |

**Key finding**: ServiceManager and TokenValidator are not mocked — they're **bypassed entirely** in BDD tests. These are the two highest-impact traits (process lifecycle and auth), and neither has any BDD coverage through its trait interface.
