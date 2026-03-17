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
- Agent feeds back unpromoted local changes to journal BEFORE accepting journal state (CR1)
- If local changes conflict with journal state on same config keys → merge conflict (F13)
- Non-conflicting local changes are recorded in journal, then agent syncs to current state
- Config subscription resumes from `from_sequence`
- Locally logged audit events replayed to journal for audit continuity

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

## F13: Merge conflict on partition reconnect

**Trigger:** Partitioned agent reconnects with unpromoted local changes that conflict with journal state on the same config keys.

**Impact:**
- Agent pauses convergence — does not apply journal state for conflicting keys
- Node remains in locally-drifted state until conflict resolved
- Non-conflicting config keys sync normally

**Degradation:**
- Node operational but not converged to vCluster overlay for conflicting keys
- Admin notification if active CLI session exists (CR5)
- Scheduling may be affected if capability diverges

**Recovery:**
- Admin resolves conflict: accept local (promote to journal) or accept journal (overwrite local)
- Grace period timeout (default: commit window duration) → journal-wins fallback (CR3)
- Overwritten local changes logged for audit regardless of resolution path

**Detection:**
- Agent logs merge conflict with affected keys
- Journal records conflict event with both local and journal values
- Loki event: "merge conflict on reconnect" with node_id, vcluster_id, affected keys

---

## F14: Promote conflicts with local node changes

**Trigger:** Admin promotes a node delta to vCluster overlay, but other nodes in the vCluster have local changes on the same config keys.

**Impact:**
- Promote workflow pauses at conflict acknowledgment step
- Promotion does not proceed until admin resolves each conflicting key

**Degradation:**
- Promote blocked, but no data loss
- Existing node configurations unchanged until resolution

**Recovery:**
- Promoting admin explicitly accepts or overwrites each conflict (CR4)
- Accept: keep the target node's local value (it becomes a per-node delta)
- Overwrite: apply promoted value, local change is superseded and logged

**Detection:**
- CLI displays conflict summary during promote workflow
- Journal records promote-with-conflicts event

---

## F15: IdP unreachable

**Trigger:** Keycloak or other OIDC provider is down or unreachable from user's client machine.

**Impact:**
- No new logins possible (no token issuance)
- No token refresh when current tokens expire
- After last valid access token expires, no authenticated CLI access

**Degradation:**
- Existing valid access tokens continue to work until expiry (server validates JWT offline via cached JWKS)
- Cached OIDC discovery document used for flow selection
- PACT admins use break-glass (pact emergency)
- Lattice users cannot authenticate; scheduling commands fail

**Recovery:**
- IdP restored → users run login again
- No data loss, no state corruption

**Detection:**
- Login command reports IdP connection failure
- Token refresh failures logged as warnings

---

## F16: Token cache deleted or corrupted

**Trigger:** User's local token cache file is removed, corrupted, or has wrong permissions.

**Impact:**
- All authenticated commands fail until re-login
- No data loss (cache is derived state, re-creatable via login)

**Degradation:**
- Fail closed: no attempt to use corrupted tokens
- Clear error message directing user to run login

**Recovery:**
- User runs login again
- Cache recreated with correct permissions

**Detection:**
- CLI reports authentication required with reason (missing/corrupt/permissions)

---

## F17: IdP discovery document stale

**Trigger:** Cached OIDC discovery document contains outdated endpoint URLs or signing key references after IdP reconfiguration.

**Impact:**
- Login may fail (wrong token endpoint)
- Token refresh may fail (wrong token endpoint)

**Degradation:**
- On any auth failure: clear cached discovery, report error, suggest retry
- Manual IdP config override available as fallback

**Recovery:**
- IdP reachable → fresh discovery document fetched on next attempt
- Manual config override bypasses discovery entirely

**Detection:**
- Auth failure after previously successful login suggests stale discovery

---

## F18: Vault unreachable for journal CA key rotation

**Trigger:** Journal's intermediate CA cert is approaching expiry and Vault is unavailable to renew it.

**Impact:**
- Journal can still sign CSRs with current CA key until it expires
- If CA cert expires: journal cannot sign new CSRs, new boot enrollments and cert renewals fail
- Existing mTLS connections continue (already established)

**Degradation:**
- Journal logs warning: "CA cert expiring, Vault unreachable"
- Boot enrollments and renewals fail with "CA_CERT_EXPIRED" until Vault returns
- Agents with valid certs continue operating normally

**Recovery:**
- Vault restored → journal obtains renewed CA cert + key
- Pending enrollments can retry immediately

**Detection:**
- Alert: `pact_ca_cert_days_remaining < 7`
- Journal health endpoint reports CA cert status

---

## F19: Journal unreachable during agent certificate renewal

**Trigger:** Agent's cert is approaching expiry but journal is unreachable for CSR signing.

**Impact:**
- Agent cannot obtain a renewed certificate
- Active mTLS channel continues until cert expires
- 1-day renewal window (2/3 of 3-day lifetime) provides buffer

**Degradation:**
- Agent retries renewal on configurable interval (default 5 minutes)
- Active channel continues serving — no operational impact during buffer window
- If cert expires before journal returns: agent enters degraded mode (cached config, A9)

**Recovery:**
- Journal reachable → agent sends new CSR → signed locally → dual-channel swap
- Agent in degraded mode re-enrolls with new CSR

**Detection:**
- Agent logs: "cert renewal failed, retrying" with expiry countdown
- Alert: journal metrics show nodes with certs expiring < 24h

---

## F20: Hardware identity mismatch at boot

**Trigger:** Agent boots and presents hardware identity that doesn't match any enrollment record (wrong node on wrong network, or hardware changed).

**Impact:**
- Agent cannot obtain mTLS certificate
- Agent cannot connect to journal
- Node starts in fully disconnected mode (no config, no policy, no services)

**Degradation:**
- Agent logs: "enrollment rejected: NODE_NOT_ENROLLED"
- Agent retries enrollment periodically (in case of transient registry issue)
- No cached config to fall back to (first boot) — node is inert

**Recovery:**
- Admin investigates: wrong boot target, or hardware needs re-enrollment
- If hardware changed: `pact node decommission` old + `pact node enroll` new
- If wrong Manta: fix boot configuration in Manta

**Detection:**
- Alert: journal logs `NODE_NOT_ENROLLED` rejections
- `pact node list --state registered` shows nodes that never activated

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
| F13: Merge conflict on reconnect | Medium | Partial (grace period fallback) | If conflict |
| F14: Promote conflicts | Low | No (blocks until resolved) | Yes (acknowledge) |
| F15: IdP unreachable | High | Yes (when IdP returns) | No (break-glass for admins) |
| F16: Cache deleted/corrupted | Low | Yes (re-login) | No |
| F17: Stale discovery doc | Low | Yes (clear + refetch) | No |
| F18: Vault unreachable (CA rotation) | Medium | Yes (current CA key continues) | If CA cert expires |
| F19: Journal unreachable (renewal) | High | Partial (1-day buffer) | If prolonged |
| F20: Hardware identity mismatch | Medium | No | Yes (re-enroll or fix boot) |
