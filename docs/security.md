# STRIDE Threat Model — pact

This document applies the STRIDE threat modeling framework to pact's architecture.
Each section identifies threats by STRIDE category, maps them to components, assesses
residual risk after existing mitigations, and recommends further hardening where gaps
remain.

**Scope**: pact-agent, pact-journal (Raft quorum), pact-policy, pact CLI, pact MCP
server, and the trust boundaries between them. External systems (Vault, OPA, Sovra,
OpenCHAMI/Manta, lattice) are modeled as trust boundary crossings.

**Data flow summary** (for threat identification):

```
                         ┌───────────────────────────────────────────┐
                         │            Vault (PKI CA)                  │
                         │  root CA, intermediate CA issuance         │
                         └─────────────┬─────────────────────────────┘
                                       │ intermediate CA key
                                       ▼
┌─────────────┐  mTLS   ┌─────────────────────────────┐  Raft     ┌───────────────┐
│ pact-agent  │◄───────►│       pact-journal           │◄────────►│ pact-journal   │
│ (compute    │         │  (leader / follower)          │          │ (other peers)  │
│  node)      │         │  ┌──────────┐ ┌────────────┐ │          └───────────────┘
│             │         │  │ PolicySvc│ │ EnrollSvc  │ │
│ observer    │         │  └─────┬────┘ └────────────┘ │
│ supervisor  │         │        │ localhost            │
│ shell srv   │         │        ▼                     │
│ drift eval  │         │  ┌──────────┐                │
└──────┬──────┘         │  │   OPA    │                │
       │                │  └──────────┘                │
       │                └──────────────┬───────────────┘
       │                               │
       │  gRPC+OIDC     ┌─────────────┴────────────────┐
       │                │         pact CLI               │
       │                │  (admin workstation)            │
       │                └──────────────────────────────┘
       │                               │
       │  gRPC+OIDC     ┌─────────────────────────────┐
       └────────────────│       pact MCP server        │
                        │  (AI agent tool-use)          │
                        └──────────────────────────────┘
```

---

## Trust boundaries

| ID | Boundary | Protection |
|----|----------|------------|
| TB1 | Agent ↔ Journal | mTLS (X.509, intermediate CA signed by Vault) |
| TB2 | CLI/MCP ↔ Journal | gRPC + OIDC Bearer JWT |
| TB3 | CLI/MCP ↔ Agent (shell/exec) | gRPC + OIDC Bearer JWT, routed through journal auth |
| TB4 | Journal ↔ OPA sidecar | localhost-only (127.0.0.1:8181), no auth |
| TB5 | Journal ↔ Vault | mTLS or Vault token, periodic (CA renewal only) |
| TB6 | Journal ↔ Sovra | mTLS, federation policy sync |
| TB7 | Agent ↔ OS (eBPF, cgroups, PTY) | Kernel privilege (CAP_SYS_ADMIN, CAP_BPF) |
| TB8 | Enrollment endpoint | Server-TLS only (unauthenticated gRPC) |

---

## S — Spoofing

### S-1: Rogue node enrollment

**Target**: Enrollment endpoint (TB8) — the only unauthenticated gRPC endpoint.

**Threat**: An attacker on the management network spoofs MAC + BMC serial to enroll
a malicious node, obtaining a valid mTLS certificate.

**Existing mitigations**:
- Enrollment registry gate: only pre-registered hardware identities are served (E1).
- Once-Active rejection: after the real node enrolls, duplicates are rejected with
  `ALREADY_ACTIVE` until heartbeat timeout (E7).
- CSR model: even if spoofed, the attacker gets a cert for *their* key — they cannot
  impersonate the real node's existing connections (E4).
- Rate limiting: configurable N enrollments/min (default 100).
- Audit: all enrollment attempts (success/failure) logged + Loki alert on repeated
  failures.

**Residual risk**: **Medium**. MAC + BMC serial are not cryptographically strong.
The spoofing window is narrow (between PXE boot and first enrollment, ~seconds) but
exists. If an attacker has physical or BMC-level access to read identifiers, they can
pre-stage the race.

**Recommendations**:
1. Enable TPM attestation (`tpm_endorsement_key_hash` in enrollment request) for
   high-security deployments — closes the spoofing window entirely.
2. Monitor `NODE_ALREADY_ACTIVE` rejections as a spoofing indicator. Alert on any
   occurrence outside of expected agent restarts.
3. Consider network segmentation: enrollment endpoint accessible only from the PXE/boot
   VLAN, not the general management network.

### S-2: OIDC token theft / replay

**Target**: CLI ↔ Journal (TB2), CLI ↔ Agent (TB3).

**Threat**: Stolen JWT used to impersonate an admin. JWTs are bearer tokens — whoever
holds one is authenticated.

**Existing mitigations**:
- Token cache files at 0600 permissions, strict mode rejects wrong perms (Auth5, PAuth1).
- Refresh tokens never logged (Auth7).
- Token expiry limits replay window.
- RS256 + JWKS in production; HS256 only in dev.
- Per-server token isolation (Auth6).

**Residual risk**: **Medium**. Bearer tokens are inherently stealable from memory,
process environment, or network (if TLS is terminated incorrectly). Standard OIDC risk.

**Recommendations**:
1. Short access token lifetime (5-15 min) to limit replay window.
2. Consider token binding (DPoP) if the IdP supports it — binds tokens to a
   cryptographic key.
3. Audit unusual token usage patterns: same token from different IPs, role escalation
   within a session.

### S-3: Journal impersonation (rogue journal node)

**Target**: Agent ↔ Journal (TB1).

**Threat**: Attacker stands up a fake journal node to intercept agent connections,
capture CSRs, or serve malicious overlays.

**Existing mitigations**:
- Agent validates journal server certificate against CA bundle baked into SquashFS image.
- mTLS: both sides validate. A fake journal without a valid server cert is rejected.
- Raft membership changes require quorum agreement.

**Residual risk**: **Low**. Requires compromising the CA bundle in the boot image
(OpenCHAMI supply chain) or the Vault intermediate CA key.

### S-4: AI agent privilege escalation via MCP

**Target**: MCP server ↔ Journal (TB2).

**Threat**: AI agent (pact-service-ai) attempts to perform operations beyond its
authorized scope — particularly entering emergency mode.

**Existing mitigations**:
- P8: AI agents cannot enter/exit emergency mode (enforced by RBAC).
- MCP server authenticates as `pact-service-ai` principal with limited write
  permissions.
- All operations logged as `author: service/ai-agent/<name>`.

**Residual risk**: **Low**. Policy enforcement is sound. Risk increases if AI agent
credentials are leaked or if OPA policies are misconfigured.

---

## T — Tampering

### T-1: Journal state corruption

**Target**: Raft state machine, WAL files.

**Threat**: Attacker with access to a journal node modifies WAL files, snapshots, or
in-memory state to alter configuration history or policy.

**Existing mitigations**:
- Raft consensus: writes require majority agreement (J7). Tampering a single node's
  WAL is detectable — other replicas hold the correct state.
- Immutability invariant (J2): entries are append-only, never modified.
- Overlays are checksummed (J5).

**Residual risk**: **Low** (requires compromising majority of journal nodes). **High**
if an attacker gains root on the majority of journal nodes.

**Recommendations**:
1. Encrypt WAL at rest (dm-crypt or filesystem-level encryption).
2. Signed Raft entries: journal leader signs each entry with its intermediate CA key.
   Followers and agents can verify entry provenance. This defends against a compromised
   minority node injecting entries.
3. Integrity monitoring on `/var/lib/pact/journal/` (file hashes, inotify alerts).

### T-2: Overlay poisoning during boot stream

**Target**: Boot config streaming (Phase 1 + Phase 2).

**Threat**: Man-in-the-middle modifies overlay data in transit, causing nodes to boot
with malicious configuration.

**Existing mitigations**:
- mTLS protects the stream (TB1).
- Overlay checksums (J5) detect corruption.

**Residual risk**: **Low**. Standard TLS MITM risk.

**Recommendations**:
1. Agent should verify overlay checksum after receipt (defense in depth if TLS
   termination is misconfigured at a load balancer).

### T-3: Shell command injection / whitelist bypass

**Target**: Shell server (pact exec, pact shell).

**Threat**: Attacker crafts input to escape the restricted shell environment or execute
commands outside the whitelist.

**Existing mitigations**:
- pact exec: no shell interpretation. Command + args are fork/exec'd directly.
  Whitelist is checked against the command binary name, not parsed from a string (S1).
- pact shell: rbash prevents PATH changes, absolute path execution, output
  redirection (S3). PATH restricted to symlinks in session-specific directory.
  `BASH_ENV=""`, `ENV=""` prevent startup injection.
- Platform admin bypass is logged (S2, S4).
- Mount namespace (optional) hides sensitive paths.

**Residual risk**: **Medium**. rbash has known escape techniques:
- Exploiting allowed commands that can spawn subshells (e.g., `vi`, `less`, `man`,
  `awk`, `find -exec`). The default whitelist includes `grep` which is safe, but
  custom whitelist additions could introduce escapable binaries.
- LD_PRELOAD or similar environment variables if not scrubbed.
- Exploiting writable directories to place executables (if any exist in PATH scope).

**Recommendations**:
1. Maintain a deny-list of known rbash-escapable binaries (vi, vim, less, man, more,
   awk, nawk, find, env, perl, python, ruby, lua, ed, ftp, gdb, git) and warn
   admins if they are added to a vCluster whitelist.
2. Scrub dangerous environment variables beyond PATH: LD_PRELOAD, LD_LIBRARY_PATH,
   PYTHONPATH, PERL5LIB, etc.
3. Consider seccomp profiles for shell sessions to restrict `execve` to the whitelist
   at the kernel level (defense in depth beyond rbash).
4. Audit shell session transcripts for escape attempt patterns.

### T-4: Tampering with drift detection (observer bypass)

**Target**: State observer (eBPF, inotify, netlink).

**Threat**: Attacker with root on a compute node disables or evades eBPF probes,
inotify watches, or netlink monitoring to make changes invisible to pact.

**Existing mitigations**:
- eBPF probes attached at kernel level (require CAP_BPF to detach).
- Blacklist-based detection: only excluded paths are ignored (D1). Everything else
  is monitored.
- Observer health: if an observer crashes or is killed, the agent should detect it.

**Residual risk**: **High** (if attacker has root). Root on a compute node can:
- Detach eBPF programs (`bpf()` syscall).
- Kill inotify watches.
- Modify the agent process directly.

**Recommendations**:
1. pact-agent should monitor its own observer health — restart crashed observers and
   alert if observers are repeatedly killed.
2. Use eBPF program pinning in bpffs with restricted permissions.
3. IMA (Integrity Measurement Architecture) on the agent binary to detect tampering.
4. Periodic "heartbeat" from observers to the agent — silence is treated as compromise.
5. Accept that root-on-node is a trust boundary: if the attacker has root, they are
   inside the trust perimeter. Focus on detection (anomalous capability reports,
   missing observer heartbeats) rather than prevention.

### T-5: Policy tampering via OPA sidecar

**Target**: Journal ↔ OPA (TB4).

**Threat**: Attacker on a journal node modifies OPA policy bundles or intercepts
localhost REST calls to alter authorization decisions.

**Existing mitigations**:
- OPA runs on localhost only (not network-accessible).
- Fall back to built-in RbacEngine if OPA unavailable.

**Residual risk**: **Medium**. No authentication on the OPA REST API (TB4 is
localhost-only, no auth). A compromised process on the journal node can push arbitrary
Rego policies via OPA's management API.

**Recommendations**:
1. OPA authentication token on the localhost endpoint (OPA supports bearer token auth).
2. Read-only OPA data API — policy bundles loaded from signed files only, management
   API disabled.
3. File integrity monitoring on OPA policy bundle directory.
4. Alternatively, embed OPA as a library (opa-wasm or rego-rs) to eliminate the
   sidecar attack surface entirely.

---

## R — Repudiation

### R-1: Audit log gaps during partition

**Target**: Audit trail continuity (O3).

**Threat**: Actions taken during a network partition are not recorded in the immutable
journal, allowing an operator to deny their actions.

**Existing mitigations**:
- Local logging during partition: all degraded-mode decisions logged locally (A9).
- Replay on reconnect: local logs replayed to journal for audit continuity.
- Shell PROMPT_COMMAND captures every executed command (S4).
- Emergency mode preserves full audit trail (ADR-004).

**Residual risk**: **Low-Medium**. If an agent is compromised during partition, local
logs can be tampered with before replay. The replay mechanism trusts the agent's local
log integrity.

**Recommendations**:
1. Sign local audit entries with the agent's private key. On replay, the journal
   verifies signatures — tampered entries are flagged.
2. Forward local audit entries to a secondary sink (syslog, Loki direct) in addition
   to journal replay, providing an independent record.

### R-2: BMC console access is unaudited by pact

**Target**: Out-of-band access (PAuth4).

**Threat**: When pact-agent is down, the break-glass path is BMC/Redfish console.
Actions taken via BMC are not captured by pact's audit trail until the agent recovers
and detects drift.

**Existing mitigations**:
- Changes made via BMC are detected as "unattributed drift" on agent recovery (F6).
- BMC consoles typically have their own audit log (Redfish event log).

**Residual risk**: **Medium**. There is a temporal gap where actions are unauditable
by pact. Attribution depends on the BMC's own logging (which is outside pact's
control).

**Recommendations**:
1. Document that BMC audit logs must be preserved and correlated with pact drift
   events post-recovery.
2. Consider forwarding BMC/Redfish event logs to the same Loki instance as pact
   events for unified audit.

### R-3: Platform admin actions without second approval

**Target**: Platform admin bypass (P6).

**Threat**: Platform admin performs destructive actions without oversight. Since
platform admin is always authorized and bypasses two-person approval, a single
compromised platform-admin credential has unchecked power.

**Existing mitigations**:
- All platform admin actions are logged (P6).
- Only 2-3 people per site have this role.
- Platform admin scope is visible in audit trail.

**Residual risk**: **Medium**. No preventive control — detection only. A compromised
platform-admin account can do anything.

**Recommendations**:
1. Consider requiring two-person approval for platform-admin on regulated vClusters
   (currently exempt).
2. Time-bound platform-admin access: use short-lived privilege escalation (e.g., Vault
   dynamic credentials) rather than permanent role assignment.
3. Anomaly detection on platform-admin activity: alert on unusual hours, unusual
   volume, unusual target vClusters.

---

## I — Information Disclosure

### I-1: Intermediate CA key exposure on journal nodes

**Target**: Journal intermediate CA signing key.

**Threat**: Compromise of a journal node exposes the intermediate CA key, allowing the
attacker to sign arbitrary agent certificates.

**Existing mitigations**:
- Key is on 3-5 journal nodes (not 10,000 agents) — small blast radius.
- Key is revocable via Vault CRL.
- Agent private keys are NOT stored on journal nodes (E4).

**Residual risk**: **High**. The intermediate CA key is the crown jewel. Compromise
allows minting certificates for any node identity, enabling full impersonation.

**Recommendations**:
1. Store intermediate CA key in HSM or TPM on journal nodes (not filesystem).
2. If HSM is not available, use Vault Agent sidecar with response wrapping — key is
   held in memory only, never written to disk.
3. Rotate the intermediate CA more frequently (weekly instead of monthly) to limit
   exposure window.
4. Monitor Vault CRL for unexpected revocations or issuances.
5. Consider short-lived intermediate CA certs (hours) with automatic renewal — limits
   the window even if the key is stolen.

### I-2: Config overlay data in transit

**Target**: Boot config streams, config subscription updates.

**Threat**: Overlay data may contain sensitive configuration (credentials, API keys,
mount credentials for shared filesystems).

**Existing mitigations**:
- mTLS encrypts all agent ↔ journal traffic.
- Config state never leaves the site (F1 federation invariant).

**Residual risk**: **Low** (assuming TLS is correctly configured). **Medium** if
overlays contain embedded secrets rather than references to a secret store.

**Recommendations**:
1. Document that overlays should reference secret stores (Vault, Kubernetes secrets)
   rather than embedding credentials directly.
2. Scan overlay content for patterns matching secrets (API keys, passwords) and
   warn during `pact apply`.

### I-3: Shell session output exposure

**Target**: Shell/exec output streaming.

**Threat**: Shell output (e.g., `cat /etc/shadow`, `env`) may contain sensitive data.
This data flows over gRPC and is logged in the audit trail.

**Existing mitigations**:
- mTLS/TLS encrypts output in transit.
- Viewer role is read-only; sensitive commands require ops role.
- Output size limit (10MB default).
- Shell whitelist controls which commands are available.

**Residual risk**: **Medium**. An authorized ops user can intentionally exfiltrate
data via shell/exec. The output is logged, making it auditable but not preventable.

**Recommendations**:
1. Consider output redaction for known sensitive patterns (tokens, keys) in audit
   logs — store hash instead of plaintext for sensitive output.
2. DLP-style alerting: flag exec/shell output containing high-entropy strings or
   known secret patterns.

### I-4: Raft state at rest

**Target**: WAL files, snapshots on journal nodes.

**Threat**: Physical access or backup theft exposes all configuration history, policy
state, enrollment records, and audit trail.

**Existing mitigations**:
- No private key material in Raft state (E4).
- Journal nodes are management infrastructure (typically physically secured).

**Residual risk**: **Medium**. Configuration data, policy rules, admin operation
history, and enrollment records are sensitive operational data.

**Recommendations**:
1. Encrypt WAL and snapshots at rest (dm-crypt, LUKS, or filesystem encryption).
2. Encrypt backups before exporting to object storage.
3. Access control on `/var/lib/pact/journal/` — restrict to pact service user.

---

## D — Denial of Service

### D-1: Enrollment endpoint flooding

**Target**: Enrollment endpoint (TB8, unauthenticated).

**Threat**: Attacker floods the enrollment endpoint with fake enrollment requests,
consuming journal CPU (CSR validation, registry lookups) and Raft writes (failed
enrollment audit entries).

**Existing mitigations**:
- Rate limiting: N enrollments/min (default 100).
- Enrollment registry gate: unknown identities rejected immediately (minimal CPU).
- Failed enrollments are logged but may not require Raft writes.

**Residual risk**: **Low-Medium**. Rate limiting helps, but a distributed attack from
many IPs could still cause load. The registry lookup is fast (HashMap), but audit
logging of failures adds overhead.

**Recommendations**:
1. Implement connection-level rate limiting (per-IP) in addition to enrollment-level
   rate limiting.
2. Consider moving failure audit to async/batch writes rather than per-request journal
   entries.
3. Network segmentation: enrollment endpoint accessible only from the PXE/boot VLAN.

### D-2: Boot storm amplification

**Target**: Journal boot config streaming.

**Threat**: Attacker triggers repeated reboots of large node groups, causing sustained
boot storm load on the journal.

**Existing mitigations**:
- Boot config reads do not go through Raft (served from local state, J8).
- Read replicas (learners) absorb load.
- `max_concurrent_boot_streams` limit (default 15,000).
- Overlay caching prevents per-boot recomputation.

**Residual risk**: **Low**. The architecture handles 10,000+ concurrent boots by
design (F11). Triggering reboots requires admin access (reboot is delegated to
OpenCHAMI/Manta).

### D-3: Shell session exhaustion

**Target**: Shell server on agent.

**Threat**: Attacker opens maximum concurrent shell sessions, exhausting PTY
allocation and agent resources.

**Existing mitigations**:
- Configurable max concurrent sessions.
- Session-level cgroup resource limits.
- Stale session cleanup (Closing state timeout).
- Each session requires OIDC authentication + RBAC authorization.

**Residual risk**: **Low**. Requires valid credentials. Legitimate risk if an
authorized account is compromised.

**Recommendations**:
1. Per-identity session limits (not just per-node total).
2. Alert on unusual session counts from a single identity.

### D-4: Raft leader overload via write amplification

**Target**: Journal Raft leader.

**Threat**: Attacker (with valid credentials) submits high-volume write operations
(rapid exec commands, frequent config changes) to overload the Raft leader.

**Existing mitigations**:
- Operations require authentication + authorization.
- Commit window model limits config change frequency.
- Raft leader failover on crash (F8).

**Residual risk**: **Low-Medium**. An authorized ops user could submit rapid exec
commands that each generate Raft entries (ExecLog). The per-command audit write
could be a bottleneck under sustained load.

**Recommendations**:
1. Rate limit write operations per identity per vCluster.
2. Batch audit entries: buffer exec logs and flush periodically rather than one Raft
   write per exec.

---

## E — Elevation of Privilege

### E-1: Viewer role escalation via cached policy

**Target**: Degraded-mode RBAC (ADR-011).

**Threat**: During a network partition, a viewer-role user exploits cached policy to
perform operations that would be denied by the full policy engine.

**Existing mitigations**:
- Cached RBAC is conservative: viewers remain read-only in cached mode (P2, P7).
- Two-person approval fails closed during partition.
- Complex OPA rules fail closed during partition.
- All degraded-mode decisions logged locally and replayed.

**Residual risk**: **Low**. The tiered fail-closed strategy (ADR-011) is well-designed.
Risk exists only if the cached role bindings are stale (e.g., a recently-revoked user
still appears in cache).

**Recommendations**:
1. Short cache TTL for role bindings (e.g., 5 minutes) — agent drops cached
   authorization if it hasn't refreshed within the TTL.
2. On reconnect, replay degraded-mode decisions to the journal. If any decision
   would now be denied by the full policy engine, generate a retroactive alert.

### E-2: Compromised supervised service escaping cgroup

**Target**: Process supervisor, cgroup isolation (TB7).

**Threat**: A service supervised by pact-agent (e.g., lattice-node-agent, a custom
service) escapes its cgroup, gains access to agent resources, or interferes with
other services.

**Existing mitigations**:
- cgroup v2 isolation: per-service cgroup under `pact.slice`.
- Memory limits, CPU quotas per service.
- pact-agent sets PR_SET_CHILD_SUBREAPER.

**Residual risk**: **Medium**. cgroup escapes exist (kernel vulnerabilities). A
compromised service with root inside its cgroup could potentially:
- Manipulate the cgroup filesystem.
- Signal the pact-agent process.
- Access shared tmpfs (capability manifest, unix sockets).

**Recommendations**:
1. Use user namespaces where possible to run services as non-root.
2. Seccomp profiles per supervised service (restrict cgroup-related syscalls).
3. Mount the cgroup filesystem read-only for supervised services.
4. Consider running supervised services in mount namespaces to limit filesystem
   visibility.

### E-3: Emergency mode abuse for extended privilege window

**Target**: Emergency mode lifecycle.

**Threat**: An authorized ops admin enters emergency mode not for a genuine emergency,
but to extend the commit window and suppress auto-rollback, allowing persistent
unauthorized changes.

**Existing mitigations**:
- Emergency mode does NOT expand the whitelist (A10).
- Emergency mode does NOT bypass RBAC (ADR-004).
- Emergency mode does NOT suppress audit logging.
- Stale emergency detection + alerting (F4).
- Reason field is required and logged.

**Residual risk**: **Low-Medium**. Emergency mode legitimately extends the operational
window. Abuse is detectable (reason field, duration, actions taken) but not
preventable — it's a trade-off for operational flexibility.

**Recommendations**:
1. Alert on emergency mode entries with vague reasons.
2. Require manager/peer approval for emergency mode on regulated vClusters (extend
   two-person approval to emergency entry).
3. Post-incident review: all emergency sessions should be reviewed as part of
   operational practice.

### E-4: OPA policy injection for privilege escalation

**Target**: OPA sidecar on journal nodes (TB4).

**Threat**: Attacker with access to a journal node pushes a Rego policy that grants
elevated permissions (e.g., makes all roles equivalent to platform-admin).

**Existing mitigations**:
- OPA is localhost-only.
- Fallback to built-in RbacEngine if OPA is unavailable.
- Federated policies come from Sovra (signed? see below).

**Residual risk**: **High** (if journal node is compromised). OPA management API has
no authentication (TB4). A compromised process can push arbitrary policies.

**Recommendations**:
1. Disable OPA management API. Load policies from signed bundle files only.
2. Policy bundles signed by Sovra or a site-local signing key. OPA verifies signature
   before loading.
3. Built-in RBAC invariants (P1-P8) should be enforced in pact-policy code as a
   floor — OPA can add restrictions but should not be able to relax core invariants.
   This makes the built-in RbacEngine the security baseline, not a fallback.

---

## Summary: Risk Heat Map

| Threat | STRIDE | Severity | Residual Risk | Priority |
|--------|--------|----------|---------------|----------|
| S-1: Rogue node enrollment | Spoofing | High | Medium | Enable TPM attestation |
| S-2: OIDC token theft | Spoofing | High | Medium | Short token lifetime, DPoP |
| S-3: Journal impersonation | Spoofing | High | Low | Existing controls sufficient |
| S-4: AI privilege escalation | Spoofing | Medium | Low | Existing controls sufficient |
| T-1: Journal state corruption | Tampering | Critical | Low | WAL encryption, entry signing |
| T-2: Overlay MITM | Tampering | High | Low | Checksum verification at agent |
| T-3: Shell injection / rbash escape | Tampering | High | Medium | Deny-list escapable binaries, seccomp |
| T-4: Observer bypass (root) | Tampering | High | High | Detection-focused (accept trust boundary) |
| T-5: OPA policy tampering | Tampering | Critical | Medium | Disable mgmt API, signed bundles |
| R-1: Audit gaps during partition | Repudiation | Medium | Low-Medium | Sign local audit entries |
| R-2: BMC unaudited access | Repudiation | Medium | Medium | Correlate BMC logs |
| R-3: Platform admin unchecked | Repudiation | High | Medium | Time-bound escalation |
| I-1: Intermediate CA key exposure | Info Disclosure | Critical | High | HSM or memory-only key |
| I-2: Config data in transit | Info Disclosure | Medium | Low | No embedded secrets in overlays |
| I-3: Shell output exposure | Info Disclosure | Medium | Medium | Output redaction in audit |
| I-4: Raft state at rest | Info Disclosure | Medium | Medium | Encryption at rest |
| D-1: Enrollment endpoint flood | DoS | Medium | Low-Medium | Network segmentation |
| D-2: Boot storm amplification | DoS | Medium | Low | By-design resilience |
| D-3: Shell session exhaustion | DoS | Low | Low | Per-identity limits |
| D-4: Raft write amplification | DoS | Medium | Low-Medium | Rate limiting, batch audit |
| E-1: Cached policy escalation | EoP | High | Low | Short cache TTL |
| E-2: cgroup escape | EoP | High | Medium | User namespaces, seccomp |
| E-3: Emergency mode abuse | EoP | Medium | Low-Medium | Peer approval for regulated |
| E-4: OPA policy injection | EoP | Critical | High | Signed bundles, invariant floor |

## Top 5 Hardening Priorities

1. **Intermediate CA key protection** (I-1): Move to HSM or memory-only storage.
   This is the single highest-impact secret in the system.

2. **OPA sidecar hardening** (T-5, E-4): Disable management API, signed policy
   bundles, enforce built-in RBAC as invariant floor. Currently, localhost access
   to OPA is equivalent to policy bypass.

3. **TPM attestation for enrollment** (S-1): Close the hardware identity spoofing
   gap. Already designed as optional — make it a deployment recommendation for
   production.

4. **Shell environment hardening** (T-3): Maintain deny-list of rbash-escapable
   binaries, scrub dangerous environment variables, add seccomp as defense in depth.

5. **Encryption at rest** (I-4, T-1): WAL, snapshots, and backups should be
   encrypted. Configuration history and audit trails are sensitive operational data.
