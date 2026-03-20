<p align="center">
  <img src="logo.png" alt="pact" width="360">
</p>

<p align="center">
  <a href="https://github.com/witlox/pact/actions/workflows/ci.yml"><img src="https://github.com/witlox/pact/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://codecov.io/gh/witlox/pact"><img src="https://codecov.io/gh/witlox/pact/branch/main/graph/badge.svg" alt="Coverage"></a>
  <a href="https://github.com/witlox/pact/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License"></a>
  <a href="https://github.com/witlox/pact"><img src="https://img.shields.io/badge/rust-stable-orange.svg" alt="Rust"></a>
</p>

---

Promise-based configuration management and admin operations for HPC/AI infrastructure. pact manages post-boot runtime configuration on large-scale diskless HPC/AI clusters and provides the sole admin operations interface to compute nodes — replacing both traditional config management tools and SSH.

On compute nodes, pact-agent is the init system: it supervises all services, manages configuration, and provides authenticated remote access via pact shell.

## Design Principles

- **No SSH needed** — pact shell provides authenticated, audited, policy-scoped remote access
- **pact-agent as init** — boots diskless nodes in <2s, supervises 5-9 services directly
- **Acknowledged drift** — detected, measured, explicitly handled — never silently converged
- **Immutable audit log** — every action recorded, any state reconstructible
- **Optimistic concurrency** — apply first, commit within time window, rollback on expiry
- **Network separation** — management net for pact control traffic, HSN for lattice workload data
- **10,000+ node scale** — streaming boot config, no per-node scrape targets

## Architecture

```
Admin Plane      pact CLI / pact shell / AI Agent (MCP)
Control Plane    pact-journal (Raft) + pact-policy (IAM/OPA) + Grafana/Loki
Node Plane       pact-agent (init + supervisor + observer + shell server)
Infrastructure   OpenCHAMI (boot) → pact (config + init) → lattice (scheduling)
```

Integrates with [Lattice](https://github.com/witlox/lattice),
[OpenCHAMI](https://openchami.org), and [Sovra](https://github.com/witlox/sovra).
Shares types and traits with lattice via the [hpc-core](https://github.com/witlox/hpc-core)
crates (hpc-node, hpc-audit, hpc-identity).

## Installation

Download pre-built binaries from the [latest release](https://github.com/witlox/pact/releases/latest):

```bash
# Platform binaries (journal, CLI, MCP) — pick your arch
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-platform-x86_64.tar.gz
tar xzf pact-platform-x86_64.tar.gz -C /usr/local/bin/

# Agent binary — pick your arch + GPU + supervisor variant
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-agent-x86_64-nvidia-pact.tar.gz
tar xzf pact-agent-x86_64-nvidia-pact.tar.gz -C /usr/local/bin/
```

| Platform binaries | Agent variants |
|-------------------|----------------|
| `pact-platform-x86_64` | `pact-agent-x86_64-{pact,systemd}` |
| `pact-platform-aarch64` | `pact-agent-x86_64-nvidia-{pact,systemd}` |
| | `pact-agent-x86_64-amd-{pact,systemd}` |
| | `pact-agent-aarch64-{pact,systemd}` |
| | `pact-agent-aarch64-nvidia-{pact,systemd}` |

See [ARCHITECTURE.md](ARCHITECTURE.md#feature-flags) for what each variant includes.

## CLI Overview

```bash
# Configuration management
pact status                    # Node/vCluster state, drift, capabilities
pact diff                      # Declared vs actual (git diff for config)
pact commit -m "msg"           # Commit drift (local or cluster-wide)
pact rollback [seq]            # Roll back to previous state
pact log [-n N]                # Configuration history
pact apply <spec.toml>         # Apply declarative config spec

# Admin operations (replaces SSH)
pact exec <node> -- <command>  # Run diagnostic command on node
pact shell <node>              # Interactive pact shell on node
pact service <action> <name>   # Service management (start/stop/restart/status)
pact diag <node>              # Diagnostic logs (dmesg, syslog, services)
pact diag --vcluster X --grep "ECC"  # Fleet-wide log grep

# Delta promotion
pact promote <node>            # Export committed node deltas as overlay TOML

# Node lifecycle (delegated)
pact drain <node>              # Drain workloads from node (→ lattice)
pact cordon <node>             # Mark node as unschedulable (→ lattice)
pact uncordon <node>           # Remove cordon from node (→ lattice)
pact reboot <node>             # Reboot node via BMC/Redfish (→ OpenCHAMI)
pact reimage <node>            # Re-image node via OpenCHAMI

# Operational
pact watch                     # Live event stream
pact emergency                 # Enter/exit emergency mode
pact cap                       # Capability report
pact group                     # vCluster/group management
pact blacklist                 # Drift detection exclusions

# Authentication
pact login                     # OIDC login (Auth Code, Device Code, Service Account)
pact logout                    # Clear session

# Supercharged (pact + lattice)
pact jobs list [--node X]         # List running allocations
pact jobs cancel <id>             # Cancel a stuck job
pact jobs inspect <id>            # Job details
pact queue [--vcluster X]         # Scheduling queue status
pact cluster                      # Combined Raft cluster health
pact audit [--source all]         # Unified audit trail
pact accounting [--vcluster X]    # Resource usage (GPU/CPU hours)
pact health                       # Combined system health check
pact services list                # List registered lattice services
pact services lookup <name>       # Service endpoint details
```

## Boot Sequence (pact as init)

```
Kernel → SquashFS root → pact-agent (PID 1)
  → auth to journal → stream vCluster config overlay
  → apply: kernel params, modules, mounts, uenv
  → start services: chronyd, nvidia-persistenced, lattice-node-agent, ...
  → report capabilities to scheduler
  → steady state: observer + shell server + commit windows
```

Total: <2 seconds from pact-agent start to node ready.

## Contributing with Claude Code

This project includes structured [Claude Code](https://claude.com/claude-code) profiles for different development phases: analyst, architect, adversary, implementer, and integrator. Each profile constrains Claude to a specific role in the workflow defined in ['.claude/CLAUDE.md'](.claude/CLAUDE.md), which is loaded by default.

## License

[Apache-2.0](LICENSE)


## Citation

If you use this in research, please cite:

```bibtex
@software{pact,
  title={pact: Promise-based configuration management and admin operations for HPC/AI infrastructure},
  author={Pim Witlox},
  year={2026},
  url={https://github.com/witlox/pact}
}
```

