# pact Examples

Practical examples for getting started with pact.

## CLI Examples

Shell scripts demonstrating common workflows:

| Script | Description |
|--------|-------------|
| [01-basic-status.sh](cli/01-basic-status.sh) | Check node status, view logs, show drift |
| [02-commit-rollback.sh](cli/02-commit-rollback.sh) | Commit drift and roll back changes |
| [03-emergency-mode.sh](cli/03-emergency-mode.sh) | Enter/exit emergency mode for diagnostics |
| [04-approval-workflow.sh](cli/04-approval-workflow.sh) | Two-person approval on regulated vClusters |
| [05-apply-spec.sh](cli/05-apply-spec.sh) | Apply a declarative TOML spec |
| [06-diagnostics.sh](cli/06-diagnostics.sh) | Retrieve diagnostic logs (per-node and fleet-wide) |
| [07-node-enrollment.sh](cli/07-node-enrollment.sh) | Register, assign, and manage compute nodes |
| [08-service-management.sh](cli/08-service-management.sh) | Start, stop, restart supervised services |
| [09-supercharged-commands.sh](cli/09-supercharged-commands.sh) | Unified pact + lattice admin commands |
| [10-promote-and-blacklist.sh](cli/10-promote-and-blacklist.sh) | Promote node deltas and manage drift blacklist |

## Configuration Examples

TOML configuration files for different deployment scenarios:

| File | Description |
|------|-------------|
| [agent-dev.toml](config/agent-dev.toml) | Minimal agent config for local development |
| [agent-production.toml](config/agent-production.toml) | Production agent with mTLS and eBPF |
| [journal-3node.toml](config/journal-3node.toml) | 3-node journal quorum config |
| [vcluster-ml-training.toml](config/vcluster-ml-training.toml) | ML training vCluster overlay (sysctl, mounts, services) |
| [vcluster-regulated.toml](config/vcluster-regulated.toml) | Regulated vCluster with audit services and two-person approval |

## Policy Examples

OPA/Rego policies for authorization:

| File | Description |
|------|-------------|
| [ml-training.rego](policy/ml-training.rego) | Policy rules for an ML training vCluster |
| [regulated-bio.rego](policy/regulated-bio.rego) | Strict policy with two-person approval and limited exec whitelist |

## Prerequisites

- A running pact journal (see `just run-journal` for development)
- A running pact agent (see `just run-agent` for development)
- The `pact` CLI binary on your PATH (see `just cli` to run from source)

For the full development setup, see [docs/usage/getting-started.md](../docs/usage/getting-started.md).
