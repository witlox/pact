# ADR-006: Pact-Agent as Init System with SystemD Fallback

## Status: Accepted (amended 2026-03-17)

## Context

Diskless HPC compute nodes run 5-9 services. systemd is designed for general-purpose
systems with hundreds of units. We need to decide who manages process lifecycle.

## Decision

**pact-agent includes a built-in process supervisor (PactSupervisor) as the default.
systemd is available as a fallback for conservative deployments.**

Both backends implement the same ServiceManager trait. VCluster config selects which.

### Amendment (2026-03-17): Sub-context decomposition

Node management is decomposed into six bounded contexts, each with a **strategy pattern**
providing PactSupervisor (default) and SystemdBackend (compat) implementations:

1. **Process Supervision** — service lifecycle, background supervision loop, health checks,
   dependency ordering, restart policies
2. **Resource Isolation** — cgroup v2 hierarchy, per-service scopes, OOM containment,
   namespace creation for lattice allocations
3. **Identity Mapping** — OIDC→POSIX UID/GID translation for NFS (pact-nss module).
   Only active in PactSupervisor mode.
4. **Network Management** — netlink interface configuration. Replaces wickedd/NetworkManager
   in PactSupervisor mode.
5. **Platform Bootstrap** — boot phases (InitHardware → ConfigureNetwork → LoadIdentity →
   PullOverlay → StartServices → Ready), hardware watchdog, SPIRE integration, coldplug.
6. **Workload Integration** — namespace handoff to lattice (unix socket, SCM_RIGHTS),
   mount refcounting, readiness gate. Contract defined in hpc-core.

Cross-cutting: audit events (hpc-audit AuditSink trait) emitted by all contexts.

### Supervision loop

PactSupervisor includes a background supervision loop that:
- Polls process status and triggers restarts per RestartPolicy
- Uses adaptive interval: faster when idle (500ms, deeper eBPF inspections), slower
  when workloads active (2-5s, minimal overhead)
- Is coupled to the hardware watchdog — each loop tick pets `/dev/watchdog`
- If the loop hangs, the watchdog expires and BMC triggers hard reboot
- Only runs in PactSupervisor mode. SystemdBackend delegates restarts to systemd natively.

### Hardware watchdog

When pact-agent is PID 1 on BMC-equipped nodes, it pets `/dev/watchdog`. If no hardware
watchdog is available, the node runs in systemd mode (pact is a regular service, not PID 1).
The watchdog is the crash/hang recovery mechanism for PID 1 — there is no other process
that can restart PID 1.

### cgroup v2 enforcement

PactSupervisor creates a cgroup v2 hierarchy at boot:
- `pact.slice/` — pact-owned system services (infra, network, gpu, audit sub-slices)
- `workload.slice/` — lattice-owned workload allocations
- pact-agent itself runs with OOMScoreAdj=-1000

Each supervised service gets a CgroupScope with configurable resource limits. On process
death, all children in the scope are killed via `cgroup.kill` — no orphans.

Ownership boundary: exclusive write per slice, shared read for metrics, emergency override
with audit trail for cross-slice intervention.

### Real service sets (from HPE Cray EX compute nodes)

Derived from actual `ps aux` analysis. pact replaces: systemd, atomd (HPE ATOM),
nomad + 17 executors, slurmd, munged, sssd, ldmsd, nrpe, hb_ref, rsyslogd, wickedd,
udevd, haveged, DVS-IPC, agetty, bos.reporter.

**ML training (GPU) — 7 services:**
chronyd, dbus-daemon, cxi_rh (×4 per NIC), nvidia-persistenced, nv-hostengine,
rasdaemon, lattice-node-agent. Plus rpcbind/rpc.statd if NFS.

**Regulated/sensitive — +2:** auditd, audit-forwarder = 9 services.

**Dev sandbox — 5:** chronyd, dbus-daemon, cxi_rh, rasdaemon, lattice-node-agent.

## Rationale

On a diskless node with a known, small, declared set of services, the process supervision
requirements are simple:

- Start N processes in dependency order
- Monitor health, restart on failure with backoff (supervision loop)
- Manage cgroup v2 isolation (memory/CPU limits per service)
- Kill orphaned children on process death (cgroup.kill)
- Hardware watchdog for agent hang detection
- Adaptive polling to minimize workload disturbance
- Ordered shutdown

Benefits of pact as init:
- Every service lifecycle change is inherently a pact operation (logged, auditable)
- No log ownership conflict between journald and pact's log pipeline
- Smaller base image (no systemd, no D-Bus if DCGM standalone, no logind)
- Boot is faster (no unit parsing, no generator execution)
- Single process to debug if something goes wrong
- cgroup hierarchy owned by pact, shared contract with lattice via hpc-core
- Namespace pre-creation and mount refcounting for lattice ("steroids" mode)
- Network configuration via netlink (no wickedd daemon)
- Identity mapping for NFS (no SSSD)

## systemd Fallback

Some deployments may prefer systemd for:
- Existing operational tooling assumes systemd
- Compliance requirements mandate specific init system
- Third-party software requires systemd features (socket activation, etc.)
- No hardware watchdog available

The fallback is selected per vCluster:
```toml
[agent.supervisor]
backend = "systemd"
```

In systemd mode, pact-agent:
- Does NOT manage the hardware watchdog (systemd handles it)
- Does NOT configure network interfaces (wickedd/NetworkManager handles it)
- Does NOT write identity mapping .db files (SSSD handles it)
- Does NOT create cgroup hierarchy (systemd manages it)
- DOES pull overlays and manage config state (pact-specific)
- DOES manage pact-specific services via generated systemd unit files

## Trade-offs

- PactSupervisor must handle edge cases: zombie reaping, OOM killer interaction,
  signal propagation, cgroup cleanup on crash, watchdog petting, adaptive polling
- Software that expects systemd (rare in HPC compute context) needs adaptation
- Two code paths to maintain (though the strategy pattern minimizes this)
- Six sub-contexts increase architectural complexity but enable independent testing
  and clear ownership boundaries

## References

- specs/domain-model.md §2a-2f (sub-context decomposition)
- specs/invariants.md PS1-PS3, RI1-RI6, IM1-IM7, NM1-NM2, PB1-PB5, WI1-WI6
- specs/features/ (resource_isolation, identity_mapping, network_management,
  platform_bootstrap, workload_integration feature files)
- specs/failure-modes.md F21-F36
- ADR-015 (hpc-core shared contracts)
- ADR-016 (identity mapping)
