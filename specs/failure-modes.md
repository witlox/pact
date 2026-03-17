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

## F18: CA key rotation on journal restart

**Trigger:** Journal restarts and regenerates its ephemeral CA (or loads from disk).

**Impact:**
- Previous CA is no longer valid for signing
- Agents with certs signed by previous CA must re-enroll to get certs from new CA
- Existing mTLS connections using old certs will fail on next reconnect

**Degradation:**
- Agents detect mTLS failure and trigger re-enrollment automatically
- Agents with valid SPIRE SVIDs are unaffected (SPIRE is primary identity provider)
- Brief enrollment storm as agents re-enroll with new CA

**Recovery:**
- Agents re-enroll automatically: generate new keypair + CSR, journal signs with new CA
- SPIRE-managed agents unaffected — SPIRE identity continues working

**Detection:**
- Journal logs: "CA regenerated, agents will re-enroll"
- Spike in enrollment requests after journal restart

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

## F21: Supervision loop detects crashed service

**Trigger:** A supervised service (e.g., nvidia-persistenced, lattice-node-agent) crashes or exits unexpectedly.

**Impact:**
- Service unavailable until restarted
- Dependent services may be affected (e.g., GPU workloads if nvidia-persistenced dies)

**Degradation:**
- Supervision loop detects exit via `try_wait()` within poll interval
- RestartPolicy evaluated: Always → immediate restart, OnFailure → restart on non-zero exit, Never → mark Stopped
- Restart count incremented, restart delay applied
- AuditEvent emitted with service name, exit code, restart count

**Recovery:**
- Automatic restart per policy
- If restart fails repeatedly, service remains Failed — admin notified
- No max restart count by default (configurable)

**Detection:**
- AuditEvent: service crash + restart
- CapabilityReport updated if service affects capabilities

---

## F22: cgroup creation failure

**Trigger:** Cannot create cgroup scope for a service (filesystem error, permission issue, cgroup hierarchy corruption).

**Impact:**
- Service cannot start (no resource isolation)
- CgroupHandle callback notifies Process Supervision of failure

**Degradation:**
- Service start fails with descriptive error
- Other services unaffected (each has independent cgroup scope)
- AuditEvent emitted

**Recovery:**
- Admin investigates cgroup filesystem state
- Possible: remount cgroup2, or reboot node

**Detection:**
- Service state transitions to Failed
- AuditEvent with cgroup creation error detail

---

## F23: Hardware watchdog timeout (pact-agent hang)

**Trigger:** pact-agent stops petting `/dev/watchdog` (hang, deadlock, infinite loop — not crash, since crash would also stop petting).

**Impact:**
- Watchdog timer expires
- BMC triggers hard reboot of the node
- All supervised services killed (unclean shutdown)
- In-progress workloads lost

**Degradation:**
- Hard reboot — no graceful shutdown
- Node reboots and pact-agent starts fresh
- Journal detects node went Inactive (heartbeat timeout) then Active again (re-enrollment)
- Unattributed drift detected for any state changes before hang

**Recovery:**
- Automatic via reboot + re-enrollment
- Admin should investigate root cause of hang (logs, core dump if available)

**Detection:**
- Journal: node went Inactive then Active in quick succession
- BMC logs: watchdog timeout event

---

## F24: UidMap exhaustion (UID range full)

**Trigger:** On-demand UID assignment attempted but the org's UID range is exhausted.

**Impact:**
- New user authentication succeeds (OIDC is fine) but UID assignment fails
- User cannot access NFS mounts (no POSIX identity)
- Existing users unaffected

**Degradation:**
- Authentication succeeds but operations requiring UID fail with clear error
- Alert: range exhaustion event
- Admin must extend the org's UID range in journal policy

**Recovery:**
- Admin extends range: `pact policy uid-range extend --org local --end 59999`
- New assignments resume immediately

**Detection:**
- AuditEvent: UID range exhaustion
- Journal metrics: range utilization percentage

---

## F25: NSS module cannot read .db files

**Trigger:** `/run/pact/passwd.db` or `group.db` is missing, corrupted, or has wrong permissions.

**Impact:**
- All NSS lookups via pact fail (getpwnam, getpwuid, getgrnam, getgrgid)
- NFS file operations may fail or show numeric UIDs instead of names
- `/etc/nsswitch.conf` falls through to next source (files)

**Degradation:**
- System users (/etc/passwd) still resolve
- pact-managed users show as numeric UIDs
- pact-agent should detect and rewrite .db files from cached UidMap

**Recovery:**
- pact-agent rewrites .db files from in-memory UidMap cache
- If UidMap cache is empty: re-pull from journal

**Detection:**
- pact-agent health check: .db file integrity
- User reports: numeric UIDs on NFS files

---

## F26: SPIRE agent unreachable

**Trigger:** SPIRE agent socket not available when pact-agent tries to obtain SVID.

**Impact:**
- pact-agent continues with bootstrap identity (or journal-signed cert)
- Full SPIRE-managed mTLS not available

**Degradation:**
- Bootstrap identity provides journal connectivity
- Periodic retry for SPIRE SVID
- No functional impact if journal accepts bootstrap identity

**Recovery:**
- SPIRE agent becomes available → pact-agent obtains SVID → rotates to SPIRE-managed mTLS
- Fully automatic, no admin intervention

**Detection:**
- pact-agent logs: "SPIRE agent unreachable, using bootstrap identity"
- Periodic retry log entries

---

## F27: Namespace handoff failure (pact → lattice)

**Trigger:** Unix socket connection between pact-agent and lattice-node-agent fails, or FD passing fails.

**Impact:**
- Allocation cannot start in prepared namespace
- lattice-node-agent falls back to self-service mode (creates own namespaces if capable)

**Degradation:**
- In fallback mode: lattice creates namespaces without pact's cgroup enforcement
- Reduced isolation guarantees
- AuditEvent emitted

**Recovery:**
- Socket reconnection on next allocation request
- If persistent: admin investigates socket path, permissions, process state

**Detection:**
- AuditEvent: namespace handoff failure
- lattice-node-agent logs: "falling back to self-service namespace creation"

---

## F28: Network configuration failure (netlink)

**Trigger:** Netlink interface configuration fails during boot (wrong config, driver issue, hardware failure).

**Impact:**
- Network-dependent boot phases blocked (PB3: strict ordering)
- Node cannot reach journal, cannot pull overlay
- Boot sequence stuck at ConfigureNetwork phase

**Degradation:**
- Node is unreachable
- Boot fails — no services started
- BMC console is the only access path

**Recovery:**
- Admin investigates via BMC console
- Fix network config in overlay or hardware issue
- Node reboots with corrected config

**Detection:**
- Node never reaches Active enrollment state
- Journal logs: no heartbeat from node after expected boot time

---

## F29: Cascade service failure (dependency chain)

**Trigger:** A service that other services depend on crashes (e.g., dbus-daemon crashes, nv-hostengine depends on it).

**Impact:**
- Dependent services may malfunction or crash in cascade
- Multiple services entering Failed state simultaneously

**Degradation:**
- Supervision loop restarts the root-cause service first (lowest order number)
- Dependent services are restarted after their dependency is Running
- Dependency ordering is re-evaluated on each restart cycle
- If root-cause service keeps failing: dependents remain Failed

**Recovery:**
- Root-cause service recovers → dependents restart in order
- Admin investigates if root-cause service enters restart loop

**Detection:**
- Multiple AuditEvents for service crashes in rapid succession
- CapabilityReport shows multiple services Failed

**Blast radius:** Limited to the dependency chain of the failed service. Services in other slices or without dependency are unaffected.

---

## F30: cgroup.kill failure (scope cleanup)

**Trigger:** cgroup.kill fails to terminate processes in a scope (kernel bug, zombie processes, D-state processes stuck in I/O).

**Impact:**
- CgroupScope cannot be fully released
- Orphaned processes may consume resources
- New cgroup scope for service restart may coexist with old one

**Degradation:**
- Log error with PID list of unkillable processes
- Mark scope as "zombie" — do not reuse
- Create new scope for service restart in same slice
- Zombie scopes cleaned up on next reboot

**Recovery:**
- Reboot clears all zombie scopes
- D-state processes resolve when I/O completes (or hardware is fixed)

**Detection:**
- AuditEvent: "cgroup.kill failed, zombie scope"
- Monitoring: zombie scope count metric

**What must NEVER happen:** pact-agent must never hang waiting for cgroup.kill. Use a timeout — if kill doesn't complete within 10s, log and move on.

---

## F31: Mount refcount inconsistency

**Trigger:** Bug or crash timing causes MountRef refcount to diverge from actual allocation count (undercount: mount unmounted while still in use; overcount: mount never unmounted).

**Impact:**
- Undercount: active allocation loses access to mounted filesystem — data access failure
- Overcount: mount leaks, consuming kernel resources

**Degradation:**
- Undercount is the dangerous case — detected by allocation I/O errors. pact re-mounts and corrects refcount.
- Overcount is benign — detected by periodic reconciliation (mount table scan vs. journal state). Corrected by adjusting refcount.

**Recovery:**
- Periodic reconciliation (on supervision loop idle tick) scans mount table against active allocations
- Discrepancies logged and corrected automatically

**Detection:**
- I/O errors from allocations (undercount)
- Mount table growing unboundedly (overcount)
- Reconciliation audit log entries

**What must NEVER happen:** Refcount must never go negative. Negative refcount is a bug — assert and log, do not unmount.

---

## F32: UidMap propagation lag

**Trigger:** New UID assigned via journal Raft, but not yet propagated to all agents (eventual consistency window).

**Impact:**
- User authenticates successfully (OIDC valid) but NSS lookup fails on nodes that haven't received the update
- NFS file access may fail or show numeric UIDs on lagging nodes

**Degradation:**
- Lag is typically sub-second (journal subscription push)
- On lagging nodes: getpwnam returns not-found, falls through to "files" in nsswitch.conf
- User retries after brief delay — second attempt succeeds

**Recovery:**
- Self-healing: journal subscription delivers update within seconds
- If subscription is broken (F3 partition): cached UidMap used, new users unresolvable until reconnect

**Detection:**
- Agent logs: UidMap subscription lag metric
- User reports: "identity not found" error that resolves on retry

**Blast radius:** Limited to the propagation window (sub-second in normal operation). Only affects the newly assigned user, not existing users.

---

## F33: Boot phase retry exhaustion

**Trigger:** A boot phase fails repeatedly (e.g., network driver keeps failing, journal permanently unreachable on first boot with no cache).

**Impact:**
- Node stuck in BootFailed state indefinitely
- No services started, node not schedulable

**Degradation:**
- Exponential backoff on retries (1s, 2s, 4s, ... up to 60s max)
- After configurable max retries (default: 30), agent stops retrying and remains in BootFailed
- BMC console is the only access path
- If hardware watchdog is active: watchdog continues being petted during retry loop (agent is alive, just stuck in boot)

**Recovery:**
- Admin investigates via BMC console
- Fix underlying issue (network, journal availability, hardware)
- Agent restart or node reboot to retry boot

**Detection:**
- Node never reaches Active enrollment state
- Agent logs: "boot phase X failed, retry N/30"
- Journal: no heartbeat from node

**What must NEVER happen:** Boot retry loop must not prevent watchdog petting. A node stuck in boot should not trigger BMC reboot — the agent is alive and diagnosable via BMC console.

---

## F34: Emergency override failure

**Trigger:** Admin requests emergency freeze/kill of workload.slice, but the operation fails (cgroup freeze not supported on kernel, or processes in D-state).

**Impact:**
- Runaway workload continues consuming resources despite emergency mode
- Admin cannot regain control of node resources through pact

**Degradation:**
- Log failure with detail (kernel version, process states)
- Suggest escalation: BMC reboot via OpenCHAMI
- Emergency mode remains active (does not auto-close on failure)
- Admin can try individual process SIGKILL as alternative

**Recovery:**
- BMC reboot as last resort
- Or: wait for D-state processes to resolve, retry freeze

**Detection:**
- AuditEvent: "emergency freeze failed"
- Admin CLI shows error with suggested actions

---

## F35: Namespace leak

**Trigger:** NamespaceSet not cleaned up after allocation ends — pact misses the cgroup-empty event (race condition, or cgroup notification lost).

**Impact:**
- Leaked namespace FDs consume kernel resources (file descriptors, network namespace state)
- Over time: file descriptor exhaustion on the node

**Degradation:**
- Periodic reconciliation (idle supervision tick) scans /proc for orphaned namespaces
- Orphaned namespaces (no processes, no active allocation in journal) are cleaned up
- AuditEvent emitted for each leaked namespace found

**Recovery:**
- Automatic via periodic reconciliation
- Worst case: node reboot clears all namespaces

**Detection:**
- Reconciliation audit log: "orphaned namespace cleaned up"
- Monitoring: namespace count metric trending upward

**Blast radius:** Gradual resource leak, not immediate failure. Reconciliation prevents accumulation.

---

## F36: Simultaneous multiple service failures

**Trigger:** Multiple services crash simultaneously (e.g., kernel OOM kills several services, or shared dependency failure).

**Impact:**
- Multiple services need restart
- Supervision loop processes one crash per tick — all are detected but restart is sequential

**Degradation:**
- Services restarted in dependency order (lowest order first)
- Independent services (no dependency relationship) may restart in parallel
- If OOM was the cause: cgroup limits should prevent recurrence after restart
- CapabilityReport updated to reflect multiple Failed services

**Recovery:**
- Automatic restart per policy in dependency order
- If OOM caused by pact-agent itself: agent is protected by OOMScoreAdj=-1000 (RI4)
- If OOM caused by workload: workload.slice processes killed first (kernel cgroup OOM priority)

**Detection:**
- Burst of AuditEvents for service crashes
- Kernel OOM messages in dmesg (if OOM was cause)
- CapabilityReport shows degraded state

**What must NEVER happen:** pact-agent itself must not be OOM-killed. RI4 (OOMScoreAdj=-1000) ensures this. If pact-agent IS killed despite this, the hardware watchdog triggers reboot (F23).

---

## Unacceptable Failure Behaviors

The following must NEVER happen, regardless of failure scenario:

| Rule | Context | Rationale |
|------|---------|-----------|
| pact-agent OOM-killed | RI4 | OOMScoreAdj=-1000. If violated, watchdog reboots (F23). |
| Agent hang blocks watchdog | PS2, F33 | Boot retry loop must still pet watchdog. Only a true hang triggers reboot. |
| Orphaned cgroup scope leaks permanently | RI5, PS3 | Callback + cgroup.kill ensures cleanup. Zombie scopes cleared on reboot (F30). |
| MountRef refcount goes negative | WI2 | Assert and log. Never unmount on negative. |
| Silent UID collision across orgs | IM2 | Ranges non-overlapping by construction. Sequential assignment, no hash. |
| Write to another system's cgroup slice outside emergency | RI1, RI3 | Requires emergency mode + OIDC auth + audit. No exceptions. |
| Boot phase skipped | PB3 | Strict ordering. Phase failure blocks all subsequent phases. |
| Network call from NSS module | IM5 | libnss_pact.so is read-only mmap. Never blocks on I/O. |
| Audit trail gap (except during crash) | O3 | All operations logged. Local log during partition, replayed on reconnect. |
| Bootstrap identity used after SVID obtained | PB4 | Discarded immediately. Never stored persistently. |

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
| F18: CA key rotation on journal restart | Medium | Yes (agents re-enroll) | No (SPIRE-managed agents unaffected) |
| F19: Journal unreachable (renewal) | High | Partial (1-day buffer) | If prolonged |
| F20: Hardware identity mismatch | Medium | No | Yes (re-enroll or fix boot) |
| F21: Supervised service crash | Medium | Yes (supervision loop) | If persistent |
| F22: cgroup creation failure | High | No | Yes (investigate fs) |
| F23: Watchdog timeout (agent hang) | Critical | Yes (BMC reboot) | Yes (investigate cause) |
| F24: UID range exhaustion | Medium | No | Yes (extend range) |
| F25: NSS .db file corruption | Low | Yes (rewrite from cache) | No |
| F26: SPIRE agent unreachable | Low | Yes (retry + bootstrap fallback) | No |
| F27: Namespace handoff failure | Medium | Partial (lattice self-service fallback) | If persistent |
| F28: Network config failure | Critical | No | Yes (BMC console) |
| F29: Cascade service failure | High | Yes (dependency-ordered restart) | If root cause persists |
| F30: cgroup.kill failure | Medium | Partial (zombie scope, reboot clears) | If persistent |
| F31: Mount refcount inconsistency | Medium | Yes (periodic reconciliation) | No |
| F32: UidMap propagation lag | Low | Yes (sub-second self-healing) | No |
| F33: Boot phase retry exhaustion | High | Partial (retries with backoff) | Yes (BMC investigation) |
| F34: Emergency override failure | Critical | No | Yes (BMC reboot) |
| F35: Namespace leak | Low | Yes (periodic reconciliation) | No |
| F36: Simultaneous multi-service failure | High | Yes (dependency-ordered restart) | If OOM persists |
