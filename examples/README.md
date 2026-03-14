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

## Configuration Examples

TOML configuration files for different deployment scenarios:

| File | Description |
|------|-------------|
| [agent-dev.toml](config/agent-dev.toml) | Minimal agent config for local development |
| [agent-production.toml](config/agent-production.toml) | Production agent with mTLS and eBPF |
| [journal-3node.toml](config/journal-3node.toml) | 3-node journal quorum config |

## Policy Examples

OPA/Rego policies for authorization:

| File | Description |
|------|-------------|
| [ml-training.rego](policy/ml-training.rego) | Policy rules for an ML training vCluster |

## Prerequisites

- A running pact journal (see `just run-journal` for development)
- A running pact agent (see `just run-agent` for development)
- The `pact` CLI binary on your PATH (see `just cli` to run from source)

For the full development setup, see [docs/usage/getting-started.md](../docs/usage/getting-started.md).
