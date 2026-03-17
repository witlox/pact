# CLI Reference

pact CLI is the primary interface for configuration management and admin operations.
Every command is authenticated, authorized, and logged to the immutable journal.

## Global Options

```
pact [OPTIONS] <COMMAND>
```

| Option | Description |
|--------|-------------|
| `--endpoint <URL>` | Journal gRPC endpoint (overrides `PACT_ENDPOINT` and config file) |
| `--token <TOKEN>` | OIDC bearer token (overrides `PACT_TOKEN` and config file) |
| `--vcluster <NAME>` | Default vCluster scope (overrides `PACT_VCLUSTER` and config file) |
| `--output <FORMAT>` | Output format: `text` (default) or `json` |

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PACT_ENDPOINT` | Journal gRPC endpoint | `http://localhost:9443` |
| `PACT_TOKEN` | OIDC bearer token | (none, reads from `~/.config/pact/token`) |
| `PACT_VCLUSTER` | Default vCluster scope | (none) |
| `PACT_OUTPUT` | Output format (`text` or `json`) | `text` |
| `RUST_LOG` | Log level for debug output | `warn` |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error (connection failure, invalid arguments) |
| 2 | Authentication or authorization failure |
| 3 | Policy rejection (OPA denied the operation) |
| 4 | Conflict (concurrent modification detected) |
| 5 | Timeout (journal unreachable) |
| 6 | Command not whitelisted (exec/shell) |
| 10 | Rollback failed (active consumers hold the state) |

---

## Read Commands

These commands query state without modifying anything. Available to all roles
including `pact-viewer-{vcluster}`.

### `pact status`

Show node or vCluster state, drift, and capabilities.

```bash
pact status                          # All nodes in default vCluster
pact status node-042                 # Specific node
pact status --vcluster ml-training   # All nodes in a vCluster
```

| Option | Description |
|--------|-------------|
| `[node]` | Node ID to query (optional, defaults to all nodes) |
| `--vcluster <NAME>` | vCluster scope |

### `pact log`

Show configuration history from the immutable journal.

```bash
pact log                             # Last 20 entries
pact log -n 50                       # Last 50 entries
pact log --scope node:node-042       # Filter by node
pact log --scope vc:ml-training      # Filter by vCluster
pact log --scope global              # Global entries only
```

| Option | Description |
|--------|-------------|
| `-n <COUNT>` | Number of entries to show (default: 20) |
| `--scope <FILTER>` | Scope filter: `node:<id>`, `vc:<name>`, or `global` |

### `pact diff`

Show declared vs actual state differences (drift).

```bash
pact diff                            # Current node
pact diff node-042                   # Specific node
pact diff --committed node-042       # Show committed node deltas not yet promoted
```

| Option | Description |
|--------|-------------|
| `[node]` | Node ID to diff (optional) |
| `--committed` | Show committed node deltas not yet promoted to overlay |

### `pact cap`

Show node hardware capability report (CPU, GPU, memory, network).

```bash
pact cap                             # Local node
pact cap node-042                    # Remote node
```

| Option | Description |
|--------|-------------|
| `[node]` | Node ID (optional, defaults to local) |

### `pact watch`

Live event stream from the journal. Streams events in real time until interrupted.

```bash
pact watch                           # Default vCluster
pact watch --vcluster ml-training    # Specific vCluster
```

| Option | Description |
|--------|-------------|
| `--vcluster <NAME>` | vCluster scope |

Press `Ctrl-C` to stop the stream.

---

## Write Commands

These commands modify configuration state. Requires `pact-ops-{vcluster}` or
`pact-platform-admin` role. On regulated vClusters, write commands trigger the
two-person approval workflow.

### `pact commit`

Commit current drift on the node as a configuration entry in the journal.

```bash
pact commit -m "tuned hugepages for ML training"
pact commit -m "added NFS mount for datasets"
```

| Option | Description |
|--------|-------------|
| `-m <MESSAGE>` | Commit message (required) |

The commit is scoped to the current vCluster (from `--vcluster`, `PACT_VCLUSTER`,
or config file). On regulated vClusters, this triggers approval workflow.

### `pact rollback`

Roll back to a previous configuration state by sequence number.

```bash
pact rollback 42                     # Roll back to seq 42
```

| Option | Description |
|--------|-------------|
| `<seq>` | Target sequence number to roll back to (required) |

Use `pact log` to find the sequence number you want to roll back to.

### `pact apply`

Apply a declarative configuration spec from a TOML file.

```bash
pact apply overlay.toml              # Apply a spec file
pact apply /tmp/hugepages.toml       # Apply from absolute path
```

| Option | Description |
|--------|-------------|
| `<spec>` | Path to TOML spec file (required) |

The spec file format matches the vCluster overlay format. See
`config/vcluster-examples/overlays.toml` for the schema.

---

## Exec Commands

These commands execute operations on remote nodes. Requires `pact-ops-{vcluster}`
or `pact-platform-admin` role. All executions are logged to the journal.

### `pact exec`

Run a whitelisted command on a remote node. The command and its output are recorded
in the immutable audit log.

```bash
pact exec node-042 -- nvidia-smi
pact exec node-042 -- dmesg -T | tail -20
pact exec node-042 -- cat /proc/meminfo
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |
| `-- <command...>` | Command and arguments (after `--`, required) |

Commands must be on the agent's whitelist. Non-whitelisted commands return exit
code 6.

### `pact shell`

Open an interactive shell session on a remote node. This replaces SSH access.

```bash
pact shell node-042
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |

Inside the shell, commands are subject to the whitelist policy configured on the
agent (`whitelist_mode` in agent config). The session is fully logged.

```
pact:node-042> dmesg | tail -5
pact:node-042> cat /etc/hostname
pact:node-042> exit
```

### `pact service`

Manage services on a node.

#### `pact service status`

```bash
pact service status                  # All services
pact service status chronyd          # Specific service
```

#### `pact service restart`

```bash
pact service restart nvidia-persistenced
```

Restarts are subject to the commit window. If the commit window has expired,
you must commit or extend first.

#### `pact service logs`

```bash
pact service logs lattice-node-agent
```

Streams the last 50 log lines for the service.

---

## Admin Commands

These commands handle emergency operations and approval workflows.

### `pact emergency`

Enter or exit emergency mode. Emergency mode relaxes policy constraints while
maintaining the full audit trail. Use only for genuine emergencies.

#### `pact emergency start`

```bash
pact emergency start -r "GPU node unresponsive, need unrestricted diagnostics"
```

| Option | Description |
|--------|-------------|
| `-r <REASON>` | Reason for entering emergency mode (required) |

Emergency mode extends the commit window to 4 hours (configurable via
`emergency_window_seconds`) and relaxes whitelist restrictions.

#### `pact emergency end`

```bash
pact emergency end                   # End your own emergency
pact emergency end --force           # Force-end another admin's emergency
```

| Option | Description |
|--------|-------------|
| `--force` | Force-end another admin's emergency session |

### `pact approve`

Manage the two-person approval workflow for regulated vClusters.

#### `pact approve list`

```bash
pact approve list
```

Lists all pending approval requests across vClusters you have access to.

#### `pact approve accept`

```bash
pact approve accept ap-7f3a
```

| Option | Description |
|--------|-------------|
| `<id>` | Approval ID (required) |

You cannot approve your own request. The approver must have
`pact-regulated-{vcluster}` or `pact-platform-admin` role.

#### `pact approve deny`

```bash
pact approve deny ap-7f3a -m "change window not scheduled"
```

| Option | Description |
|--------|-------------|
| `<id>` | Approval ID (required) |
| `-m <MESSAGE>` | Denial reason (required) |

### `pact extend`

Extend the current commit window.

```bash
pact extend                          # Extend by 15 minutes (default)
pact extend 30                       # Extend by 30 minutes
```

| Option | Description |
|--------|-------------|
| `[mins]` | Additional minutes (default: 15) |

---

## Node Lifecycle Commands

These commands manage node state transitions. Requires `pact-ops-{vcluster}` or
`pact-platform-admin` role. All lifecycle operations are logged to the journal.

### `pact promote`

Promote a node to active service after enrollment and configuration.

```bash
pact promote node-042                # Promote node
pact promote node-042 --dry-run      # Show what would happen without applying
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |
| `--dry-run` | Show promotion plan without executing |

### `pact drain`

Drain workloads from a node. Delegates to lattice to gracefully migrate running
workloads before taking the node out of service.

```bash
pact drain node-042
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |

### `pact cordon`

Mark a node as unschedulable. Existing workloads continue running but no new
workloads will be placed on the node.

```bash
pact cordon node-042
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |

### `pact uncordon`

Remove a cordon from a node, making it schedulable again.

```bash
pact uncordon node-042
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |

### `pact reboot`

Reboot a node via BMC/Redfish. Delegates to OpenCHAMI for the actual reboot.

```bash
pact reboot node-042
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |

### `pact reimage`

Re-image a node via OpenCHAMI. The node will be re-provisioned with the base
SquashFS image and re-enrolled with pact.

```bash
pact reimage node-042
```

| Option | Description |
|--------|-------------|
| `<node>` | Target node ID (required) |

---

## Group Commands

Manage vCluster groups and their policies.

### `pact group list`

List all vCluster groups.

```bash
pact group list
pact group list --output json
```

### `pact group show`

Show details for a specific group.

```bash
pact group show ml-training
```

| Option | Description |
|--------|-------------|
| `<group>` | Group name (required) |

### `pact group set-policy`

Update the policy for a group.

```bash
pact group set-policy ml-training --file policy.toml
```

| Option | Description |
|--------|-------------|
| `<group>` | Group name (required) |
| `--file <PATH>` | Path to policy TOML file (required) |

---

## Blacklist Commands

Manage drift detection exclusion patterns.

### `pact blacklist list`

List current blacklist patterns for a node or vCluster.

```bash
pact blacklist list
pact blacklist list --vcluster ml-training
```

### `pact blacklist add`

Add a pattern to the drift detection blacklist.

```bash
pact blacklist add "/var/cache/**"
pact blacklist add "/opt/scratch/**" --vcluster ml-training
```

| Option | Description |
|--------|-------------|
| `<pattern>` | Glob pattern to exclude from drift detection (required) |
| `--vcluster <NAME>` | Apply to a specific vCluster (optional, defaults to node-local) |

### `pact blacklist remove`

Remove a pattern from the drift detection blacklist.

```bash
pact blacklist remove "/var/cache/**"
```

| Option | Description |
|--------|-------------|
| `<pattern>` | Glob pattern to remove (required) |

---

## Configuration File

The CLI reads its configuration from `~/.config/pact/cli.toml`:

```toml
endpoint = "https://journal.example.com:9443"
default_vcluster = "ml-training"
output_format = "text"
timeout_seconds = 30
token_path = "~/.config/pact/token"
```

All fields are optional and have sensible defaults. See the
[Getting Started](getting-started.md) guide for the full precedence chain.
