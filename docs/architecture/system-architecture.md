# System Architecture

See [../../ARCHITECTURE.md](../../ARCHITECTURE.md) for the high-level overview.
This document covers detailed design and data flows.

## Design Requirements

- R1: Eventual consistency with acknowledged drift
- R2: Immutable configuration log
- R3: Optimistic concurrency with commit windows
- R4: Admin-native CLI + pact shell (replacing SSH)
- R5: Streaming boot configuration (<2s for 10k nodes)
- R6: Degradation-aware (partial HW failure → revised promises)
- R7: vCluster-aware grouping
- R8: IAM and policy enforcement (OIDC/RBAC/audit)
- R9: Blacklist-based drift detection with learning mode
- R10: Emergency mode (extended window + no rollback + full audit)
- R11: Observe-first deployment
- R12: Agentic API (MCP tool-use)
- R13: Process supervision (pact as init, systemd fallback)
- R14: No SSH (pact shell + pact exec)

## Raft Deployment

pact-journal runs its own Raft group, independent from lattice's quorum.
Two deployment modes (see [ADR-001](../decisions/ADR-001-separate-raft-quorum.md)):

- **Standalone**: pact-journal on dedicated management nodes (3-5 nodes)
- **Co-located**: pact-journal and lattice-server on the same management nodes,
  each with its own Raft group on separate ports

Pact is the incumbent in co-located mode — its quorum is already running when
lattice starts. Lattice configures its peers to point to the same hostnames.
No protocol-level coupling; co-location is a deployment decision.

## Consistency Model

AP in CAP terms. Nodes use cached config and cached policy during partitions.
Conflict resolution by timestamp ordering with admin-committed > auto-converge.
A node that can't reach the config server keeps running its workload.

During partitions, pact-agent falls back to cached `VClusterPolicy` for
authorization (role bindings and whitelists only — complex OPA rules and
two-person approval require connectivity). Degraded-mode decisions are logged
locally and replayed to the journal when connectivity is restored.

## Commit Window Formula

```
window_seconds = base_window / (1 + drift_magnitude * sensitivity)
```

| Drift | Example | Window | Rationale |
|-------|---------|--------|-----------|
| Tiny (0.05) | Single sysctl | ~13 min | Low risk |
| Small (0.15) | Config file edit | ~10 min | Routine |
| Moderate (0.3) | Mount + service | ~6 min | Needs attention |
| Large (0.8) | Multiple categories | ~3 min | Significant deviation |

Emergency mode: `pact emergency --reason "..."` extends to 4h, suspends rollback.

## Data Flows

### Boot-Time (10,000 nodes)

```
PXE → SquashFS → pact-agent (PID 1)
  → mTLS auth → Phase 1 stream (vCluster overlay, ~200KB, any replica)
  → apply config → Phase 2 (node delta, <1KB)
  → start services → CapabilityReport → ready
```

### Admin Change

```
pact exec / pact shell → command executed on node
  → state observer detects change → drift evaluator
  → commit window opens (proportional to drift)
  → admin commits (local/cluster-wide) or window expires (rollback)
  → journal records everything
```

### Commit Lifecycle and Reboot Persistence

Manual changes (via exec/shell) that are committed become **node-level state
deltas** in the journal. The journal maintains two layers of declared state:

```
vCluster overlay (shared)     e.g. "all ml-training nodes mount /scratch"
  + node deltas (per-node)    e.g. "node042 has extra sysctl from debugging"
  = effective declared state  (what the agent applies at boot)
```

On reboot, the agent streams both layers from the journal. Committed node
deltas are reapplied automatically — manual changes survive reboots as long
as they remain in the journal's node state.

**However, accumulating ad-hoc node deltas is not desirable long-term.** They
represent drift that was accepted rather than codified. Over time, nodes with
many committed deltas diverge from their vCluster peers, making fleet-wide
reasoning harder.

The intended lifecycle for manual changes:

| Stage | State | Action |
|-------|-------|--------|
| Detected | Drift | Observer flags divergence from declared state |
| Committed | Node delta | Admin commits change, recorded in journal |
| Promoted | vCluster overlay | `pact apply` updates the overlay to include the change |
| Expired | Cleaned up | `pact rollback` or superseded by overlay update |

**Promotion path**: when a committed manual change proves correct, the admin
should codify it via `pact apply <spec.toml>` at the vCluster level. This
updates the shared overlay and makes the node-level delta redundant.
`pact diff --committed` shows accumulated node deltas that haven't been
promoted.

**Expiry**: node deltas with a `ttl` field expire automatically. Emergency-mode
changes default to a TTL matching the emergency window. Changes without TTL
persist until explicitly rolled back or superseded.

### Hardware Degradation

```
GPU soft-fails → agent detects (NVML for NVIDIA, ROCm SMI for AMD, or eBPF)
  → CapabilityReport updated → scheduler adjusts eligibility
  → DriftDetected in journal → admin ack if policy requires
```

## Integration Delegation

| Action | Owner | pact does |
|--------|-------|-----------|
| Reboot node | OpenCHAMI | `pact reboot` calls Manta/Redfish API |
| Re-image node | OpenCHAMI | `pact reimage` calls Manta API |
| Firmware update | OpenCHAMI | `pact firmware` calls Magellan API |
| Drain node | Lattice | `pact drain` calls lattice scheduler API |
| Cordon node | Lattice | `pact cordon` calls lattice scheduler API |
| Job status | Lattice | `pact jobs` calls lattice API |
| Config management | pact (native) | Direct implementation |
| Remote access | pact (native) | Shell server, exec endpoint |
| Service lifecycle | pact (native) | PactSupervisor or SystemdBackend |
