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
| `pact commit -m "msg"` | Commit drift on current node (node-level delta) |
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

### Supercharged (pact + lattice)

These commands combine data from pact and lattice into unified views.
Requires `PACT_LATTICE_ENDPOINT` to be configured.

| Command | Description |
|---------|-------------|
| `pact jobs list [--node X]` | List running allocations |
| `pact jobs cancel <id>` | Cancel a stuck job |
| `pact jobs inspect <id>` | Job details |
| `pact queue [--vcluster X]` | Scheduling queue status |
| `pact cluster` | Combined Raft cluster health |
| `pact audit [--source all]` | Unified audit trail (pact + lattice) |
| `pact accounting [--vcluster X]` | Resource usage (GPU/CPU hours) |
| `pact health` | Combined system health check |

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

## Two-Person Approval (Regulated vClusters)

For vClusters with `two_person_approval = true`, state-changing operations
require a second admin to approve before execution.

### CLI Commands

| Command | Description |
|---------|-------------|
| `pact approve list` | Show pending approval requests |
| `pact approve <id>` | Approve a pending request |
| `pact approve deny <id> -m "reason"` | Deny a pending request |

### Flow

```bash
# Admin A: commit a change on a regulated vCluster
$ pact commit -m "add hugepages for training"
  Approval required (two-person policy on vcluster: sensitive-compute)
  Pending approval: ap-7f3a (expires in 30 min)
  Waiting for approval... (Ctrl-C to background)

# Admin B (separately): sees pending approvals
$ pact approve list
  ap-7f3a  sensitive-compute  "add hugepages for training"  by admin-a@org  12 min ago

$ pact approve ap-7f3a
  Approved. Commit applied on sensitive-compute.
```

### Mechanism

1. Admin A's operation triggers `PolicyService.Evaluate()` → OPA returns
   `ApprovalRequired { approval_type: "two_person", pending_approval_id: "ap-7f3a" }`
2. The request is stored in the journal as a pending operation (new entry type)
3. Admin B queries pending approvals via journal, approves via `PolicyService`
4. The journal stores the approval and executes the original operation
5. If no approval within the timeout (default 30 min, configurable per vCluster),
   the request expires and the change is rolled back

### Notifications

Pending approvals are emitted as Loki events with structured labels. Grafana
alert rules can route these to Slack, PagerDuty, or email based on vCluster
and severity.

## BMC Console Access (on-node)

BMC console provides regular bash — not restricted bash, not pact shell.
This is the out-of-band fallback for when pact-agent is unresponsive or when
the admin needs unrestricted access (e.g. to debug pact-agent itself).

BMC access is controlled by BMC credentials (IPMI/Redfish), not by pact RBAC.
Changes made via BMC are detected by the drift observer when pact-agent is
running, and appear as unattributed drift (no OIDC identity).

```
[BMC console connects — regular bash]
root@node042:~# pact status
  Node: node042  State: COMMITTED  Supervisor: 5 services running
root@node042:~# systemctl status pact-agent
  [check agent health]
root@node042:~# nvidia-smi
  [unrestricted access, drift detected if state changes]
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
