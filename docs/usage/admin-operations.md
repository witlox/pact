# Admin Operations

This guide covers day-to-day operational workflows with pact. All operations
described here are authenticated via OIDC, authorized via OPA policy, and
recorded in the immutable journal.

## Roles

| Role | Access | Typical Users |
|------|--------|---------------|
| `pact-platform-admin` | Full system access | 2-3 people per site |
| `pact-ops-{vcluster}` | Day-to-day ops for a vCluster | Ops engineers |
| `pact-viewer-{vcluster}` | Read-only access | Monitoring teams, auditors |
| `pact-regulated-{vcluster}` | Ops with two-person approval | Sensitive workload admins |

## Day-to-Day Operations

### Check node status

```bash
# Overview of all nodes in your vCluster
pact status --vcluster ml-training

# Detailed status for a specific node
pact status node-042
```

### View drift

Drift is the difference between declared state (in the journal) and actual state
(on the node). pact uses blacklist-based detection -- everything is monitored
except explicitly excluded paths.

```bash
# See what has drifted on a node
pact diff node-042

# See committed deltas not yet promoted to the vCluster overlay
pact diff --committed node-042
```

### Commit drift

When drift is intentional (e.g., you tuned a sysctl), commit it to make it the
new declared state:

```bash
pact commit -m "tuned vm.nr_hugepages for training workload"
```

Commits happen within a time window (default 15 minutes). If the window expires,
drift is flagged for review rather than silently discarded.

### Roll back

If a configuration change caused problems, roll back to a known-good state:

```bash
# Find the sequence number to roll back to
pact log -n 20

# Roll back
pact rollback 42
```

### Extend the commit window

If you need more time to finalize changes before committing:

```bash
pact extend          # +15 minutes (default)
pact extend 30       # +30 minutes
```

### Apply a configuration spec

For bulk or repeatable changes, write a TOML spec and apply it:

```bash
pact apply config/vcluster-examples/overlays.toml
```

This updates the vCluster overlay in the journal. All nodes in the vCluster
will converge to the new declared state.

## Emergency Mode

Emergency mode is for situations where normal policy constraints would prevent
necessary diagnostic or repair actions. It relaxes whitelist restrictions and
extends the commit window, while maintaining the full audit trail.

### When to use emergency mode

- Node is degraded and you need unrestricted diagnostic access
- A service is failing and you need to inspect or modify files outside the whitelist
- You need to make urgent changes that would normally require approval

### Entering emergency mode

```bash
pact emergency start -r "GPU ECC errors on node-042, need unrestricted diagnostics"
```

This:
1. Records the emergency entry in the journal with your identity and reason
2. Extends the commit window to 4 hours (configurable)
3. Relaxes command whitelist restrictions on the node
4. Sends a notification via Loki/Grafana alerting

### Working in emergency mode

All commands are still logged. Emergency mode does not bypass authentication or
audit -- it only relaxes operational constraints.

```bash
pact shell node-042
pact:node-042> nvidia-smi -q -d ECC
pact:node-042> dmesg | grep -i error
pact:node-042> cat /var/log/pact-agent.log
pact:node-042> exit
```

### Exiting emergency mode

```bash
pact emergency end
```

If another admin left an emergency session open, a platform admin can force-end it:

```bash
pact emergency end --force
```

### Audit implications

Emergency mode entries are flagged in the journal and appear prominently in audit
reports. For regulated vClusters (7-year retention), emergency entries include:
- Who entered emergency mode and when
- The stated reason
- Every command executed during the session
- Who ended emergency mode and when

## Two-Person Approval Workflow

Regulated vClusters (those with `two_person_approval = true`) require a second
admin to approve state-changing operations before they take effect.

### Submitting a change

```bash
# Admin A commits a change on a regulated vCluster
pact commit -m "add audit-forwarder service to sensitive-compute"
```

Output:
```
Approval required (two-person policy on vcluster: sensitive-compute)
Pending approval: ap-7f3a (expires in 30 min)
Waiting for approval... (Ctrl-C to background)
```

### Reviewing and approving

```bash
# Admin B lists pending approvals
pact approve list

# Review the change details, then approve
pact approve accept ap-7f3a
```

### Denying a change

```bash
pact approve deny ap-7f3a -m "not scheduled in the change window"
```

### Rules

- You cannot approve your own request
- Approvals expire after a configurable timeout (default 30 minutes)
- Expired requests are automatically rolled back
- Both the request and the approval/denial are recorded in the journal

## Service Management

pact-agent supervises services on compute nodes. You can check status, restart
services, and view logs remotely.

### Check service status

```bash
pact service status                  # All services on local node
pact service status chronyd          # Specific service
```

### Restart a service

```bash
pact service restart nvidia-persistenced
```

Service restarts are subject to the commit window. If the window has expired,
extend it first:

```bash
pact extend
pact service restart nvidia-persistenced
```

### View service logs

```bash
pact service logs lattice-node-agent
```

Streams the last 50 lines. For continuous streaming, use `pact watch`.

## Remote Command Execution

pact replaces SSH for all admin access to compute nodes. Commands are executed
via the agent's gRPC exec endpoint.

### Single command

```bash
pact exec node-042 -- nvidia-smi
pact exec node-042 -- cat /proc/meminfo
pact exec node-042 -- dmesg -T
```

Commands must be on the agent's whitelist. The whitelist mode is configured
per-agent:

| Mode | Behavior |
|------|----------|
| `strict` | Only explicitly whitelisted commands are allowed |
| `learning` | All commands are allowed but non-whitelisted ones are logged for review |
| `bypass` | All commands allowed (development only) |

### Interactive shell

```bash
pact shell node-042
```

The shell provides a restricted environment on the node. Same whitelist rules apply.

## Using the MCP Server

pact includes an MCP (Model Context Protocol) server for AI-assisted operations.
The MCP server exposes 24 tools that mirror the CLI commands.

### Starting the MCP server

```bash
PACT_ENDPOINT="http://localhost:9443" pact-mcp
```

The server communicates via JSON-RPC 2.0 over stdio. Connect it to Claude Code
or any MCP-compatible AI agent.

### Available tools

| Tool | Category | Description |
|------|----------|-------------|
| `pact_status` | Read | Query node/vCluster state |
| `pact_diff` | Read | Show declared vs actual differences |
| `pact_log` | Read | Query configuration history |
| `pact_cap` | Read | Show hardware capabilities |
| `pact_service_status` | Read | Query service status |
| `pact_query_fleet` | Read | Fleet-wide queries |
| `pact_commit` | Write | Commit drift |
| `pact_rollback` | Write | Roll back configuration |
| `pact_apply` | Write | Apply a config spec |
| `pact_exec` | Write | Run a remote command |
| `pact_emergency` | Admin | Emergency mode (restricted to human admins) |
| `pact_jobs_list` | Lattice | List running allocations |
| `pact_queue_status` | Lattice | Scheduling queue depth |
| `pact_cluster_health` | Lattice | Combined Raft cluster status |
| `pact_system_health` | Lattice | Combined system health check |
| `pact_accounting` | Lattice | Resource usage accounting |
| `pact_undrain` | Lattice | Cancel drain on a node |
| `pact_dag_list` | Lattice | List DAG workflows |
| `pact_dag_inspect` | Lattice | DAG details and step status |
| `pact_budget` | Lattice | Tenant or user budget/usage |
| `pact_backup_create` | Admin | Create lattice state backup |
| `pact_backup_verify` | Lattice | Verify backup integrity |
| `pact_nodes_list` | Lattice | List nodes with state |
| `pact_node_inspect` | Lattice | Node hardware/ownership details |

The MCP server connects to the journal (config operations), agent (exec/shell),
and lattice (delegation). If any backend is unreachable, it falls back to stub
responses. Destructive operations (`dag cancel`, `backup restore`) are excluded
from MCP — use the CLI for those.

### Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PACT_ENDPOINT` | Journal gRPC endpoint | `http://localhost:9443` |
| `PACT_AGENT_ENDPOINT` | Agent gRPC endpoint | `http://localhost:9445` |
