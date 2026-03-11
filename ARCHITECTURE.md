# Architecture Overview

## System Context

```
┌─────────────────────────────────────────────────────────────────┐
│ User Workloads (jobs, inference services, simulations)          │
├─────────────────────────────────────────────────────────────────┤
│ Lattice Scheduler (job placement, vCluster management)          │
├─────────────────────────────────────────────────────────────────┤
│ pact (init + config + admin ops + process supervision)          │  ← this project
├─────────────────────────────────────────────────────────────────┤
│ OpenCHAMI (hardware discovery, boot provisioning, DHCP)         │
├─────────────────────────────────────────────────────────────────┤
│ Hardware (nodes, GPUs, network fabric, BMC/Redfish)             │
└─────────────────────────────────────────────────────────────────┘

Cross-cutting: Sovra (federated key management, policy federation)
```

## What pact does

Two core functions:

**1. Configuration management**: Declare desired state for vClusters and nodes. Detect
drift. Commit/rollback with time-windowed optimistic concurrency. Immutable audit log.
Stream config to 10,000+ nodes at boot.

**2. Admin operations**: Authenticated, audited, policy-scoped remote access to compute
nodes. Replaces SSH entirely. Pact shell for interactive debugging. Remote exec for
diagnostics. Service lifecycle management.

## What pact delegates

- **To OpenCHAMI/Manta**: reboot, re-image, firmware updates, hardware discovery, DHCP
- **To lattice**: drain/cordon nodes, job management, vCluster creation, scheduling policy
- **To Sovra**: cross-site trust establishment, federated policy attestation

pact provides a unified CLI for all operations, calling OpenCHAMI and lattice APIs
for delegated actions.

## Component Architecture

### pact-agent (every compute node)

The init system for diskless compute nodes. First process to start after kernel boot.

- **Process Supervisor**: starts/stops/monitors declared services, cgroup v2 isolation,
  restart with backoff. Default: built-in PactSupervisor. Fallback: SystemdBackend.
- **Shell Server**: authenticated remote shell access (replaces SSH). OIDC-scoped.
  Whitelisted command set with learning mode. Every command logged.
- **Exec Endpoint**: single command execution for diagnostics and automation.
- **State Observer**: eBPF probes (system-level, no overlap with lattice's workload-level
  eBPF), inotify watches, netlink subscriptions.
- **Drift Evaluator**: actual vs declared state, drift vector, magnitude computation.
- **Commit Window Manager**: optimistic concurrency, emergency mode.
- **Capability Reporter**: multi-vendor GPU detection (NVIDIA via NVML, AMD via ROCm SMI),
  hardware inventory → lattice scheduler + local manifest.

### pact-journal (Raft quorum, 3-5 nodes)

Separate Raft group from lattice scheduler. Immutable append-only log.

- Streaming boot config (two-phase: vCluster overlay + node delta)
- Event stream to Loki for Grafana dashboards
- Read replicas for 100k+ boot storms

### pact-policy

- OIDC/SAML integration (shared identity provider with lattice/OpenCHAMI)
- RBAC with per-vCluster role scoping
- OPA/Rego policy engine co-located on journal nodes (see [ADR-003](docs/decisions/ADR-003-policy-engine.md))
- Sovra federation sync for Rego policy templates
- Two-person approval workflow for regulated vClusters

### pact CLI

Admin workstation tool. Also runs locally as pact shell on nodes.

## Boot Sequence

```
T+0.0s  PXE → kernel + initramfs → mount SquashFS root (OpenCHAMI)
T+0.1s  pact-agent starts (PID 1 or early init target)
T+0.2s  mTLS auth to pact-journal
T+0.3s  Phase 1: stream vCluster base overlay (~100-200 KB compressed)
T+0.5s  Apply: sysctl, kernel modules, NFS/Lustre mounts, base uenv
T+0.6s  Phase 2: node-specific delta (<1 KB)
T+0.7s  Start declared services in dependency order:
          chronyd → nvidia-persistenced → metrics-exporter →
          lattice-node-agent → (audit-forwarder if regulated)
T+1.5s  CapabilityReport to lattice scheduler. Node eligible for jobs.
T+1.6s  Steady state: observer active, shell server listening.
```

## Process Supervision

### Why not systemd?

A diskless HPC compute node runs 4-7 services. systemd is designed for general-purpose
systems with hundreds of units. The overhead (binary size, D-Bus, journald, unit
parsing) is unnecessary for a known, small, declared set of processes.

### PactSupervisor (default)

Built into pact-agent. Direct process management:

- fork/exec child processes
- cgroup v2 isolation (memory limits, CPU quotas per service)
- Health checks (process alive, optional HTTP/TCP health endpoint)
- Restart with exponential backoff
- Ordered startup (dependency graph from vCluster overlay)
- Ordered shutdown (reverse dependency order)
- Zombie reaping (pact-agent as PID 1 subreaper)
- stdout/stderr capture → pact log pipeline → Loki

### SystemdBackend (fallback)

For deployments that prefer systemd:

- pact-agent generates systemd unit files from vCluster overlay
- Delegates start/stop/restart to systemd via D-Bus
- Monitors via systemd notifications
- Same pact abstraction — different execution engine

Selected per node/vCluster:
```toml
[agent.supervisor]
backend = "pact"  # "pact" | "systemd"
```

### Service Declaration

```toml
[vcluster.ml-training.services]

[vcluster.ml-training.services.chronyd]
binary = "/usr/sbin/chronyd"
args = ["-d"]  # foreground
restart = "always"
restart_delay_seconds = 5
cgroup_memory_max = "64M"
order = 1

[vcluster.ml-training.services.nvidia-persistenced]
binary = "/usr/bin/nvidia-persistenced"
args = ["--no-persistence-mode", "--verbose"]
restart = "always"
restart_delay_seconds = 2
depends_on = []
order = 2

[vcluster.ml-training.services.lattice-node-agent]
binary = "/usr/bin/lattice-node-agent"
args = ["--config", "/etc/lattice/agent.toml"]
restart = "always"
restart_delay_seconds = 5
depends_on = ["chronyd"]
health_check = { type = "http", url = "http://localhost:9100/health", interval_seconds = 30 }
order = 10
```

## OIDC Role Model

```
pact-platform-admin         Full access. Journal mgmt, global policy, all vClusters.
pact-ops-{vcluster}         Day-to-day ops. Exec, commit, emergency, services.
pact-viewer-{vcluster}      Read-only. Status, diff, log, watch, cap.
pact-regulated-{vcluster}   Like ops + two-person approval for state changes.
pact-service-agent           Machine identity for pact-agents (mTLS).
pact-service-ai              Machine identity for AI agents (MCP).
```

Mapped from OIDC groups to pact RBAC roles in pact-policy. Every operation
authenticated and authorized against caller's role + target scope.

## No SSH Model

- pact shell and pact exec are the only remote access to compute nodes
- BMC/Redfish console (via OpenCHAMI/Magellan) is the out-of-band fallback
- All remote commands are: authenticated (OIDC), authorized (RBAC),
  logged (immutable journal), scoped (per-vCluster)
- State-changing commands go through commit window model
- Read-only diagnostics execute immediately with logging only

## Observability

```
pact-journal server metrics ──→ Prometheus ──→ Grafana
Config + admin events ──→ pact-journal ──→ Loki ──→ Grafana
pact-agent process health ──→ lattice-node-agent eBPF ──→ Prometheus
Alerts ──→ Grafana alerting ──→ PagerDuty / Slack
```

## Federation (via Sovra)

- Configuration state is site-local
- Policy templates are federated via Sovra mTLS
- Consistent with lattice's federation model
