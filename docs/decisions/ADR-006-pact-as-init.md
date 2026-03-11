# ADR-006: Pact-Agent as Init System with SystemD Fallback

## Status: Accepted

## Context

Diskless HPC compute nodes run 4-7 services. systemd is designed for general-purpose
systems with hundreds of units. We need to decide who manages process lifecycle.

## Decision

**pact-agent includes a built-in process supervisor (PactSupervisor) as the default.
systemd is available as a fallback for conservative deployments.**

Both backends implement the same ServiceManager trait. VCluster config selects which.

## Rationale

On a diskless node with a known, small, declared set of services (chronyd,
nvidia-persistenced, lattice-node-agent, metrics-exporter, maybe audit-forwarder),
the process supervision requirements are simple:

- Start N processes in dependency order
- Monitor health, restart on failure with backoff
- Manage cgroup v2 isolation (memory/CPU limits)
- Capture stdout/stderr
- Ordered shutdown

This is ~1000-1500 lines of Rust with tokio::process and cgroup v2 filesystem API.
systemd adds ~50MB of binary, D-Bus, journald, and hundreds of unit types that are
never used.

Benefits of pact as init:
- Every service lifecycle change is inherently a pact operation (logged, auditable)
- No log ownership conflict between journald and pact's log pipeline
- Smaller base image (no systemd, no D-Bus, no logind)
- Boot is faster (no unit parsing, no generator execution)
- Single process to debug if something goes wrong

## systemd Fallback

Some deployments may prefer systemd for:
- Existing operational tooling assumes systemd
- Compliance requirements mandate specific init system
- Third-party software requires systemd features (socket activation, etc.)

The fallback is selected per vCluster:
```toml
[agent.supervisor]
backend = "systemd"
```

pact-agent generates systemd unit files and delegates via D-Bus.

## Trade-offs

- PactSupervisor must handle edge cases: zombie reaping, OOM killer interaction,
  signal propagation, cgroup cleanup on crash
- Software that expects systemd (rare in HPC compute context) needs adaptation
- Two code paths to maintain (though the trait abstraction minimizes this)
