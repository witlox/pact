# ADR-015: hpc-core Shared Contracts (hpc-node, hpc-audit, hpc-identity)

## Status: Accepted

## Context

pact and lattice are independent systems that benefit from co-deployment. When pact is
the init system and lattice manages workloads, lattice gains capabilities ("steroids"):
cgroup pre-creation, namespace handoff, mount refcounting, unified audit, shared mTLS.
But lattice must also work standalone (on systemd-managed nodes without pact).

Both systems need to agree on:
1. cgroup slice layout and ownership boundaries
2. Namespace FD passing protocol
3. Mount point conventions
4. Audit event format for SIEM integration
5. Workload identity (mTLS certificate) management

The existing hpc-core workspace (`../hpc-core`) contains three trait-based crates:
`raft-hpc-core`, `hpc-scheduler-core`, `hpc-auth`. These define shared contracts that
pact and lattice implement independently. The pattern works: traits + types, no
implementations, no runtime coupling.

## Decision

**Add three new crates to hpc-core, following the same trait-based pattern.**

### hpc-node

Shared contracts for node-level resource management.

| Contract | Purpose | Implements |
|----------|---------|-----------|
| `CgroupManager` trait | Hierarchy creation, scope management, metrics | pact-agent (direct cgroup v2), lattice-node-agent (standalone) |
| `NamespaceProvider` trait | Create namespaces for allocations | pact-agent |
| `NamespaceConsumer` trait | Request namespaces (with self-service fallback) | lattice-node-agent |
| `MountManager` trait | Refcounted mounts, lazy unmount, reconstruction | pact-agent, lattice-node-agent (standalone) |
| `ReadinessGate` trait | Boot readiness signaling | pact-agent (provider), lattice-node-agent (consumer) |
| `SliceOwner` enum | Pact / Workload ownership | Compile-time contract |
| `slices` constants | Well-known cgroup paths | Compile-time contract |
| Well-known paths | Socket paths, mount bases | Compile-time contract |

Key design:
- `CgroupManager` does NOT enforce ownership — that's the implementer's responsibility.
  The trait provides `slice_owner()` as a query, not a guard.
- Namespace handoff uses unix socket at `HANDOFF_SOCKET_PATH` with SCM_RIGHTS.
- Mount conventions define base paths but not mount implementation.
- ReadinessGate has both sync (`is_ready()`) and async (`wait_ready()`) methods.

### hpc-audit

Shared audit event types and sink trait. Loose coupling, high coherence.

| Contract | Purpose | Implements |
|----------|---------|-----------|
| `AuditEvent` type | Universal event format (who, what, when, where, outcome) | All components emit |
| `AuditSink` trait | Destination interface (`emit()` + `flush()`) | pact-journal (append), pact-agent (buffer+forward), lattice-quorum, file writer, SIEM forwarder |
| `CompliancePolicy` type | Retention rules, required audit points | pact-policy, lattice-policy |
| Action constants | Well-known action strings | Compile-time contract |

Key design:
- `AuditSink::emit()` must not block. Buffer internally.
- Each system owns its audit log (pact → journal, lattice → quorum).
- Shared format enables a single `AuditForwarder` for SIEM integration.
- `AuditSource` enum distinguishes which system emitted an event.

### hpc-identity

Workload identity abstraction. SPIRE/self-signed/bootstrap behind a trait.

| Contract | Purpose | Implements |
|----------|---------|-----------|
| `IdentityProvider` trait | Obtain workload identity from any source | SpireProvider, SelfSignedProvider, StaticProvider |
| `CertRotator` trait | Certificate rotation (dual-channel swap) | pact-agent, lattice-node-agent |
| `IdentityCascade` | Try providers in order (SPIRE → self-signed → bootstrap) | pact-agent, lattice-node-agent |
| `WorkloadIdentity` type | Source-agnostic cert + key + trust bundle | Used by all mTLS consumers |
| `IdentitySource` enum | Spire / SelfSigned / Bootstrap | Audit provenance |
| Provider configs | SpireConfig, SelfSignedConfig, BootstrapConfig | Configurable per deployment |

Key design:
- `IdentityCascade` is a struct, not a trait — it composes `IdentityProvider` impls.
- Provider implementations live in the consuming crates (pact-agent, lattice-node-agent),
  not in hpc-identity. The crate only defines the contract.
- `WorkloadIdentity` contains PEM data (not parsed certs) for maximum interoperability.
- `CertRotator::rotate()` contract: must not interrupt in-flight operations.
- Partially supersedes ADR-008 cert management (see ADR-008 amendment).

## Rationale

**Why hpc-core, not pact-specific crates?**
- Lattice must work independently of pact (A-Int6). If contracts were pact-specific,
  lattice would depend on a pact crate — creating coupling.
- hpc-core is the established pattern: trait-based, no implementation, no runtime coupling.
- Both systems implement the same traits, ensuring convention agreement without coordination.

**Why three crates, not one?**
- Different change reasons: cgroup layout (hpc-node) changes rarely, audit format
  (hpc-audit) changes with compliance requirements, identity providers (hpc-identity)
  change with infrastructure evolution (SPIRE adoption).
- Minimal dependencies: hpc-audit needs only serde + chrono. hpc-node needs only serde.
  hpc-identity needs async-trait + chrono + thiserror. No reason to force all consumers
  to take all dependencies.

**Why not extend hpc-auth?**
- hpc-auth is about OAuth2/OIDC token management (user authentication at the CLI level).
- hpc-identity is about workload mTLS identity (machine authentication between services).
- Different domains despite both being "identity." Different consumers (CLI vs agent).

## Trade-offs

- (+) Clear trait-based contracts — both systems implement independently
- (+) Lattice gains capabilities when pact is present, works alone when not
- (+) Shared audit format for unified SIEM
- (+) SPIRE integration shared between pact and lattice
- (+) Well-known paths and conventions prevent configuration drift
- (-) Three more crates to maintain in hpc-core
- (-) Trait design must be stable — breaking changes affect both pact and lattice
- (-) Contract validation is only at integration test level (no compile-time guarantee
  that both sides interpret traits the same way)

## Consequences

- hpc-core workspace gains three new members: `crates/node/`, `crates/audit/`, `crates/identity/`
- pact-agent depends on hpc-node, hpc-audit, hpc-identity (compile-time)
- lattice-node-agent depends on hpc-node, hpc-audit, hpc-identity (compile-time)
- No runtime dependency between pact and lattice (only shared contracts)
- CI for hpc-core gains three new pipelines (ci-node.yml, ci-audit.yml, ci-identity.yml)
- Version scheme follows existing hpc-core pattern (year.major.commitcount)

## References

- specs/domain-model.md §2b, §2f, Cross-cutting: Audit, hpc-identity
- specs/architecture/interfaces/hpc-node.md, hpc-audit.md, hpc-identity.md
- specs/invariants.md RI1, WI1-WI6, O3, PB4-PB5
- ADR-008 (amended: SPIRE primary, self-signed fallback)
