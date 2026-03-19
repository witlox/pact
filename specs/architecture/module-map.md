# Module Map

Module boundaries, responsibilities, and ownership. Each module maps to a Rust crate.

---

## pact-common

**Responsibility:** Shared kernel — types, config, errors, protobuf bindings used by all crates.

**Owns:**
- All domain types (ConfigEntry, VClusterPolicy, DriftVector, CapabilityReport, etc.)
- Configuration structs (PactConfig, AgentConfig, JournalConfig, PolicyConfig)
- Error taxonomy (PactError enum)
- Protobuf-generated types (proto modules for all 7 .proto files)

**Does NOT own:** Business logic, I/O, network, state management.

**Justification:** Shared kernel pattern — common types prevent duplication and ensure wire compatibility. Referenced by every invariant that mentions a domain type.

---

## pact-journal

**Responsibility:** Distributed immutable log. Raft consensus. Hosts ConfigService and PolicyService gRPC endpoints. Boot config streaming. Telemetry.

**Owns:**
- Raft state machine (JournalState) — the single source of truth
- ConfigService gRPC handlers (journal.proto)
- PolicyService gRPC handlers (policy.proto) — delegates to pact-policy library
- BootConfigService gRPC handlers (stream.proto)
- EnrollmentService gRPC handlers (enrollment.proto) — node enrollment, CSR signing (ADR-008)
- Enrollment registry (NodeEnrollment records in Raft state)
- CaKeyManager — intermediate CA key, local CSR signing (ADR-008)
- Overlay pre-computation and caching
- Telemetry: Prometheus metrics + health endpoint (axum on port 9091)
- Loki event forwarding

**Submodules:**
- `raft/` — state machine, types, Raft type config
- `service/` — ConfigService, PolicyService, BootConfigService handlers
- `enrollment/` — EnrollmentService handlers, CaKeyManager (ADR-008)
- `overlay/` — overlay builder, cache, staleness detection
- `telemetry/` — metrics, health, Loki forwarding

**Justification:** Journal is the control plane core (domain-model.md: Configuration Management context). All writes go through Raft (invariant J7). PolicyService hosted here per ADR-003.

---

## pact-policy

**Responsibility:** Library crate providing IAM, RBAC, and OPA policy evaluation logic. Linked into pact-journal binary — NOT a standalone service.

**Owns:**
- OIDC token validation (JWT verification, JWKS caching)
- RBAC engine (role → permission evaluation)
- OPA integration (REST client to localhost sidecar, feature-gated `opa`)
- Sovra federation sync (Rego template pull, feature-gated `federation`)
- Policy caching for agent degraded mode

**Submodules:**
- `iam/` — OIDC/JWT verification
- `rbac/` — role definitions, permission evaluation
- `rules/` — OPA delegation, policy evaluation orchestration
- `federation/` — Sovra sync (feature-gated)
- `cache/` — policy cache for degraded mode

**Justification:** ADR-003 decided OPA/Rego as sidecar. pact-policy is library, not service (interactions.md: I7). Separate crate for testability and feature-gating.

---

## pact-agent

**Responsibility:** Per-node daemon. Init system on diskless nodes. Decomposed into 6 sub-contexts (domain-model.md §2a-2f), each with PactSupervisor/SystemdBackend strategy.

**Owns:**
- Process supervision (PactSupervisor + SystemdBackend) — supervision loop, health checks, restarts
- Resource isolation — cgroup v2 hierarchy, scopes, namespace creation
- Identity mapping — UidMap management, NSS .db file writer (PactSupervisor mode only)
- Network management — netlink interface config (PactSupervisor mode only)
- Platform bootstrap — boot phases, watchdog, SPIRE integration, coldplug
- Workload integration — namespace handoff server, mount refcounting
- State observer (eBPF, inotify, netlink)
- Drift evaluator (7-dimension comparison)
- Commit window manager (formula-based timing, auto-rollback)
- Capability reporter (GPU, memory, network, storage, software)
- Shell server (exec endpoint + interactive shell)
- Emergency mode manager
- Enrollment: hardware identity detection, identity cascade (SPIRE/self-signed/bootstrap)
- Config subscription (live updates from journal)
- Local config/policy cache for partition resilience

**Submodules:**
- `supervisor/` — ServiceManager trait, PactSupervisor (with supervision loop), SystemdBackend
- `isolation/` — CgroupManager impl, namespace creation, mount refcounting (implements hpc-node traits)
- `identity/` — UidMap cache, NSS .db writer, identity cascade (implements hpc-identity traits)
- `network/` — netlink interface configuration
- `boot/` — BootSequence phases, WatchdogHandle, coldplug, readiness signal
- `handoff/` — namespace handoff unix socket server (implements hpc-node NamespaceProvider)
- `observer/` — eBPF, inotify, netlink observers
- `drift/` — DriftEvaluator, blacklist filtering
- `commit/` — CommitWindow runtime, auto-rollback, active consumer check
- `capability/` — GpuBackend trait, CapabilityReport builder
- `shell/` — ShellService gRPC, exec handler, interactive shell, whitelist
- `emergency/` — EmergencySession lifecycle
- `enrollment/` — HardwareIdentity detection, EnrollmentClient
- `subscription/` — config update stream consumer
- `cache/` — local config + policy cache
- `audit/` — AuditSink impl (journal append + local buffer), AuditEvent emitter

**Justification:** Agent is the Node Management bounded context (domain-model.md §2). ADR-006 defines pact-agent as init. ADR-007 defines shell as SSH replacement. Sub-context decomposition per analyst Layer 1.

---

## pact-cli

**Responsibility:** Admin command-line tool. Stateless. Connects to journal and agent via gRPC.

**Owns:**
- Command parsing (clap derive)
- gRPC client connections (journal ConfigService/PolicyService/BootConfigService/EnrollmentService, agent ShellService)
- Lattice client connection (for supercharged commands: jobs, queue, cluster, audit, accounting, health)
- OIDC token acquisition
- Output formatting (table, JSON)
- Exit code semantics
- Delegation stubs (lattice drain/cordon, OpenCHAMI reboot/reimage)

**Submodules:**
- `commands/` — one module per command group (status, diff, commit, exec, shell, jobs, queue, cluster, audit, accounting, health, etc.)
- `client/` — gRPC client wrappers (journal + lattice)
- `auth/` — OIDC token flow
- `output/` — formatting

**Lattice integration:** The supercharged commands (`jobs list/cancel/inspect`, `queue`, `cluster`, `audit`, `accounting`, `health`) require a lattice-client dependency to query the lattice scheduler and audit APIs. Configured via `PACT_LATTICE_ENDPOINT`.

**Justification:** CLI is the Admin Operations bounded context entry point (domain-model.md). cli-design.md defines the full command set.

---

## pact-test-harness

**Responsibility:** Shared test infrastructure. Builders, mocks, fixtures for cross-crate testing.

**Owns:**
- ConfigEntryBuilder, VClusterPolicyBuilder, ServiceDeclBuilder
- MockJournalClient, MockPolicyEngine
- Future: MockSupervisor, MockObserver, MockGpuBackend, CapabilityReportBuilder

**Justification:** testing-strategy.md mandates shared test infrastructure. `publish = false`.

---

## pact-acceptance

**Responsibility:** BDD acceptance tests. Gherkin feature files + cucumber step implementations.

**Owns:**
- 28 feature files (463 scenarios) — the behavioral specification
- PactWorld step implementations
- Custom harness (`harness = false`)

**Justification:** testing-strategy.md Level 3. Feature files are the specification — steps must call real pact code when features are implemented.

---

## pact-nss (companion crate — outside workspace)

**Responsibility:** glibc NSS module mapping OIDC identities to POSIX UID/GID for NFS compatibility. Built separately as a shared library (`libnss_pact.so.2`).

**Owns:**
- NSS `passwd` and `group` hooks (via `libnss` crate)
- Reads `/run/pact/passwd.db` and `/run/pact/group.db` (JSON, written by pact-agent identity module)
- In-memory caching with mtime-based invalidation

**Does NOT own:** Identity mapping logic (pact-agent writes the .db files). No network calls. Read-only.

**License:** LGPL-3.0-only (required by `libnss` dependency). Dynamic linking (`cdylib`) satisfies LGPL. Not part of workspace to avoid license contamination.

**Platform:** Linux only (NSS is glibc-specific).

**Justification:** ADR-016 — OIDC-to-POSIX identity mapping for NFS. Only active in PactSupervisor mode. Separate crate because NSS modules must be shared libraries loaded by glibc at runtime.

---

## hpc-auth (external shared crate)

**Responsibility:** OAuth2/OIDC token acquisition, caching, and refresh. Shared between pact-cli and lattice-cli.

**Owns:**
- OAuth2 flow execution (Auth Code+PKCE, Device Code, Client Credentials, Manual Paste)
- Token cache (file-based, per-server keyed, permission validation)
- OIDC discovery document fetching and caching
- Silent token refresh
- Cascading flow fallback logic (Auth8)

**Does NOT own:**
- gRPC metadata injection (consumer responsibility)
- RBAC/policy evaluation (server-side, pact-policy)
- CLI subcommand definitions (consumer defines `login`/`logout`)
- Server-side token validation (pact-agent shell/auth.rs)

**Consumed by:** pact-cli, lattice-cli (as a library dependency)

**Invariants enforced:** Auth1-Auth8, with PAuth1-PAuth5 enforced by consumer (pact-cli)

**Justification:** specs/invariants.md Auth1-Auth8 require a shared auth library. Both pact and lattice CLIs need identical OAuth2 flows, differing only in permission mode (PAuth1 vs lenient). Extracting to a shared crate prevents duplication and ensures consistent behavior.

---

## hpc-node (external shared crate — NEW)

**Responsibility:** Shared contracts for node-level resource management between pact and lattice. Traits and types only — no implementation.

**Owns:**
- Cgroup slice naming conventions (constants for pact.slice/, workload.slice/)
- `CgroupManager` trait — hierarchy creation, scope management, metrics reading
- `ResourceLimits`, `CgroupHandle`, `CgroupMetrics` types
- `NamespaceProvider` / `NamespaceConsumer` traits — namespace handoff protocol
- `NamespaceRequest`, `NamespaceResponse` types
- `MountManager` trait — mount refcounting, lazy unmount, reconstruction
- `MountHandle` types
- `ReadinessGate` trait — boot readiness signaling
- Well-known paths (socket paths, mount base paths)
- Error types (CgroupError, NamespaceError, MountError, ReadinessError)

**Does NOT own:** Implementations. No Linux-specific code. No async runtime dependency.

**Consumed by:** pact-agent (implements CgroupManager, NamespaceProvider, MountManager), lattice-node-agent (implements CgroupManager, NamespaceConsumer, MountManager in standalone mode)

**Invariants enforced:** RI1 (slice ownership via SliceOwner enum), WI1 (handoff socket path), WI4 (shared conventions)

**Justification:** Both pact and lattice need to agree on cgroup layout, namespace handoff, and mount conventions. hpc-core shared kernel pattern (domain-model.md §2f). Lattice must work independently of pact (A-Int6).

---

## hpc-audit (external shared crate — NEW)

**Responsibility:** Shared audit event types and sink trait. Loose coupling, high coherence.

**Owns:**
- `AuditEvent` type — universal event format
- `AuditPrincipal`, `AuditScope`, `AuditOutcome`, `AuditSource` types
- `AuditSink` trait — destination interface
- `CompliancePolicy` type — retention rules, required audit points
- Well-known action string constants
- Error types (AuditError)

**Does NOT own:** Sink implementations (each system implements its own). No I/O.

**Consumed by:** pact-agent, pact-journal, pact-cli, lattice-node-agent, lattice-quorum

**Invariants enforced:** O3 (audit trail continuity — via AuditSink contract)

**Justification:** Both pact and lattice need audit. Shared format enables unified SIEM forwarding. Cross-cutting concern (domain-model.md).

---

## hpc-identity (external shared crate — NEW)

**Responsibility:** Workload identity abstraction. SPIRE/self-signed/bootstrap cert sources behind a trait. Certificate rotation.

**Owns:**
- `WorkloadIdentity` type — source-agnostic cert + key + trust bundle
- `IdentitySource` enum (Spire, SelfSigned, Bootstrap)
- `IdentityProvider` trait — obtain identity from any source
- `CertRotator` trait — dual-channel rotation pattern
- `IdentityCascade` — try providers in order
- Provider configs (SpireConfig, SelfSignedConfig, BootstrapConfig)
- Error types (IdentityError)

**Does NOT own:** Provider implementations. No SPIRE client code. No CA management code.

**Consumed by:** pact-agent (SpireProvider + SelfSignedProvider + StaticProvider), lattice-node-agent (same providers)

**Invariants enforced:** PB4 (bootstrap temporary), PB5 (no hard SPIRE dependency), E6 (dual-channel rotation pattern)

**Justification:** Both pact and lattice need mTLS. SPIRE is pre-existing (A-I7). ADR-008 self-signed model is fallback. Shared trait prevents each system from reinventing identity management. See A-mTLS1.
