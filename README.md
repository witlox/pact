# pact

**Promise-based configuration management and admin operations for HPC/AI infrastructure**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://github.com/witlox/pact)

---

pact manages post-boot runtime configuration on large-scale diskless HPC/AI clusters
and provides the sole admin operations interface to compute nodes — replacing both
traditional config management tools and SSH.

On compute nodes, pact-agent is the init system: it supervises all services, manages
configuration, and provides authenticated remote access via pact shell.

## Why pact?

- **No SSH needed** — pact shell provides authenticated, audited, policy-scoped remote access
- **pact-agent as init** — boots diskless nodes in <2s, supervises 4-7 services directly
- **Acknowledged drift** — detected, measured, explicitly handled — never silently converged
- **Immutable audit log** — every action recorded, any state reconstructible
- **Optimistic concurrency** — apply first, commit within time window, rollback on expiry
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

# Operational
pact watch                     # Live event stream
pact emergency                 # Enter/exit emergency mode
pact cap                       # Capability report
pact group                     # vCluster/group management
pact blacklist                 # Drift detection exclusions
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

## Documentation

- [System Architecture](docs/architecture/system-architecture.md)
- [Agent Design](docs/architecture/agent-design.md) (supervisor + shell + observer)
- [Journal Design](docs/architecture/journal-design.md)
- [CLI & Shell Design](docs/architecture/cli-design.md)
- [Observability](docs/architecture/observability.md)
- [Agentic API (MCP)](docs/architecture/agentic-api.md)
- [Federation Model](docs/architecture/federation.md)
- [Architecture Decision Records](docs/decisions/)

## License

[Apache-2.0](LICENSE)
