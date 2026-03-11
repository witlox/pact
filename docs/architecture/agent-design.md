# Agent Design

## Overview

pact-agent is the init system, configuration manager, process supervisor, and shell
server for diskless HPC/AI compute nodes. It is PID 1 (or near-PID-1) and the only
management process that starts from the base boot image.

## Subsystems

### Process Supervisor (`src/supervisor/`)

Two backends behind the `ServiceManager` trait:

**PactSupervisor** (default):
- Direct process management via `tokio::process::Command`
- cgroup v2 isolation: creates `/sys/fs/cgroup/pact.slice/<service>/` per service
- Memory limits, CPU quotas via cgroup controllers
- Health checks: process alive + optional HTTP/TCP endpoint check
- Restart with exponential backoff (configurable per service)
- Dependency ordering from service declarations in vCluster overlay
- Zombie reaping: pact-agent sets PR_SET_CHILD_SUBREAPER
- stdout/stderr capture via pipes → pact log pipeline → Loki
- Ordered shutdown: reverse dependency order, SIGTERM → grace period → SIGKILL

**SystemdBackend** (fallback):
- Generates systemd unit files from vCluster service declarations
- Start/stop/restart via D-Bus connection to systemd
- Monitor via sd_notify protocol
- Same ServiceManager trait — transparent to rest of pact-agent

### Shell Server (`src/shell/`)

Replaces SSH. Listens on a gRPC endpoint (mTLS authenticated).

**pact exec** (single command):
```
Client → ExecRequest{node_id, command, args} → pact-agent
  → authenticate (OIDC token verification)
  → authorize (RBAC: caller's role allows exec on this vCluster?)
  → whitelist check (command in allowed set?)
  → classify (read-only or state-changing?)
  → if state-changing: go through commit window model
  → execute via fork/exec in restricted environment
  → stream stdout/stderr back to client
  → log command + output to journal
```

**pact shell** (interactive session):
```
Client → ShellSessionRequest{node_id} → pact-agent
  → authenticate + authorize (shell requires higher privilege than exec)
  → open bidirectional gRPC stream
  → allocate PTY on node
  → each command: whitelist check → classify → execute → log
  → state-changing commands trigger commit windows
  → session recorded in journal (commands + timestamps, not full output)
  → session ends: cleanup PTY, log session summary
```

**Whitelist with learning mode**:
- Default whitelist: nvidia-smi, dmesg, lspci, ip, ss, cat, less, head, tail,
  grep, journalctl, mount (read), df, free, top, ps, lsmod, sysctl (read),
  uname, hostname, date, uptime, lscpu, lsmem, lsblk, findmnt, ethtool
- State-changing commands: mount (write), umount, sysctl -w, modprobe, rmmod,
  ip addr/route (write), service management (via pact service)
- Learning mode: non-whitelisted commands allowed but generate alert + suggestion
- Platform admins: whitelist bypass (but still logged)

### State Observer (`src/observer/`)

Three detection mechanisms:
- eBPF probes: mount, sethostname, sysctl writes, module load/unload
- inotify: config file paths (derived from declared state + watch list)
- netlink: interface state, address changes, mount events, routing

Observe-only mode for initial deployment (log everything, enforce nothing).

### Drift Evaluator (`src/drift/`)

DriftVector across 7 dimensions (mounts, files, network, services, kernel, packages,
gpu). Magnitude = weighted Euclidean norm with per-vCluster dimension weights.

### Commit Window Manager (`src/commit/`)

Optimistic concurrency. Active consumer check before rollback (don't unmount
filesystems with open handles). Emergency mode: extended window + suspended rollback.

### Capability Reporter (`src/capability/`)

Reports to lattice scheduler (gRPC) and local tmpfs manifest + unix socket
(consumed by lattice-node-agent, which pact supervises as a child process).

### Emergency Mode (`src/emergency/`)

`pact emergency --reason "..."` → extended window, no rollback, full audit logging.
Must end with explicit commit or rollback. Stale emergency → alert + scheduling hold.

## Resource Budget

- RSS: < 50 MB (including eBPF maps and supervisor overhead)
- CPU steady state: < 0.5%
- CPU during drift eval: < 2%
- CPU during shell session: depends on commands executed
