# Pact Cross-Context Interactions

Integration points between bounded contexts and external systems.

---

## Internal Context Interactions

### I1: Agent → Journal (Config Subscription)

**Direction:** Agent subscribes to journal for live config updates after boot.
**Protocol:** gRPC streaming (`BootConfigService.SubscribeConfigUpdates`)
**Data flow:**
- Journal pushes: overlay changes, node delta changes, policy updates, blacklist changes
- Agent reconnects with `from_sequence` on interruption
**Failure mode:** F3 (network partition) — agent uses cached config
**Invariants:** J8 (reads from local state), A9 (cached config during partition)

### I2: Agent → Journal (Boot Config)

**Direction:** Agent requests boot config from journal at startup.
**Protocol:** gRPC streaming (`BootConfigService.StreamBootConfig`)
**Data flow:**
- Phase 1: vCluster overlay (~100-200 KB, zstd compressed)
- Phase 2: node delta (< 1 KB)
- ConfigComplete with version + checksum
**Failure mode:** F11 (boot storm) — served from local state, any replica
**Invariants:** J8 (reads from local state), A8 (< 2s boot)

### I3: Agent → Journal (Commit/Rollback)

**Direction:** Agent writes config changes through journal Raft.
**Protocol:** gRPC unary (`ConfigService.AppendEntry`)
**Data flow:**
- Agent sends ConfigEntry (Commit/Rollback/DriftDetected)
- Journal returns sequence number
**Failure mode:** F1 (quorum loss), F3 (partition) — writes blocked
**Invariants:** J7 (Raft for writes), J2 (immutability)

### I4: Agent → Journal (Admin Operations)

**Direction:** Agent records exec/shell/service operations in journal audit log.
**Protocol:** gRPC unary (`ConfigService.AppendEntry` with ExecLog/ShellSession/ServiceLifecycle type)
**Data flow:**
- Agent sends AdminOperation via ConfigEntry
- Audit log appended through Raft
**Failure mode:** F3 (partition) — logged locally, replayed on reconnect
**Invariants:** J3 (authenticated authorship), O3 (audit continuity)

### I3a: Agent → Journal (Partition Reconnect Protocol)

**Direction:** Agent reconnects to journal after partition heal.
**Protocol:** gRPC unary + streaming (combines I3 and I1)
**Data flow:**
1. Agent sends accumulated local changes (unpromoted drift, emergency events, audit logs) via `ConfigService.AppendEntry`
2. Journal detects conflicts: local change keys vs. current journal state for same vCluster/node
3. If conflicts exist: journal returns conflict manifest (affected keys, local values, journal values)
4. Agent pauses convergence for conflicting keys, flags merge conflict (F13)
5. Admin resolves via CLI: accept local or accept journal, per key
6. Grace period timeout (default: commit window duration): journal-wins, local changes logged as overwritten
7. After resolution: agent resumes config subscription via I1 (`SubscribeConfigUpdates` with `from_sequence`)
**Failure mode:** F13 (merge conflict) — see failure modes catalog
**Invariants:** CR1 (local first), CR2 (pause on conflict), CR3 (grace period fallback), O3 (audit continuity)

### I5: CLI → Journal (Config Queries)

**Direction:** CLI queries journal for status, diff, log, overlay.
**Protocol:** gRPC unary + streaming (`ConfigService.*`)
**Data flow:**
- CLI sends query (GetEntry, ListEntries, GetNodeState, GetOverlay)
- Journal responds from local state
**Failure mode:** F1 (quorum loss for writes), F8 (leader failover) — reads from any replica
**Invariants:** J8 (reads from local state)

### I6: CLI → Agent (Exec/Shell)

**Direction:** CLI sends exec/shell requests to agent.
**Protocol:** gRPC unary (`ShellService.Exec`) or bidirectional streaming (`ShellService.Shell`)
**Data flow:**
- CLI sends authenticated request (OIDC token in metadata)
- Agent validates token, checks whitelist, evaluates policy
- Exec: agent fork/execs command, streams stdout/stderr
- Shell: agent spawns rbash PTY, bidirectional stream
**Failure mode:** F6 (agent crash) — session terminated, CLI gets error
**Invariants:** S1 (whitelist), S4 (audit), P1 (authenticated), P2 (authorized)

### I7: Policy (in Journal) → OPA Sidecar

**Direction:** pact-policy library calls OPA for complex policy evaluation.
**Protocol:** REST (localhost:8181)
**Data flow:**
- PolicyService.Evaluate() sends request context to OPA
- OPA evaluates Rego rules against data + input
- Returns allow/deny with reason
**Failure mode:** F7 (OPA crash) — falls back to cached policy
**Invariants:** P7 (degraded mode restrictions)

### I8: Agent → PolicyService (Authorization)

**Direction:** Agent calls PolicyService for operation authorization.
**Protocol:** gRPC unary (`PolicyService.Evaluate`)
**Data flow:**
- Agent sends: identity, scope, action, optional proposed change/command
- PolicyService returns: authorized (bool), policy_ref, denial_reason, optional approval_required
**Failure mode:** F2 (unreachable) — cached policy
**Invariants:** P1-P7

---

## External System Interactions

### E1: Agent → lattice-node-agent (Capability Delivery)

**Direction:** pact-agent writes CapabilityReport, lattice-node-agent reads it.
**Protocol:** tmpfs file (`/run/pact/capability.json`) + unix socket
**Data flow:**
- Agent writes JSON manifest to tmpfs
- lattice-node-agent reads manifest and reports to scheduler
- pact does NOT gRPC-stream directly to scheduler
**Failure mode:** lattice-node-agent not running — capability not reported to scheduler
**Invariant:** A-Int4 (lattice-node-agent mediates)

### E2: OpenCHAMI → pact-agent (Boot Provisioning)

**Direction:** OpenCHAMI provisions base image containing pact-agent + mTLS cert.
**Protocol:** PXE boot + SquashFS image
**Data flow:**
- OpenCHAMI provisions kernel + initramfs + SquashFS root
- SquashFS includes pact-agent binary and mTLS certificates
- pact-agent starts as init (PID 1 or early)
**Failure mode:** Provisioning failure — node doesn't boot at all (outside pact scope)
**Invariant:** A-I2 (certs provisioned by OpenCHAMI)

### E3: pact CLI → Lattice API (Delegation)

**Direction:** CLI delegates drain/cordon to lattice scheduler.
**Protocol:** Lattice Rust client library (gRPC)
**Data flow:**
- `pact drain <node>` → lattice drain API
- `pact cordon/uncordon <node>` → lattice cordon API
**Failure mode:** Lattice unreachable — delegation fails with clear error
**Invariant:** A-Int1 (lattice Rust client exists)

### E4: pact CLI → OpenCHAMI API (Delegation)

**Direction:** CLI delegates reboot/reimage to OpenCHAMI.
**Protocol:** REST (Redfish/Manta API)
**Data flow:**
- `pact reboot <node>` → OpenCHAMI Redfish API
- `pact reimage <node>` → OpenCHAMI Manta API
**Failure mode:** OpenCHAMI unreachable — delegation fails
**Invariant:** A-Int2 (client status unknown — stubbed initially)

### E5: Sovra → pact-policy (Federation Sync)

**Direction:** pact-policy syncs Rego templates from Sovra.
**Protocol:** mTLS + REST (configurable interval, default 300s)
**Data flow:**
- Rego policy templates pulled from Sovra
- Stored locally in `/etc/pact/policies/`
- Loaded into OPA as bundles
- Site-local data pushed separately, never leaves site
**Failure mode:** F10 (Sovra unreachable) — uses cached templates
**Invariant:** F1 (config site-local), F2 (templates federated), F3 (graceful failure)

### E6: Journal → Loki (Event Streaming)

**Direction:** Journal streams structured events to Loki.
**Protocol:** HTTP push (Loki API)
**Data flow:**
- Config commits, admin operations, emergencies → structured JSON → Loki
- Labels: component, node_id, vcluster_id
- Fields: entry_type, scope, author, timestamp, sequence, detail
**Failure mode:** Loki unreachable — events buffered or dropped (optional channel)
**Invariant:** O3 (audit continuity in journal, Loki is secondary)

### E7: Journal → Prometheus (Metrics)

**Direction:** Prometheus scrapes journal metrics endpoint.
**Protocol:** HTTP pull (axum endpoint on port 9091)
**Data flow:**
- Raft metrics: leader, term, log entries, replication lag
- Journal metrics: entries total, boot streams active, stream duration, overlay builds
- Health: /health endpoint
**Failure mode:** Journal down — scrape fails, alert fires
**Invariant:** O2 (port 9091)

---

## Interaction Summary Matrix

| Source | Target | Protocol | Direction | Failure Handling |
|--------|--------|----------|-----------|------------------|
| Agent | Journal (config) | gRPC stream | Subscribe | Cached config |
| Agent | Journal (boot) | gRPC stream | Request | Cached config |
| Agent | Journal (write) | gRPC unary | Push | Block until available |
| Agent | Journal (reconnect) | gRPC unary+stream | Push then subscribe | Merge conflict (F13) |
| Agent | PolicyService | gRPC unary | Request | Cached policy |
| CLI | Journal | gRPC | Request | Timeout (exit 5) |
| CLI | Agent | gRPC | Request | Connection error |
| Policy | OPA | REST localhost | Request | Cached policy |
| Agent | lattice-node-agent | tmpfs + socket | File write | Capability not reported |
| CLI | Lattice | gRPC | Delegate | Error with context |
| CLI | OpenCHAMI | REST | Delegate | Stubbed |
| Sovra | pact-policy | mTLS REST | Pull | Cached templates |
| Journal | Loki | HTTP push | Push | Buffer/drop (optional) |
| Prometheus | Journal | HTTP pull | Scrape | Alert on failure |
