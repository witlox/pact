# CLAUDE.md — pact project instructions

## What is pact?

pact is the configuration management and admin operations layer for HPC/AI
infrastructure. It replaces both traditional config management tools (Ansible/Puppet/
Salt) AND SSH access to compute nodes with a single authenticated, audited, policy-
enforced system.

On diskless compute nodes, pact-agent is the effective init system — it is the first
management process to start after boot, it supervises all other services, and it
provides the only interactive access to the node (replacing SSH).

pact sits in the lattice ecosystem:
- **Below pact**: OpenCHAMI (hardware discovery, boot provisioning, DHCP, BMC)
- **Beside pact**: lattice (workload scheduling, job management)
- **Above pact**: Sovra (federated key management, cross-org trust)

Boot chain: OpenCHAMI provisions base image with pact-agent → pact-agent starts as
init → pulls config from journal → applies kernel params, mounts, uenv → starts
declared services (including lattice-node-agent) → node ready.

## Two core functions

1. **Configuration management**: declare desired state for vClusters and nodes,
   detect drift, commit/rollback with time windows, immutable audit log.

2. **Admin operations**: authenticated remote command execution (replacing SSH),
   diagnostics, service management, interactive debugging via pact shell.
   All operations are logged, policy-checked, and scoped by OIDC role.

## Architecture overview

Five components:
1. **pact-agent** (every compute node) — process supervisor, state observer, drift
   detection, commit windows, capability reporting, pact shell server, exec endpoint
2. **pact-journal** (Raft quorum, 3-5 nodes) — immutable config log, streaming boot
   config delivery, telemetry source
3. **pact-policy** (library crate, linked into pact-journal) — IAM/OIDC, RBAC,
   OPA policy evaluation, Sovra federation sync
4. **pact CLI** (admin workstation) — remote: commit, rollback, diff, status, exec,
   shell. local (on-node): same commands in pact shell mode
5. **pact MCP server** (optional) — AI agent tool-use interface

See `docs/architecture/` for detailed design documents.

## Key design principles

- **No SSH** — pact is the only admin interface to compute nodes.
  BMC/Redfish console is the out-of-band fallback for pact-agent failures.
- **pact-agent as init** — on diskless nodes, pact-agent supervises all services.
  systemd backend available as fallback for conservative deployments.
- **Eventual consistency with acknowledged drift** — never silently converge
- **Immutable log** — every config and admin action is recorded
- **Optimistic concurrency** — changes apply immediately, commit within time window
- **Admin-native** — pact shell feels like being on the box
- **OIDC-scoped roles** — every operation authenticated and authorized per vCluster

## Process supervision model

pact-agent includes a built-in process supervisor (default) with systemd as fallback:

```rust
// ServiceManager trait — two implementations
trait ServiceManager {
    async fn start(&self, service: &ServiceDecl) -> Result<()>;
    async fn stop(&self, service: &ServiceDecl) -> Result<()>;
    async fn restart(&self, service: &ServiceDecl) -> Result<()>;
    async fn status(&self, service: &ServiceDecl) -> Result<ServiceStatus>;
    async fn health(&self, service: &ServiceDecl) -> Result<HealthCheck>;
}

// Default: PactSupervisor — direct process management with cgroup v2
// Fallback: SystemdBackend — delegates to systemd via D-Bus
```

Configured per node or vCluster:
```toml
[agent.supervisor]
backend = "pact"  # "pact" (default) | "systemd" (fallback)
```

## Boot sequence (pact as init)

```
T+0.0s  Kernel + initramfs → mount SquashFS root
T+0.1s  pact-agent starts (PID 1 or early init)
T+0.2s  Agent authenticates to journal (mTLS)
T+0.3s  Phase 1: vCluster base overlay streamed (~100-200 KB)
T+0.5s  Agent applies: kernel params, modules, mounts, uenv
T+0.6s  Phase 2: node-specific delta (<1 KB)
T+0.7s  Agent starts declared services in dependency order:
          1. chronyd/PTP (time sync)
          2. nvidia-persistenced (if GPU node)
          3. metrics-exporter (if declared)
          4. lattice-node-agent (workload management)
          5. audit-forwarder (if regulated)
T+1.5s  CapabilityReport sent to lattice scheduler
T+1.6s  Steady state: observer active, shell server listening
```

Typical services per vCluster type:
- **ML training**: pact-agent, chronyd, nvidia-persistenced, lattice-node-agent (4)
- **Regulated sensitive**: + audit-forwarder, encryption-agent (6)
- **Dev sandbox**: pact-agent, lattice-node-agent (2)

## OIDC role model

```
pact-platform-admin       Full system access. Journal mgmt, global policies.
                          2-3 people per site.

pact-ops-{vcluster}       Day-to-day ops for a specific vCluster.
                          Exec, commit, emergency, service management.

pact-viewer-{vcluster}    Read-only. Status, diff, log, watch.
                          Monitoring teams, auditors, on-call.

pact-regulated-{vcluster} Like ops but requires two-person approval
                          for state-changing operations.

pact-service-agent        Machine identity for pact-agents (mTLS).
pact-service-ai           Machine identity for AI agents (MCP).
```

## Technology stack

- **Language**: Rust (consistent with lattice)
- **Raft**: raft-hpc-core (wraps openraft with HPC state machine abstractions)
- **Transport**: gRPC via tonic + protobuf via prost
- **eBPF**: aya crate
- **Linux**: nix crate (netlink, inotify, cgroups, mount, fork/exec)
- **Policy**: OPA/Rego co-located on journal/policy nodes (see ADR-003)
- **CLI**: clap
- **Config format**: TOML
- **Telemetry**: Prometheus (server-side), Loki (event streaming), Grafana

## Repository structure

```
pact/
├── CLAUDE.md                  # This file
├── README.md
├── ARCHITECTURE.md
├── Cargo.toml                 # Workspace manifest
├── rust-toolchain.toml
├── clippy.toml / rustfmt.toml / deny.toml
├── justfile
│
├── crates/
│   ├── pact-agent/            # Per-node daemon (init + config + shell)
│   │   └── src/
│   │       ├── supervisor/    # Process supervisor (PactSupervisor + SystemdBackend)
│   │       ├── shell/         # Pact shell server (replacing SSH)
│   │       ├── observer/      # eBPF, inotify, netlink state observers
│   │       ├── drift/         # Drift evaluator
│   │       ├── commit/        # Commit window manager
│   │       ├── capability/    # Hardware capability reporter
│   │       └── emergency/     # Emergency mode
│   │
│   ├── pact-journal/          # Distributed immutable log
│   │   └── src/
│   │       ├── raft/          # Raft state machine
│   │       ├── log/           # Append-only log
│   │       └── stream/        # Boot config streaming
│   │
│   ├── pact-policy/           # IAM and policy engine
│   │   └── src/
│   │       ├── iam/           # OIDC/SAML
│   │       ├── rbac/          # Role-based access control
│   │       ├── rules/         # Policy evaluation
│   │       └── federation/    # Sovra policy sync
│   │
│   ├── pact-cli/              # CLI binary
│   │   └── src/commands/
│   │
│   └── pact-common/           # Shared types, protobuf bindings
│
├── proto/pact/                # Protobuf definitions
├── config/                    # Example configs
├── docs/                      # Architecture, ADRs, operations
├── infra/                     # Docker, Grafana, alerting
├── scripts/
└── tests/e2e/
```

## Build commands

```bash
cargo build --workspace       # build
just test                     # fast tests
just test-all                 # full suite
just ci                       # all checks (fmt + clippy + deny + test)
```

## Development conventions

- All public types derive `Debug, Clone, Serialize, Deserialize` where possible
- Protobuf is the wire format; TOML is the config format
- Error types use `thiserror`; typed enums, not strings
- Async runtime: tokio (multi-threaded)
- Process supervision: `tokio::process::Command` + cgroup v2 filesystem API
- Tests: unit in-module, integration in `tests/`, BDD in `tests/e2e/`

## Integration points

- **Lattice scheduler**: pact-agent → CapabilityReport gRPC stream
- **Lattice node-agent**: pact starts it as a supervised service; reads capability
  manifest from tmpfs + unix socket
- **OpenCHAMI**: provisions base image with pact-agent + mTLS cert.
  pact delegates: reboot, re-image, firmware → OpenCHAMI/Manta APIs
- **Lattice**: pact delegates: drain, cordon, job management → lattice APIs
- **Sovra**: policy templates federated, config state site-local
- **Grafana/Loki**: journal streams events to Loki, Prometheus scrapes journal servers
- **AI agents**: MCP server for Claude Code-style tool-use

## Design decisions

See `docs/decisions/` for full ADRs:
- ADR-001: Raft quorum deployment modes (standalone or co-located with lattice)
- ADR-002: Blacklist-first drift detection with observe-only bootstrap
- ADR-003: Policy engine — OPA/Rego on journal nodes (accepted)
- ADR-004: Emergency mode preserves audit trail
- ADR-005: No agent-level Prometheus metrics
- ADR-006: Pact as init with systemd fallback
- ADR-007: No SSH — pact shell replaces remote access
- ADR-008: Node enrollment, domain membership, certificate lifecycle
- ADR-009: Overlay staleness detection and on-demand rebuild
- ADR-010: Per-node delta TTL bounds (15 min – 10 days)
- ADR-011: Degraded-mode policy evaluation (cached whitelist, fail-closed)
- ADR-012: Merge conflict grace period with journal-wins fallback
- ADR-013: Two-person approval as stateful Raft entries
- ADR-014: Optimistic concurrency with commit windows
- ADR-015: hpc-core shared contracts (hpc-node, hpc-audit, hpc-identity)
- ADR-016: Identity mapping — OIDC-to-POSIX UID/GID shim for NFS
