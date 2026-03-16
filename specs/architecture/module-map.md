# Module Map

Module boundaries, responsibilities, and ownership. Each module maps to a Rust crate.

---

## pact-common

**Responsibility:** Shared kernel — types, config, errors, protobuf bindings used by all crates.

**Owns:**
- All domain types (ConfigEntry, VClusterPolicy, DriftVector, CapabilityReport, etc.)
- Configuration structs (PactConfig, AgentConfig, JournalConfig, PolicyConfig)
- Error taxonomy (PactError enum)
- Protobuf-generated types (proto modules for all 6 .proto files)

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
- Overlay pre-computation and caching
- Telemetry: Prometheus metrics + health endpoint (axum on port 9091)
- Loki event forwarding

**Submodules:**
- `raft/` — state machine, types, Raft type config
- `service/` — ConfigService, PolicyService, BootConfigService handlers
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

**Responsibility:** Per-node daemon. Init system on diskless nodes. Process supervision, state observation, drift detection, commit window management, capability reporting, shell/exec server, emergency mode.

**Owns:**
- Process supervisor (PactSupervisor + SystemdBackend)
- State observer (eBPF, inotify, netlink)
- Drift evaluator (7-dimension comparison)
- Commit window manager (formula-based timing, auto-rollback)
- Capability reporter (GPU, memory, network, storage, software)
- Shell server (exec endpoint + interactive shell)
- Emergency mode manager
- Boot sequence orchestration
- Config subscription (live updates from journal)
- Local config/policy cache for partition resilience

**Submodules:**
- `supervisor/` — ServiceManager trait, PactSupervisor, SystemdBackend
- `observer/` — eBPF, inotify, netlink observers
- `drift/` — DriftEvaluator, blacklist filtering
- `commit/` — CommitWindow runtime, auto-rollback, active consumer check
- `capability/` — GpuBackend trait, CapabilityReport builder
- `shell/` — ShellService gRPC, exec handler, interactive shell, whitelist
- `emergency/` — EmergencySession lifecycle
- `subscription/` — config update stream consumer
- `cache/` — local config + policy cache

**Justification:** Agent is the Node Management bounded context (domain-model.md). ADR-006 defines pact-agent as init. ADR-007 defines shell as SSH replacement.

---

## pact-cli

**Responsibility:** Admin command-line tool. Stateless. Connects to journal and agent via gRPC.

**Owns:**
- Command parsing (clap derive)
- gRPC client connections (journal ConfigService/PolicyService/BootConfigService, agent ShellService)
- OIDC token acquisition
- Output formatting (table, JSON)
- Exit code semantics
- Delegation stubs (lattice drain/cordon, OpenCHAMI reboot/reimage)

**Submodules:**
- `commands/` — one module per command group (status, diff, commit, exec, shell, etc.)
- `client/` — gRPC client wrappers
- `auth/` — OIDC token flow
- `output/` — formatting

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
- 18 feature files (258 scenarios) — the behavioral specification
- PactWorld step implementations
- Custom harness (`harness = false`)

**Justification:** testing-strategy.md Level 3. Feature files are the specification — steps must call real pact code when features are implemented.

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
