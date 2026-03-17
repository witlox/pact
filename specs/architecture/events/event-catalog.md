# Event Catalog

All events in the pact system, organized by producer. Events are the primary communication mechanism between subsystems.

---

## Journal Events (ConfigEntry types)

Recorded as `ConfigEntry` in the immutable Raft log. Each has `EntryType`, `Scope`, `Identity` (author), timestamp, and optional `StateDelta`.

| EntryType | Producer | Consumer(s) | Trigger | Scope |
|-----------|----------|-------------|---------|-------|
| Commit | Agent (CommitWindowManager) | Journal, CLI (pact log/diff), Loki | Commit window closed by admin | Node or vCluster |
| Rollback | Agent (CommitWindowManager) | Journal, CLI, Loki | Manual rollback or auto-rollback on expiry (A4) | Node or vCluster |
| AutoConverge | Agent (DriftEvaluator) | Journal, CLI, Loki | Drift in `auto_converge_categories` resolved automatically | Node |
| DriftDetected | Agent (StateObserver → DriftEvaluator) | Journal, CLI (pact status), Loki | Drift magnitude exceeds threshold | Node |
| CapabilityChange | Agent (GpuBackend, hardware reporters) | Journal, lattice scheduler | GPU state change, hardware added/removed | Node |
| PolicyUpdate | PolicyService | Journal, all agents (via SubscribeConfigUpdates) | Admin updates VClusterPolicy | vCluster |
| BootConfig | Agent (boot sequence) | Journal | Agent completes boot and applies overlay+delta | Node |
| EmergencyStart | Agent (EmergencySession) | Journal, Loki (alert), lattice (cordon) | Admin enters emergency mode | Node |
| EmergencyEnd | Agent (EmergencySession) | Journal, Loki, lattice (uncordon) | Admin exits or emergency expires | Node |
| ExecLog | Agent (ShellService.exec) | Journal (audit), Loki | Remote command execution | Node |
| ShellSession | Agent (ShellService.shell) | Journal (audit), Loki | Shell session start/end | Node |
| ServiceLifecycle | Agent (ServiceManager) | Journal, CLI (pact status) | Service start/stop/restart/crash | Node |
| PendingApproval | PolicyService | Journal, CLI (pact approve) | Two-person approval requested (P4) | vCluster |
| MergeConflictDetected | Agent (on reconnect) | Journal, CLI (active sessions), Loki | Local changes conflict with journal state on same keys (CR2) | Node |
| MergeConflictResolved | Agent/CLI | Journal, Loki | Admin resolves merge conflict — accept local or journal (CR2) | Node |
| GracePeriodOverwrite | Agent (ConflictManager) | Journal, CLI (active sessions), Loki | Grace period expired, journal-wins applied (CR3) | Node |
| PromoteConflictDetected | CLI (promote workflow) | Journal, Loki | Promote blocked by conflicting local changes on target nodes (CR4) | vCluster |
| NodeEnrolled | CLI (admin) | Journal, Loki | Admin registered a node in enrollment registry (ADR-008) | Global |
| NodeActivated | Agent (boot enrollment) | Journal, Loki | Node booted and received cert (ADR-008) | Node |
| NodeDeactivated | Journal (heartbeat timeout) | Journal, Loki | Node disappeared — heartbeat timeout (ADR-008) | Node |
| NodeDecommissioned | CLI (admin) | Journal, Loki, Vault CRL | Admin decommissioned node, cert revoked (ADR-008) | Node |
| NodeAssigned | CLI (admin/ops) | Journal, agents (via subscription), Loki | Node assigned to vCluster (ADR-008) | Node |
| NodeUnassigned | CLI (admin/ops) | Journal, agents (via subscription), Loki | Node removed from vCluster → maintenance mode (ADR-008) | Node |
| CertSigned | Journal (CaKeyManager) | Journal (Raft state) | CSR signed by journal intermediate CA — new or renewed cert (ADR-008) | Node |
| CertRevoked | Journal (VaultCrlClient) | Journal, Vault CRL | Certificate revoked on decommission (ADR-008) | Node |

**Invariants enforced:**
- J3: Every entry has authenticated Identity (non-empty principal + role)
- J7: All entries go through Raft consensus
- O3: Audit trail never interrupted

---

## Agent Runtime Events

Internal agent events that do NOT go through Raft. These are ephemeral, processed locally.

### DriftEvent

| Field | Type | Description |
|-------|------|-------------|
| timestamp | DateTime<Utc> | When drift was detected |
| source | DriftSource | Ebpf, Inotify, Netlink, Manual |
| dimension | DriftDimension | Mounts, Files, Network, Services, Kernel, Packages, Gpu |
| key | String | Resource identifier (file path, mount point, interface name) |
| detail | String | Change description |

**Producers:** EbpfObserver, InotifyObserver, NetlinkObserver, manual detection
**Consumer:** DriftEvaluator (filters blacklisted paths per D1, computes DriftVector)
**Channel:** `mpsc::Sender<DriftEvent>` — all observers feed same evaluator

**Invariants:**
- D1: Blacklisted paths (`/tmp/**`, `/var/log/**`, `/proc/**`, `/sys/**`, `/dev/**`, `/run/user/**`) filtered before emission
- D5: In observe-only mode, events logged but do not open commit windows

### eBPF Tracepoints

| Tracepoint | DriftDimension | What it detects |
|------------|---------------|-----------------|
| mount/umount | Mounts | Filesystem mount changes |
| sethostname | Network | Hostname changes |
| sysctl writes | Kernel | Kernel parameter modifications |
| module load/unload | Kernel | Kernel module changes |
| file permission changes | Files | chmod/chown on monitored paths |
| network namespace ops | Network | Network namespace creation/deletion |
| cgroup modifications | Services | cgroup hierarchy changes |

---

## Admin Operation Events (Audit Log)

Recorded as `AdminOperation` in `JournalState.audit_log`. Separate from ConfigEntry — these track admin actions, not config state.

| AdminOperationType | Trigger | Identity Required | Two-Person (if regulated) |
|--------------------|---------|-------------------|---------------------------|
| Exec | `pact exec <command>` | pact-ops-{vc} or platform-admin | No |
| ShellSessionStart | `pact shell` | pact-ops-{vc} or platform-admin (higher priv) | No |
| ShellSessionEnd | Shell session closes | Same as start | No |
| ServiceStart | `pact service start <name>` | pact-ops-{vc} or platform-admin | Yes |
| ServiceStop | `pact service stop <name>` | pact-ops-{vc} or platform-admin | Yes |
| ServiceRestart | `pact service restart <name>` | pact-ops-{vc} or platform-admin | Yes |
| EmergencyStart | `pact emergency start` | pact-ops-{vc} or platform-admin (NOT AI: P8) | Yes |
| EmergencyEnd | `pact emergency end` or expiry | Same as start | No |
| ApprovalDecision | `pact approve <id>` / `pact reject <id>` | Different admin than requester (P4) | N/A |

**AdminOperation struct:**
```rust
pub struct AdminOperation {
    pub operation_type: AdminOperationType,
    pub identity: Identity,
    pub node_id: NodeId,
    pub vcluster_id: VClusterId,
    pub timestamp: DateTime<Utc>,
    pub detail: String,
}
```

**Invariant O3:** Audit log entries are never deleted, even during emergency mode or degraded operation.

---

## Observability Events (Loki Streaming)

Journal streams structured JSON events to Loki for external monitoring.

| Event Category | Labels | Key Fields | Alert Trigger |
|----------------|--------|------------|---------------|
| Config commits | component=journal, node_id, vcluster_id | entry_type, scope, author, sequence | — |
| Emergency mode | component=journal, node_id, vcluster_id | reason, admin, window_seconds | Always |
| Stale emergency | component=journal, node_id | started_at, exceeded_window | Yes (F4) |
| Degraded auth | component=policy | mode=degraded, cached_at | Yes (F2) |
| OPA unreachable | component=policy | last_healthy, fallback=cached | Yes (F7) |
| GPU state change | component=agent, node_id | gpu_id, old_state, new_state | If Failed |
| Partition detected | component=agent, node_id | journal_last_seen, cached_seq | Yes (F1) |
| Merge conflict | component=agent, node_id, vcluster_id | conflicting_keys, local_values, journal_values | Yes (F13) |
| Grace period overwrite | component=agent, node_id | overwritten_keys, grace_period_seconds | Yes (F13) |
| Promote conflict | component=cli, vcluster_id | promoting_node, conflicting_nodes, keys | No (blocks until resolved) |
| Boot config streamed | component=journal | node_id, overlay_version, duration_ms | If > 5s |

---

## Event Flow Diagrams

### Drift → Commit Flow
```
Observer ──DriftEvent──→ DriftEvaluator ──DriftVector──→ CommitWindowManager
                              │                                │
                              │ (blacklist filter, D1)         │ (opens window, A1)
                              │ (weight calculation, D4)       │ (formula, A3)
                              ▼                                ▼
                         magnitude > 0?              Admin: commit or rollback
                              │                                │
                              │ (if observe-only: log only)    │ (through Raft, J7)
                              ▼                                ▼
                         Loki event                    ConfigEntry in journal
```

### Admin Operation Flow
```
CLI/MCP ──gRPC──→ Agent (ShellService)
                       │
                       ├── PolicyService.evaluate() ──→ Allow/Deny/RequireApproval
                       │        │
                       │        │ (P1: authenticated, P2: authorized)
                       │        │ (P4: two-person if regulated)
                       │        │ (P6: platform admin always allowed)
                       │        │ (P8: AI blocked from emergency)
                       │        ▼
                       ├── Execute command
                       │        │
                       │        ├── Journal: AdminOperation (audit, O3)
                       │        ├── Journal: ConfigEntry if state-changing (S5)
                       │        └── Loki: structured event
                       ▼
                  Response to caller
```
