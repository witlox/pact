# Pact Failure Modes

Catalog of failure scenarios, expected degradation behavior, and recovery paths.

---

## F1: Journal quorum loss

**Trigger:** Majority of journal nodes unavailable (e.g., 2 of 3, 3 of 5).

**Impact:**
- Writes blocked: no config commits, policy updates, or admin operation records
- Reads continue from surviving replicas (local state machine)
- Boot config streaming continues from surviving replicas
- Agents continue with cached config and cached policy

**Degradation:**
- New nodes cannot register (write needed)
- Commit/rollback operations fail with timeout (exit code 5)
- Two-person approval cannot proceed
- Audit log has gap until quorum restores

**Recovery:**
- Quorum restored when majority returns
- Raft replays missed log entries to recovered nodes
- Pending operations retry automatically

**Detection:**
- Alert: `pact_raft_leader == 0` for > 30 seconds
- Health endpoint returns non-200

---

## F2: PolicyService unreachable (from agent perspective)

**Trigger:** Agent cannot reach journal gRPC endpoint (network partition, journal overload).

**Impact:**
- Full OPA policy evaluation unavailable
- Complex policy rules cannot be evaluated
- Two-person approval cannot be initiated or resolved

**Degradation (cached policy mode):**
- Whitelist checks: honored (local cache)
- Basic RBAC: honored (cached role_bindings)
- Two-person approval: denied (fail-closed)
- Complex OPA rules: denied (fail-closed)
- Platform admin: authorized with cached role (logged)
- All degraded-mode decisions logged locally

**Recovery:**
- Agent reconnects to journal
- Local logs replayed to journal for audit continuity
- Policy cache refreshed

**Detection:**
- Agent logs "degraded authorization mode" events
- Journal metrics show reduced connected agents

---

## F3: Network partition (agent isolated from journal)

**Trigger:** Network failure between compute node and journal quorum.

**Impact:**
- Agent continues operating with cached config
- Drift detection continues locally
- Shell/exec available with cached policy authorization
- Config subscription stream disconnected

**Degradation:**
- Config changes cannot be committed to journal
- New overlays/deltas not received
- Drift events logged locally, not in journal
- Two-person approval denied
- SubscribeConfigUpdates stream broken

**Recovery:**
- Partition heals → agent reconnects
- Config subscription resumes from `from_sequence`
- Locally logged events replayed to journal
- Conflict resolution: timestamp ordering, admin-committed > auto-converge

**Detection:**
- Agent logs partition events
- Journal metrics show agent connection drops

---

## F4: Stale emergency

**Trigger:** Emergency mode window expires without admin ending it (no commit, no rollback, no --end).

**Impact:**
- Node remains in emergency state
- Auto-rollback remains suspended
- Uncommitted changes persist

**Degradation:**
- Alert fires (Loki event + Grafana rule)
- Scheduling hold triggered (lattice cordon via API)
- No auto-recovery — requires admin action

**Recovery:**
- Another admin force-ends: `pact emergency --end --force`
- Requires pact-ops-{vcluster} or pact-platform-admin role
- Uncommitted changes either committed or rolled back as part of force-end

**Detection:**
- Alert: emergency duration exceeds configured window
- Dashboard: emergency sessions panel shows stale session

---

## F5: Rollback with active consumers

**Trigger:** Commit window expires, auto-rollback attempted, but processes hold resources (e.g., open file handles on a mount to be unmounted).

**Impact:**
- Rollback fails (does not proceed)
- Node remains in drifted state
- Commit window effectively frozen

**Degradation:**
- Exit code 10 returned if CLI-initiated
- Journal records failed rollback attempt
- Admin must resolve manually (kill processes, then retry)

**Recovery:**
- Admin identifies and terminates active consumers
- Admin manually commits or rollbacks
- Or: admin enters emergency mode for extended time

**Detection:**
- Journal entry: Rollback with failure detail
- Loki event with active consumer list

---

## F6: pact-agent crash or hang

**Trigger:** Agent process terminates unexpectedly or becomes unresponsive.

**Impact:**
- All agent functions unavailable on that node
- No drift detection, no shell/exec, no capability reporting
- Supervised services continue running (orphaned processes)

**Degradation:**
- BMC console is the only access path (unrestricted bash)
- Changes made via BMC detected as unattributed drift when agent recovers
- lattice-node-agent may detect agent absence via health check and report to scheduler

**Recovery:**
- Agent restarts (PID 1 watchdog, or manual restart via BMC)
- Agent re-authenticates to journal
- Cached config applied, services reconciled
- Unattributed drift from BMC session detected and reported

**Detection:**
- lattice-node-agent health check fails
- Scheduler marks node as degraded
- No capability report updates

---

## F7: OPA sidecar crash

**Trigger:** OPA process on journal node terminates.

**Impact:**
- Complex policy evaluation unavailable on that journal node
- PolicyService.Evaluate() cannot delegate to OPA

**Degradation:**
- PolicyService falls back to cached VClusterPolicy evaluation
- Basic RBAC + whitelist checks still work
- Complex Rego rules return "denied" (fail-closed)
- Agents on other journal nodes unaffected (OPA is per-node)

**Recovery:**
- OPA sidecar restarted (systemd, supervisor, or container)
- OPA loads policy bundles from local filesystem
- Full policy evaluation resumes

**Detection:**
- Journal logs OPA connection failure
- Loki event: "OPA sidecar unreachable"

---

## F8: Raft leader failover

**Trigger:** Current journal leader becomes unavailable.

**Impact:**
- Writes temporarily blocked during leader election (typically < 1 second)
- In-flight writes may fail and need retry

**Degradation:**
- Reads continue from any replica
- Boot streaming continues from any replica
- Writes resume on new leader

**Recovery:**
- Raft elects new leader automatically
- Clients retry writes to new leader
- No data loss (Raft guarantees)

**Detection:**
- `pact_raft_term` increments
- Brief spike in write latency
- Health endpoint shows role change

---

## F9: Stale overlay

**Trigger:** Config committed for a vCluster but overlay not yet rebuilt, and a node boots requesting that vCluster's overlay.

**Impact:**
- Node would receive outdated overlay

**Degradation:**
- Stale detection: overlay version compared to latest config sequence
- If stale: on-demand rebuild triggered before serving
- Slightly slower boot for first node after config change

**Recovery:**
- Overlay rebuilt on-demand, cached for subsequent boots
- Hybrid strategy: rebuild on commit (proactive) + on-demand (reactive)

**Detection:**
- Journal metrics: overlay rebuild latency spike
- Boot stream duration slightly elevated

---

## F10: Sovra federation unreachable

**Trigger:** Cannot connect to Sovra endpoint for policy template sync.

**Impact:**
- No new policy templates received
- Compliance reports cannot be sent

**Degradation:**
- System continues with locally cached Rego templates
- All local functionality unaffected
- Federation is optional (feature-gated)

**Recovery:**
- Sovra connectivity restored
- Next sync interval pulls updated templates
- No manual intervention needed

**Detection:**
- Journal logs sync failure
- Metrics: `pact_federation_sync_failures_total` increments

---

## F11: Boot storm (10,000+ concurrent boots)

**Trigger:** Mass reboot event (power cycle, firmware update, OS update).

**Impact:**
- All nodes request boot config simultaneously
- Journal under heavy read load

**Degradation:**
- Boot config reads do NOT go through Raft (served from local state)
- Any journal replica can serve boot streams
- max_concurrent_boot_streams config (default 15,000) limits per-node connection count
- Overlay caching prevents per-boot recomputation

**Recovery:**
- Self-limiting: as nodes boot, load decreases
- No special recovery needed

**Detection:**
- `pact_journal_boot_streams_active` spikes
- `pact_journal_boot_stream_duration_seconds` may increase slightly

---

## F12: GPU hardware failure

**Trigger:** GPU becomes degraded or fails (ECC errors, thermal throttle, driver crash).

**Impact:**
- CapabilityReport updated immediately
- Scheduler adjusts workload placement via lattice

**Degradation:**
- Degraded GPU: reported, workloads may continue with reduced performance
- Failed GPU: reported, workloads evacuated by scheduler

**Recovery:**
- Hardware replacement or driver reset
- GPU health returns to Healthy
- CapabilityReport updated, scheduler adjusts

**Detection:**
- CapabilityReport.gpus[i].health changes
- CapabilityChange entry in journal
- Loki event for GPU state change

---

## Severity Classification

| Failure | Severity | Auto-Recovery | Human Required |
|---------|----------|---------------|----------------|
| F1: Quorum loss | Critical | Yes (when nodes return) | If hardware failed |
| F2: PolicyService unreachable | High | Yes (on reconnect) | No |
| F3: Network partition | High | Yes (on heal) | If conflict |
| F4: Stale emergency | High | No | Yes (force-end) |
| F5: Rollback active consumers | Medium | No | Yes (kill consumers) |
| F6: Agent crash | High | Partial (restart) | If persistent |
| F7: OPA crash | Medium | Yes (restart) | No |
| F8: Leader failover | Low | Yes (auto-elect) | No |
| F9: Stale overlay | Low | Yes (on-demand rebuild) | No |
| F10: Sovra unreachable | Low | Yes (on reconnect) | No |
| F11: Boot storm | Low | Yes (self-limiting) | No |
| F12: GPU failure | Medium | Partial (detect + report) | If hardware |
