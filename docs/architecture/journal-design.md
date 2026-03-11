# Journal Design

## Overview

pact-journal is the distributed, append-only configuration log. It runs its own
Raft group, independent from lattice's quorum (see ADR-001 for deployment modes).
Single source of truth for declared state.

## Deployment Modes

Two modes for the Raft quorum (see [ADR-001](../decisions/ADR-001-separate-raft-quorum.md)):

**Standalone** (default): pact-journal runs on dedicated management nodes.
Fully independent from lattice infrastructure.

**Co-located**: pact-journal and lattice-server run on the same management nodes,
each with its own Raft group on separate ports. Pact is the incumbent — its quorum
is already running when lattice starts. Lattice configures its own quorum to use
the same node hostnames.

In both modes:
- Independent Raft groups (separate leader election, log, snapshots)
- Separate data directories (`/var/lib/pact/journal`)
- Separate ports (Raft: 9444, gRPC: 9443)
- No protocol-level coupling

## Raft State Machine

The journal's state machine (`JournalState`) is pact-specific:

```rust
pub struct JournalState {
    /// All config entries, indexed by sequence number.
    pub entries: BTreeMap<EntrySeq, ConfigEntry>,
    /// Per-node current config state.
    pub node_states: HashMap<NodeId, ConfigState>,
    /// Per-vCluster active policy.
    pub policies: HashMap<VClusterId, VClusterPolicy>,
    /// Pre-computed boot overlays per vCluster.
    pub overlays: HashMap<VClusterId, BootOverlay>,
    /// Admin operation audit log.
    pub audit_log: Vec<AdminOperation>,
}
```

### What goes through Raft (strong consistency)

- Config commits and rollbacks
- Policy updates
- Emergency mode start/end
- Admin operation records (exec, shell session start/end)

### What does NOT go through Raft

- Boot config streaming reads (served from any replica, including learners)
- Drift detection events (written locally, forwarded to journal asynchronously)
- Capability reports (sent to lattice scheduler, not journal)
- Telemetry (Loki/Prometheus, not Raft)

## Command Set

```rust
pub enum JournalCommand {
    /// Append a new config entry (commit, rollback, policy update, etc.)
    AppendEntry(ConfigEntry),
    /// Update a node's config state (committed, drifted, emergency, etc.)
    UpdateNodeState { node_id: NodeId, state: ConfigState },
    /// Set or update a vCluster policy.
    SetPolicy { vcluster_id: VClusterId, policy: VClusterPolicy },
    /// Store a pre-computed boot overlay for a vCluster.
    SetOverlay { vcluster_id: VClusterId, overlay: BootOverlay },
    /// Record an admin operation (exec log, shell session).
    RecordOperation(AdminOperation),
}
```

## Log Structure

ConfigEntry: sequence, timestamp, entry_type, scope, author (OIDC identity),
parent (chain for state reconstruction), state_delta, policy_ref, ttl, emergency_reason.

Entry types: Commit, Rollback, AutoConverge, DriftDetected, CapabilityChange,
PolicyUpdate, BootConfig, EmergencyStart, EmergencyEnd, ExecLog, ShellSession,
ServiceLifecycle.

Note: ExecLog and ShellSession are new entry types — every remote command and shell
session is recorded in the same immutable log as configuration changes.

## Streaming Boot Config

Two-phase protocol:
- Phase 1: vCluster base overlay (pre-computed, compressed ~100-200 KB, served from any replica)
- Phase 2: node-specific delta (<1 KB)

Phase 2 includes **both** pre-declared per-node config **and** any previously
committed manual changes stored in `node_states`. This means admin changes
committed via `pact commit` survive reboots — they are reapplied from the
journal alongside the vCluster overlay.

Read replicas (non-voting Raft learners) for 100k+ boot storms. Boot config reads
do not go through Raft consensus — they read from the local state machine snapshot.
This is why boot storms do not block the Raft group.

### Overlay Pre-Computation

Hybrid commit + on-demand strategy:
- **On commit**: when a config commit or policy update affects a vCluster, the overlay
  is rebuilt and stored via `SetOverlay` through Raft. This ensures overlays are warm
  for the common case (steady-state boots after config changes).
- **On demand**: if a boot request arrives for a vCluster with no cached overlay (e.g.,
  first boot after journal restore, or new vCluster), the overlay is built on the fly,
  then stored for subsequent requests.
- Overlays are compressed (zstd) and checksummed. Stale overlays are detected by
  comparing the overlay version against the latest config sequence for that vCluster.

## Storage

```
/var/lib/pact/journal/
  raft/
    vote.json                          # Persisted vote state
    committed.json                     # Last committed log ID
    wal/
      {index}.json                     # Per-entry WAL files
    snapshots/
      snap-{term}-{index}.json         # State snapshots (keep 3 most recent)
```

## Telemetry

- Config events → Loki (structured JSON with labels)
- Server metrics → Prometheus (Raft health, stream throughput, event counts)

## Backup

WAL + periodic snapshots + export to object storage (S3/NFS).
Full state reconstruction from any snapshot + subsequent WAL.
