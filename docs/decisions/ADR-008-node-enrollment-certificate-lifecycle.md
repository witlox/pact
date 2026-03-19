# ADR-008: Node Enrollment, Domain Membership, and Certificate Lifecycle

## Status: Accepted (amended 2026-03-17 — SPIRE as primary mTLS provider)

## Context

pact-agent authenticates to pact-journal via mTLS. The current design (A-I2) assumes
OpenCHAMI provisions per-node certificates into the SquashFS base image. This assumption
breaks at scale and in multi-domain deployments:

1. **Shared image problem.** Diskless nodes boot from a single SquashFS image. You cannot
   bake 1,000 unique certificates into one read-only image.

2. **Certificate rotation.** Static certificates expire. There is no mechanism to renew
   them without re-imaging every node.

3. **Multi-domain assignment.** A physical machine may be partitioned across multiple pact
   domains (each with its own journal quorum). A node must be enrollable in
   multiple domains, but active in only one at a time.

4. **Unauthorized enrollment.** No mechanism exists to prevent a node from connecting to
   any journal it can reach. There is no enrollment registry or hardware identity
   verification.

5. **Boot storm.** 10,000+ nodes booting simultaneously must not overwhelm the certificate
   authority.

## Decision

### Two-level membership model

Node lifecycle in pact has two independent axes:

**Domain membership** (enrollment): "this node is allowed to exist in this pact instance."
Controls certificate issuance, mTLS trust, and physical/security boundary.

**vCluster assignment**: "this node is currently part of this logical group." Controls
configuration overlay, policy, drift detection, and scheduling. vCluster assignment is
optional — an enrolled node with no vCluster assignment is in maintenance mode.

These compose independently. A node can be:
- Enrolled but unassigned (maintenance pool, spare, staging)
- Enrolled and assigned to a vCluster (normal operation)
- Moved between vClusters without re-enrollment
- Enrolled in multiple domains, active in only one (shared hardware)

### Certificate authority: self-generated ephemeral CA on journal nodes

Each pact-journal node generates an ephemeral intermediate CA key at startup (in memory
only, never persisted to disk). This eliminates external CA dependencies from the boot
and renewal paths entirely.

Rationale:
- No external dependency for certificate operations — the journal is fully self-contained.
- Ephemeral keys reduce exposure risk: key compromise requires runtime access to a journal
  node's memory, and the key is rotated on every journal restart.
- Certificate revocation is handled via a Raft revocation registry (replicated to all
  journal nodes), not an external CRL.
- Journal nodes sign agent CSRs locally using the ephemeral CA key — a CPU-only operation.

Per-domain topology:
```
┌─ pact domain ─────────────────────────────────────────────┐
│                                                           │
│  pact-journal quorum (3-5 nodes)                          │
│    ├── each generates ephemeral CA key at startup         │
│    ├── CA cert distributed via enrollment responses       │
│    ├── revocation registry replicated via Raft            │
│    └── signs agent CSRs locally (CPU-only, no network)    │
│                                                           │
│  pact-agents (1000s)                                      │
│    ├── generate own keypair at boot (in RAM)              │
│    └── submit CSR to journal, receive signed cert         │
│                                                           │
│  OpenCHAMI/Manta (boot infra)                             │
│    └── boots nodes, no cert responsibility                │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

### CSR model: agent generates keypair, journal signs

Private keys never leave the agent. The agent generates its own keypair at boot, submits
a Certificate Signing Request (CSR) to the journal, and receives a signed certificate.
The journal signs using its intermediate CA key — a local CPU operation with no network
dependency.

This design ensures:
- **No private keys in Raft state.** Journal stores only enrollment records and signed
  certs (public data). Compromise of a journal node does not expose agent private keys.
- **No private keys on the wire.** The enrollment endpoint serves signed certs, not key
  material. Even if the endpoint is spoofed, the attacker gets a cert for their own key
  — they cannot impersonate the real agent.
- **Boot storm safe.** CSR signing is ~1ms CPU per cert. 10,000 concurrent CSRs are
  signed in ~10 seconds on a single core. No external traffic.

### Enrollment registry

pact-journal maintains a node enrollment registry in Raft state. Each enrollment record
contains:
- Node identity (node_id)
- Hardware identity (MAC addresses, BMC serial, optionally TPM endorsement key hash)
- Domain membership state (Registered, Active, Inactive, Revoked)
- vCluster assignment (optional, independent of enrollment state)
- Signed certificate metadata (serial, expiry — no private key)

Nodes that are not in the enrollment registry cannot obtain certificates and cannot
establish mTLS connections.

### Enrollment state machine

```
                    enroll (platform-admin)
                 ┌──────────────────────►  Registered
                 │                        (enrollment record created,
                 │                         node hasn't booted yet)
                 │                              │
                 │                              │ node boots, sends CSR
                 │                              │ with matching hw identity
                 │                              ▼
                 │                           Active
                 │                         (CSR signed, mTLS up,
                 │                          streaming boot config)
                 │                              │
                 │                 ┌────────────┤
                 │                 │            │
                 │      subscription stream  admin: decommission
                 │      disconnects + grace     │
                 │      period expires          │
                 │                 │            │
                 │                 ▼            ▼
                 │             Inactive      Revoked
                 │           (node gone,    (cert serial added to
                 │            signed cert    Raft revocation registry,
                 │            may still      record removed, cannot
                 │            be valid)      re-enroll without
                 │                 │          new enrollment)
                 │                 │
                 │                 │ node boots again,
                 │                 │ sends new CSR
                 │                 ▼
                 │              Active
                 │           (new CSR signed,
                 │            new cert issued)
```

Transition constraints:
- **Registered → Active**: only on first `Enroll` call with matching hardware identity.
- **Active → Active**: rejected. Once Active, subsequent `Enroll` calls for the same
  hardware identity return `ALREADY_ACTIVE`. This prevents concurrent enrollment races.
  The real agent already has its cert; a second caller (spoofed or restarted) is rejected.
  If the agent genuinely restarts, it must wait for the heartbeat timeout (→ Inactive)
  before re-enrolling, or reuse its existing cert from RAM if still running.
- **Inactive → Active**: on re-boot with matching hardware identity. New CSR, new cert.

### Bootstrap: hardware identity + CSR, not tokens

The agent's bootstrap credential is its hardware identity — MAC addresses and BMC serial
read from SMBIOS/DMI tables at boot. No bootstrap token injection by Manta is required.

Boot enrollment flow:
1. Admin pre-registers nodes: `pact node enroll <node-id> --mac <mac> --bmc-serial <s>`
2. Journal stores enrollment record in Raft state.
3. Node boots (via Manta, PXE, any mechanism). pact-agent starts.
4. Agent reads its hardware identity from the system (MAC, SMBIOS).
5. Agent generates an ephemeral keypair in memory.
6. Agent calls `EnrollmentService.Enroll(hardware_identity, csr)` on the journal
   (server-TLS-only — the agent does not yet have a client cert).
7. Journal matches hardware identity against enrollment registry.
8. On match (Registered or Inactive): signs CSR with intermediate CA key, returns
   signed cert + current vCluster assignment (if any). Sets state to Active.
9. On match (Active): rejects with `ALREADY_ACTIVE`. Prevents race conditions.
10. On match (Revoked): rejects with `NODE_REVOKED`.
11. On no match: rejects with `NODE_NOT_ENROLLED`.
12. Agent builds mTLS channel using its private key + signed cert.
13. If vCluster assigned: `StreamBootConfig(vcluster_id)` → normal boot.
14. If no vCluster: maintenance mode (domain defaults only).

### Enrollment endpoint security

The enrollment endpoint is the ONLY unauthenticated gRPC endpoint on the journal. Its
attack surface is mitigated by:

1. **Enrollment registry gate.** Only hardware identities pre-registered by a
   platform-admin are served. Unknown identities are rejected immediately.

2. **Once-Active rejection.** Once a node transitions to Active, further `Enroll` calls
   for the same hardware identity are rejected until the node becomes Inactive (heartbeat
   timeout). This narrows the spoofing window to the interval between PXE boot and the
   first successful enrollment (~seconds).

3. **CSR model.** Even if an attacker spoofs hardware identity and wins the enrollment
   race, they get a cert for their own key. The real node will fail to enroll (ALREADY_ACTIVE)
   and alert — making the attack detectable. The attacker cannot impersonate the real node's
   existing connections because they don't have its private key.

4. **Rate limiting.** The enrollment endpoint is rate-limited to N enrollments per minute
   (configurable, default 100). Brute-force identity guessing is impractical.

5. **Server-TLS-only.** The enrollment endpoint requires TLS (server cert validated by
   agent against the domain's CA bundle baked into the SquashFS image) but does not require
   a client cert.

6. **Audit logging.** All enrollment attempts (success and failure) are logged to the
   journal audit trail and forwarded to Loki. Repeated failures for the same hardware
   identity trigger an alert.

7. **TPM attestation (optional).** For high-security deployments, the `Enroll` request
   can include a TPM endorsement key hash or PCR quote, providing cryptographic hardware
   attestation that is not spoofable.

### Heartbeat: subscription stream liveness

Node liveness is detected through the existing config subscription stream
(`BootConfigService.SubscribeConfigUpdates`). When an agent is Active, it maintains a
long-lived streaming connection to the journal. The journal tracks:

- `last_seen`: timestamp of last message received on the subscription stream
- Heartbeat timeout: configurable per domain (default 5 minutes)

When the subscription stream disconnects AND the heartbeat grace period expires without
reconnection, the journal transitions the node from Active → Inactive. This is a Raft
write (auditable).

No separate heartbeat RPC is needed — the subscription stream is already maintained by
every active agent and its connection state is a natural liveness signal.

### Local signing eliminates boot storm and renewal batching

No external service is on the boot path or the renewal path for individual agent certs:

```
Boot storm (T+0, 10,000 nodes simultaneously):
  Agent generates keypair + CSR
  Agent → Journal: Enroll(hardware_id, csr)
  Journal: match enrollment → sign CSR locally with ephemeral CA key
  ^^^^^^ CPU-only operation. ~1ms per signing. No network calls.
  Journal: return signed cert + vCluster assignment

  Agent → Journal: StreamBootConfig(mTLS)
  ^^^^^^ Already served from local state (existing design).
```

External traffic during boot storm: zero.
External traffic during cert renewal: zero (agents send new CSR, journal signs locally).

Certificate revocation is handled entirely within the Raft revocation registry —
revoked serials are replicated to all journal nodes via consensus.

### Certificate lifecycle: 3-day default, agent-driven renewal

Certificate validity: 3 days (configurable per domain). Renewal at 2/3 lifetime (day 2).

Renewal is agent-driven:
1. Agent generates new keypair.
2. Agent calls `EnrollmentService.RenewCert(node_id, current_cert_serial, new_csr)` over
   existing mTLS channel.
3. Journal validates: caller's mTLS identity matches node_id, current_serial matches
   stored cert. Signs new CSR. Returns signed cert.
4. Agent performs dual-channel rotation (see below).

No batch pre-fetching or sweep is needed. Journal signing is local and fast.

### Dual-channel rotation (no operational disruption)

Agent maintains two gRPC channels: active and passive.

```
Day 0: Boot
  Agent generates keypair → CSR → Enroll → signed cert
  Builds active mTLS channel

Day 2: Renewal trigger (2/3 of 3 days)
  1. Agent generates new keypair + CSR
  2. Agent → Journal: RenewCert(node_id, current_serial, new_csr)
  3. Journal signs new CSR, returns signed cert
  4. Agent builds passive channel with new key + new cert
  5. Agent health-checks passive channel (ping journal)
  6. Atomic swap: passive → active, old active → drain
  7. Old channel completes in-flight RPCs, then closes

Day 3: Old cert expires (already swapped out)

If renewal fails (journal unreachable):
  Active channel continues until cert expires.
  Agent enters degraded mode (cached config, invariant A9).
  Keeps retrying. Journal recovery → new CSR signed → reconnect.
```

Shell sessions, exec operations, and boot config subscriptions are unaffected by rotation.

### Multi-domain enrollment (shared hardware)

A node may be enrolled in multiple pact domains simultaneously. This supports the use case
of special hardware (e.g., a node with rare GPU configuration) that is swapped between
domains.

Constraints:
- A node can be **Active** in at most one domain at a time (enforced by physics — it
  boots from one Manta at a time).
- Enrollment in multiple domains is a **reservation**, not an exclusive claim.
- Each domain signs CSRs independently. The agent generates a new keypair per boot, so
  each domain's cert uses a different key.
- When a node disappears from domain A (heartbeat timeout → Inactive) and boots into
  domain B (→ Active), no cross-domain coordination is required.

Optional cross-domain visibility via Sovra: when a domain activates a node, it can publish
a lightweight enrollment claim. Other domains see this and can warn if the same hardware
is active elsewhere. This is advisory, not a hard lock. If Sovra is unavailable, domains
operate independently.

### vCluster assignment: independent of enrollment

vCluster assignment is a separate journal operation. An enrolled, active node can be:
- Assigned to a vCluster → normal operation (streams overlay, applies policy)
- Unassigned → maintenance mode (domain defaults only, no drift detection, not schedulable)
- Moved between vClusters → unassign + assign (atomic journal operation)

The enrollment response includes the current vCluster assignment (if any), so the agent
knows immediately after enrollment whether to stream a vCluster overlay or enter
maintenance mode. No separate query is needed.

The certificate CN is `pact-service-agent/{node_id}@{domain_id}` — no vCluster in the
cert. Moving between vClusters does not touch the cert.

### Maintenance mode (active + unassigned)

An enrolled node with no vCluster assignment operates in maintenance mode under a
domain-default configuration:

- **Services**: pact-agent only. Time sync (chronyd/NTP) if configured in domain defaults.
  No lattice-node-agent, no workload services.
- **Policy**: domain-level default policy. Platform-admin can exec/shell. No
  vCluster-scoped roles active.
- **Drift detection**: disabled (no declared state to drift from).
- **Capability report**: generated but marked `vcluster: None`. Node is not schedulable.
- **Shell/exec**: available to platform-admin. Useful for diagnostics and pre-assignment
  hardware validation.

The domain-default configuration is a minimal `VClusterPolicy` with `enforcement_mode:
"observe"`, empty whitelists (platform-admin bypass only), and no regulated flags. It is
stored in journal config and applied to all unassigned nodes.

### Decommission safety

When decommissioning a node:
1. If active shell sessions or exec operations exist on the node, the decommission
   command warns the admin and requires `--force` to proceed.
2. On `--force` (or no active sessions): enrollment state → Revoked, cert serial added
   to Raft revocation registry, agent's mTLS connection terminates.
3. Active sessions are terminated. Session audit records are preserved.
4. The node cannot re-enroll without a new `pact node enroll` command.

### Batch enrollment

Batch enrollment (`pact node enroll --batch nodes.csv`) is not atomic. Each node is
an independent Raft command. On partial failure:
- Successfully enrolled nodes are in `Registered` state, ready for boot.
- Failed enrollments are reported per-node in the batch response.
- The admin can retry the batch — already-enrolled nodes return
  `NODE_ALREADY_ENROLLED` (idempotent for retry).

## Trade-offs

- (+) No external CA dependency — journal generates ephemeral CA key at startup
- (+) No private keys in Raft state or on the wire — agent holds its own key in RAM
- (+) No dependency on Manta/OpenCHAMI for cert management — pact owns its trust
- (+) Multi-domain shared hardware without distributed locks
- (+) Maintenance mode is a natural state, not an edge case
- (+) Certificate rotation is invisible to operations (dual-channel swap)
- (+) Enrollment registry provides inventory and prevents unauthorized nodes
- (+) Boot storm safe: local signing is CPU-only (~1ms per cert, ~10s for 10,000)
- (+) Self-contained: no external PKI required for certificate operations
- (+) Ephemeral CA key reduces exposure risk (rotated on journal restart, memory-only)
- (-) Enrollment is an additional admin step before first boot
- (-) Journal intermediate CA key is sensitive (mitigated: ephemeral, memory-only,
  rotated on restart; same trust level as journal server TLS key; 3-5 nodes, not 10,000)
- (-) Hardware identity (MAC + BMC serial) is not cryptographically strong without TPM
  (mitigated: sufficient for trusted datacenter environments; TPM optional;
  once-Active rejection limits spoofing window)
- (-) No external CRL distribution — revocation is checked only by journal nodes via
  Raft revocation registry (mitigated: journal nodes are the only mTLS terminators)

## Consequences

- A-I2 (mTLS certificates provisioned by OpenCHAMI) is superseded. Certificate lifecycle
  is pact's responsibility, using self-generated ephemeral CA keys on journal nodes.
- Agent config no longer includes `vcluster`. vCluster assignment comes from the journal.
- pact-journal gains an `EnrollmentService` gRPC endpoint with one unauthenticated RPC
  (`Enroll`) and authenticated RPCs for admin and renewal operations.
- pact-journal nodes generate an ephemeral intermediate CA key at startup (in memory only,
  NOT stored in Raft or on disk).
- pact-journal Raft state gains a revocation registry for revoked cert serials.
- pact-cli gains `pact node` subcommands: `enroll`, `decommission`, `assign`, `unassign`,
  `move`, `list`, `inspect`.
- pact-journal Raft state gains `NodeEnrollment` records (no key material).
- New invariants E1-E10 for enrollment, cert lifecycle, and domain membership.
- Node heartbeat detected via subscription stream liveness (default timeout: 5 minutes).

## Amendment (2026-03-17): SPIRE as Primary mTLS Provider

### Context for amendment

HPE Cray infrastructure uses SPIRE (SPIFFE Runtime Environment) for mTLS workload
attestation. `spire-agent` runs on compute nodes. The original ADR-008 design assumed
pact self-manages all mTLS certificates via an ephemeral intermediate CA. This creates
unnecessary duplication with the existing SPIRE infrastructure.

Additionally, lattice-node-agent also needs mTLS (to lattice-quorum). Both systems
managing their own certificate lifecycle independently is wasteful when SPIRE already
provides this.

### Amendment decision

**SPIRE is the primary mTLS provider. ADR-008's ephemeral CA self-signed model is the
fallback when SPIRE is not deployed.**

The identity acquisition is abstracted via `hpc-identity` crate (ADR-015) with an
`IdentityCascade` that tries providers in order:

1. **SpireProvider** — connect to SPIRE agent socket, obtain X.509 SVID. SPIRE handles
   rotation, attestation, and trust bundle management.
2. **SelfSignedProvider** — ADR-008 model: agent generates keypair + CSR, journal signs
   with intermediate CA. Fallback when SPIRE is not deployed.
3. **StaticProvider** — bootstrap identity from OpenCHAMI SquashFS image. Used for
   initial journal authentication before SPIRE or journal is reachable.

### What changes

| Component | Original ADR-008 | After amendment |
|-----------|-----------------|-----------------|
| Primary cert source | Ephemeral CA via journal | SPIRE SVID |
| Fallback cert source | N/A | Ephemeral CA via journal (ADR-008 model) |
| Bootstrap | Hardware identity + CSR | Same (unchanged) |
| Cert rotation | Agent-driven CSR renewal + dual-channel | SPIRE-managed rotation + dual-channel |
| External CA dependency | None (ephemeral CA) | None (SPIRE manages its own CA) |
| Lattice mTLS | Not addressed | Same IdentityCascade via hpc-identity |

### What survives unchanged

- **Enrollment registry** — hardware identity matching, enrollment states, admin enrollment
- **EnrollmentState machine** — Registered/Active/Inactive/Revoked
- **Bootstrap identity** — used for initial auth before any provider is available
- **Dual-channel rotation pattern** — applicable to both SVID and self-signed rotation
- **Enrollment endpoint security** — rate limiting, once-Active rejection, audit logging
- **Heartbeat via subscription stream** — unchanged
- **Multi-domain enrollment** — unchanged
- **Maintenance mode** — unchanged

### What is demoted to fallback

- **Ephemeral CA on journal nodes** — only needed when SPIRE not deployed
- **Per-agent CSR signing by journal** — only needed when SPIRE not deployed
- **Journal-side cert lifecycle management** — SPIRE manages this when available

### Boot sequence with SPIRE

```
T+0.0s  Kernel + initramfs → mount SquashFS root
T+0.1s  pact-agent starts (PID 1)
T+0.2s  IdentityCascade tries StaticProvider (bootstrap cert from SquashFS)
T+0.3s  Agent authenticates to journal using bootstrap identity
T+0.4s  Agent pulls vCluster overlay from journal
T+0.5s  Agent starts services (including any SPIRE-dependent services)
T+0.8s  IdentityCascade retries: SpireProvider detects SPIRE agent available
T+0.9s  Agent obtains SVID from SPIRE
T+1.0s  CertRotator performs dual-channel swap to SVID
T+1.0s  Bootstrap identity discarded (PB4)
```

If SPIRE agent is never available (standalone deployment): agent continues with
bootstrap identity or SelfSignedProvider (journal-signed cert). All functionality
works (PB5: no hard SPIRE dependency).

### Implications for lattice

lattice-node-agent uses the same `IdentityCascade` from hpc-identity:
- When SPIRE available: obtains SVID for lattice-quorum mTLS
- When SPIRE not available: uses its own cert management (equivalent to ADR-008)
- Both systems share the same `IdentityProvider` trait — no duplication

## Revisit

- If TPM attestation becomes available across the fleet, hardware identity verification
  can be strengthened from MAC+BMC to cryptographic attestation, closing the spoofing
  window entirely.
- If the ephemeral CA model proves insufficient for cross-site trust, an external CA
  (Vault, step-ca, etc.) can be introduced as the CA key source without changing the
  enrollment or CSR signing model.
- If SPIRE is adopted universally across all deployments, the SelfSignedProvider and
  ephemeral CA model can be removed entirely, simplifying the architecture.
