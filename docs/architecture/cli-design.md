# CLI & Shell Design

## Philosophy

pact CLI is both the remote admin interface (from workstations) and the local shell
(on nodes, replacing bash for admin operations). Every operation is authenticated,
authorized, and logged.

## Command Reference

### Configuration Management

| Command | Description |
|---------|-------------|
| `pact status [--vcluster X]` | Node/vCluster state, drift, capabilities |
| `pact diff [node]` | Declared vs actual state |
| `pact diff --committed [node]` | Show committed node deltas not yet in overlay |
| `pact commit -m "msg"` | Commit drift (prompted: local/cluster-wide) |
| `pact rollback [seq]` | Roll back to previous state |
| `pact log [-n N] [--scope S]` | Configuration history |
| `pact apply <spec.toml>` | Apply declarative config spec |
| `pact promote <node> [--dry-run]` | Export committed node deltas as overlay TOML |
| `pact watch [--vcluster X]` | Live event stream |
| `pact extend [mins]` | Extend commit window |
| `pact emergency` | Enter/exit emergency mode |

### Admin Operations (replaces SSH)

| Command | Description |
|---------|-------------|
| `pact exec <node> -- <cmd>` | Run command on node (whitelisted) |
| `pact shell <node>` | Interactive shell session |
| `pact service status <name>` | Service status |
| `pact service restart <name>` | Restart service (commit window applies) |
| `pact service logs <name>` | Stream service logs |
| `pact cap [node]` | Capability report |
| `pact blacklist` | Manage drift detection exclusions |

### Delegation (calls external APIs)

| Command | Delegates to | Description |
|---------|-------------|-------------|
| `pact reboot <node>` | OpenCHAMI/Manta | Reboot via Redfish BMC |
| `pact reimage <node>` | OpenCHAMI/Manta | Re-image node |
| `pact drain <node>` | Lattice | Drain jobs from node |
| `pact cordon <node>` | Lattice | Remove from scheduling |
| `pact uncordon <node>` | Lattice | Return to scheduling |

### Group Management

| Command | Description |
|---------|-------------|
| `pact group list` | List vClusters and groups |
| `pact group show <name>` | Show vCluster config |
| `pact group set-policy` | Update vCluster policy |

## Example: Debug Session

```bash
# Admin notices GPU issues on node042
$ pact cap node042
  GPUs: 3x A100 (healthy), 1x A100 (DEGRADED - ECC errors)

# Check what's different from declared state
$ pact diff node042
  gpu[3]: declared=healthy actual=DEGRADED (ECC uncorrectable: 12)

# Run diagnostics remotely
$ pact exec node042 -- nvidia-smi -q -d ECC
  [full nvidia-smi output streamed back, logged to journal]

# Need interactive access
$ pact shell node042
pact:node042> dmesg | grep -i nvidia | tail -5
  [kernel messages about GPU errors]
pact:node042> cat /var/log/nvidia-persistenced.log
  [nvidia daemon logs]
pact:node042> exit

# Cordon the node while hardware team investigates
$ pact cordon node042
  Cordoned: node042 removed from lattice scheduling (via lattice API)
```

## Example: Promoting Node Deltas to Overlay

```bash
# After debugging, admin added a sysctl and NFS mount on node042.
# These were committed as node deltas. Check what's accumulated:
$ pact diff --committed node042
  kernel: vm.nr_hugepages = 1024  (committed seq:4812, 3 days ago)
  mounts: /local-scratch type=nfs source=storage03:/scratch  (committed seq:4815, 2 days ago)

# Export as overlay TOML (dry-run to preview)
$ pact promote node042 --dry-run
  # Generated overlay fragment for vcluster: ml-training
  # From 2 committed node deltas on node042

  [vcluster.ml-training.sysctl]
  "vm.nr_hugepages" = "1024"

  [vcluster.ml-training.mounts]
  "/local-scratch" = { type = "nfs", source = "storage03:/scratch" }

# Looks right — export to file, review, then apply to the whole vCluster
$ pact promote node042 > /tmp/hugepages-and-scratch.toml
$ vi /tmp/hugepages-and-scratch.toml   # review/edit
$ pact apply /tmp/hugepages-and-scratch.toml
  Applied to vcluster ml-training (2 changes). Overlay updated.
  Node deltas on node042 superseded (seq:4812, seq:4815 now redundant).

# Verify: node042 should have no more unpromoted deltas
$ pact diff --committed node042
  (no committed node deltas)
```

The `promote` command maps `StateDelta` fields to overlay TOML sections:

| StateDelta field | Overlay TOML section |
|-----------------|---------------------|
| `kernel` | `[vcluster.<name>.sysctl]` |
| `mounts` | `[vcluster.<name>.mounts]` |
| `files` | `[vcluster.<name>.files]` |
| `services` | `[vcluster.<name>.services.<svc>]` |
| `network` | `[vcluster.<name>.network]` |
| `packages` | `[vcluster.<name>.packages]` |

Deltas that can't be cleanly mapped (e.g. GPU state changes) are emitted as
comments with the raw delta for manual handling.

## Local Pact Shell (on-node)

When accessed via BMC console, the node drops into a pact shell (not bash).
Same authentication (local agent cert), same whitelist, same logging.

```
[BMC console connects]
pact:node042 (local)> pact status
  Node: node042  State: COMMITTED  Supervisor: 5 services running
pact:node042 (local)> pact service status lattice-node-agent
  lattice-node-agent: active (running, PID 4521, uptime 3h22m)
pact:node042 (local)> nvidia-smi
  [executes, logged locally, synced to journal when connectivity restored]
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Authentication/authorization failure |
| 3 | Policy rejection |
| 4 | Conflict (concurrent modification) |
| 5 | Timeout (journal unreachable) |
| 6 | Command not whitelisted |
| 10 | Rollback failed (active consumers) |
