# pact

**Promise-based configuration management and admin operations for HPC/AI infrastructure**

pact manages post-boot runtime configuration on large-scale HPC/AI clusters
and provides the sole admin operations interface to compute nodes — replacing both
traditional config management tools and SSH.

On compute nodes, pact-agent is the init system: it supervises all services, manages
configuration, and provides authenticated remote access via pact shell.

## Key Features

- **No SSH needed** — pact shell provides authenticated, audited, policy-scoped remote access
- **pact-agent as init** — boots diskless nodes in <2s, supervises 4-7 services directly
- **Acknowledged drift** — detected, measured, explicitly handled — never silently converged
- **Immutable audit log** — every action recorded, any state reconstructible
- **Optimistic concurrency** — apply first, commit within time window, rollback on expiry
- **10,000+ node scale** — streaming boot config, no per-node scrape targets

## Architecture Overview

```
Admin Plane      pact CLI / pact shell / AI Agent (MCP)
Control Plane    pact-journal (Raft) + pact-policy (IAM/OPA) + Grafana/Loki
Node Plane       pact-agent (init + supervisor + observer + shell server)
Infrastructure   OpenCHAMI (boot) → pact (config + init) → lattice (scheduling)
```

Integrates with [Lattice](https://github.com/witlox/lattice),
[OpenCHAMI](https://openchami.org), and [Sovra](https://github.com/witlox/sovra).
