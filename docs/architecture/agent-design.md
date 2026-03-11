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
  → authorize: call PolicyService.Evaluate() on policy node (full OPA/Rego)
      if policy service unreachable: fall back to cached VClusterPolicy
      (role_bindings + whitelist only; two-person approval denied)
  → whitelist check (command in allowed set?)
  → classify (read-only or state-changing?)
  → if state-changing: go through commit window model
  → execute via fork/exec in restricted environment
  → stream stdout/stderr back to client
  → log command + output to journal
```

For exec, pact-agent controls the full command — it receives a command + args,
validates against the whitelist, and fork/execs directly. No shell interpretation.

**pact shell** (interactive session — restricted bash):

pact shell does **not** reimplement a shell. It spawns a restricted bash session
inside a controlled environment. Reimplementing line editing, pipes, redirects,
globbing, quoting, job control, and signal handling would be both enormous and
a security liability (command parsing bugs = bypasses).

```
Client → ShellSessionRequest{node_id} → pact-agent
  → authenticate + authorize (same policy call as exec; shell requires
    higher privilege — if policy service unreachable, cached RBAC check)
  → open bidirectional gRPC stream
  → allocate PTY with restricted bash environment:
      - PATH restricted to whitelisted command directories
      - readonly PATH, ENV, BASH_ENV, SHELL (prevent escape)
      - custom PROMPT_COMMAND logs each command to pact audit
      - rbash or bash --restricted as base
      - mount namespace: hide sensitive paths if configured
      - cgroup: session-level resource limits
  → session start/end logged to journal
  → session ends: cleanup PTY, cgroup, log session summary
```

**Restriction layers** (defense in depth, not command parsing):

1. **PATH restriction**: only whitelisted binaries are reachable. The agent
   builds a restricted PATH from the vCluster's `shell_whitelist`, symlinking
   allowed commands into a session-specific directory (`/run/pact/shell/<sid>/bin/`).
   Bash in restricted mode (`rbash`) prevents changing PATH or running commands
   by absolute path.

2. **PROMPT_COMMAND audit**: bash's PROMPT_COMMAND hook runs before each prompt,
   logging the previous command (`$(history 1)`) to pact's audit pipeline.
   This captures what was actually executed, not what pact *thinks* was executed.

3. **Mount namespace** (optional): hide `/root`, `/home`, SSH keys, and other
   sensitive paths from the shell session.

4. **Seccomp/cgroup**: session-level resource limits and optional syscall filtering.

5. **State change detection**: the existing drift observer (eBPF + inotify +
   netlink) detects changes made during the session. These trigger commit
   windows as normal — the shell doesn't need to pre-classify commands.

**What pact exec does vs pact shell**:
- **pact exec**: pact controls the full command lifecycle (whitelist, classify,
  fork/exec). No shell involved. Suitable for automation and diagnostics.
- **pact shell**: bash controls command execution. pact controls the environment
  (PATH, namespace, cgroup) and observes changes after the fact. Suitable for
  interactive debugging.

**Learning mode**: when a user tries to run a command not in PATH, bash returns
"command not found". The agent detects this (via audit log or PROMPT_COMMAND
exit code) and suggests adding the command to the vCluster whitelist.

### State Observer (`src/observer/`)

Three detection mechanisms:
- eBPF probes (feature-gated `ebpf`, Linux-only):
  - System-level: mount, sethostname, sysctl writes, module load/unload
  - Extended: file permission changes, network namespace operations, cgroup modifications
  - **No overlap with lattice eBPF**: lattice traces workload-level events (job lifecycle,
    GPU allocation). pact traces system-level config changes. Probe attachment points
    are coordinated to avoid conflicts.
- inotify: config file paths (derived from declared state + watch list)
- netlink: interface state, address changes, mount events, routing

Observe-only mode for initial deployment (log everything, enforce nothing).

**Cross-platform**: On macOS (development), a `MockObserver` simulates drift events
for local dev/test. Real observers only compile and run on Linux.

### Drift Evaluator (`src/drift/`)

DriftVector across 7 dimensions (mounts, files, network, services, kernel, packages,
gpu). Magnitude = weighted Euclidean norm with per-vCluster dimension weights.

### Commit Window Manager (`src/commit/`)

Optimistic concurrency. Active consumer check before rollback (don't unmount
filesystems with open handles). Emergency mode: extended window + suspended rollback.

### Config Subscription (`src/subscription/`)

After boot, the agent subscribes to `BootConfigService.SubscribeConfigUpdates()`
on the journal for live updates. This stream delivers:
- vCluster overlay changes (e.g. `pact apply` updates the overlay)
- Node-specific delta changes (e.g. promoted changes from `pact promote`)
- Policy updates (refreshes cached `VClusterPolicy` for authorization)
- Blacklist changes (updates drift detection exclusions)

This means overlay and policy changes propagate to running nodes **without
reboot**. The agent applies overlay changes through the same path as boot-time
config application. If the subscription stream is interrupted, the agent
reconnects with `from_sequence` to resume from the last received update.

### Capability Reporter (`src/capability/`)

Multi-vendor GPU detection behind a `GpuBackend` trait:
- **NVIDIA**: NVML bindings (feature `nvidia`), fallback: `nvidia-smi` shell-out
- **AMD**: ROCm SMI bindings (feature `amd`), fallback: `rocm-smi` shell-out
- Feature-gated: non-GPU nodes skip GPU detection entirely

Reports to lattice scheduler (gRPC) and local tmpfs manifest + unix socket
(consumed by lattice-node-agent, which pact supervises as a child process).

### Emergency Mode (`src/emergency/`)

`pact emergency --reason "..."` → extended window, no rollback, full audit logging.
Must end with explicit commit or rollback. Stale emergency → alert + scheduling hold.

## Cross-Platform Development

Three-tier strategy for macOS development:
1. **Feature-gate**: `#[cfg(target_os = "linux")]` for cgroup v2, eBPF, netlink,
   inotify, PTY allocation. Stubs compile on macOS.
2. **Mock implementations**: `MockSupervisor`, `MockObserver`, `MockGpuBackend`
   for local dev/test on macOS. Unit + integration tests run with mocks.
3. **Devcontainer**: Linux container for integration + acceptance tests (BDD/cucumber).
   Real supervisor, real observers, real cgroups. CI runs in this environment.

## Resource Budget

- RSS: < 50 MB (including eBPF maps and supervisor overhead)
- CPU steady state: < 0.5%
- CPU during drift eval: < 2%
- CPU during shell session: depends on commands executed
