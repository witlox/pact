# ADR-001: Raft Quorum Deployment Modes

## Status: Accepted (supersedes original ADR-001)

## Context

pact-journal needs a consensus mechanism for its immutable configuration log.
Lattice already runs a Raft quorum for node ownership and sensitive audit.

Both systems use the same Raft implementation (openraft) and target the same
management infrastructure nodes (3-5 nodes in the management VLAN).

In the boot sequence, pact comes first: pact-agent is the init system on compute
nodes, and pact-journal must be running before lattice-server starts. This means
pact's Raft quorum is established infrastructure by the time lattice needs consensus.

## Decision

Support **two deployment modes** for pact-journal's Raft quorum. In both modes,
pact and lattice maintain **independent Raft groups** with separate state machines,
separate log compaction, and separate snapshots. The groups never share consensus —
only infrastructure.

### Mode 1: Standalone (default)

Pact-journal runs its own Raft cluster on dedicated nodes.

```
pact-journal-1 ─┐
pact-journal-2 ──┤ pact Raft group
pact-journal-3 ─┘

lattice-server-1 ─┐
lattice-server-2 ──┤ lattice Raft group
lattice-server-3 ─┘
```

- 6-10 quorum nodes total (3-5 per system)
- Fully independent failure domains
- Recommended for: large sites (>5k nodes), regulated environments, sites that
  require independent maintenance windows

### Mode 2: Co-located

Pact-journal and lattice-server run on the **same management nodes**, each with
its own Raft group. Pact-journal is the incumbent — it is already running when
lattice starts. Lattice discovers pact's quorum nodes and deploys alongside them.

```
mgmt-node-1: pact-journal (Raft group A, port 9444) + lattice-server (Raft group B, port 9000)
mgmt-node-2: pact-journal (Raft group A, port 9444) + lattice-server (Raft group B, port 9000)
mgmt-node-3: pact-journal (Raft group A, port 9444) + lattice-server (Raft group B, port 9000)
```

- 3-5 quorum nodes total (shared between both systems)
- Independent Raft groups on the same nodes (separate ports, state, logs)
- Pact quorum is primary infrastructure; lattice joins existing nodes
- Hardware failure takes out both systems on that node (acceptable: Raft tolerates
  minority failure, and both groups lose the same node simultaneously)
- Recommended for: most sites, operational simplicity

### How co-location works

**Pact side** (no changes needed):
- pact-journal starts normally on management nodes
- Exposes its quorum node addresses in its config and via a discovery endpoint
- Listens on its own Raft port (default: 9444) and gRPC port (default: 9443)

**Lattice side** (configuration option):
- Lattice config gains an optional `pact_journal_endpoints` field
- When set, lattice-server deploys its Raft group on the same nodes as pact-journal
- Lattice uses its own ports (default: Raft 9000, gRPC 50051, REST 8080)
- Lattice's quorum config (`peers`) points to the same hostnames as pact's journal
  endpoints, but with lattice's Raft port

Example lattice production config (co-located):
```yaml
quorum:
  node_id: 1
  raft_listen_address: "0.0.0.0:9000"
  peers:
    - id: 2
      address: "mgmt-02:9000"    # same host as pact-journal-2
    - id: 3
      address: "mgmt-03:9000"    # same host as pact-journal-3
```

There is no protocol-level integration. Co-location is purely an infrastructure
decision — two independent processes sharing the same physical/virtual nodes.

## What is NOT shared

- **Raft consensus**: each system has its own leader election, log, and state machine
- **State machine**: pact's `JournalState` and lattice's `GlobalState` are independent
- **WAL/snapshots**: separate data directories (`/var/lib/pact/journal` vs `/var/lib/lattice/raft`)
- **Ports**: each system listens on its own ports
- **Failure recovery**: each group recovers independently (a pact leader failover
  does not trigger a lattice leader failover)

## Trade-offs

### Standalone
- (+) Independent failure domains — pact outage doesn't affect lattice and vice versa
- (+) Independent maintenance windows
- (+) Simpler mental model (no shared infrastructure)
- (-) More nodes to operate (6-10 vs 3-5)
- (-) More infrastructure cost

### Co-located
- (+) Fewer nodes (3-5 vs 6-10)
- (+) Single set of management nodes to monitor and maintain
- (+) Natural fit: both systems target the same management infrastructure
- (+) pact is already there (init system), lattice joins naturally
- (-) Shared hardware failure domain (mitigated by Raft's majority quorum)
- (-) Shared maintenance windows (reboot affects both)
- (-) Resource contention possible under heavy load (mitigated by low Raft I/O)

## Consequences

- pact-journal does not need to know about lattice at all — it just runs its Raft group
- Lattice's deployment guide documents co-located mode as an option
- Monitoring should track both Raft groups independently regardless of deployment mode
- The pact production config includes quorum node addresses that lattice can reference
- No code changes needed for co-location — it's a deployment/configuration decision

## Revisit

If a future requirement demands cross-system transactions (e.g., atomic
"commit config + drain node"), a shared Raft group with namespaced commands
could be considered. Current design does not require this.
