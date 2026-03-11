# ADR-007: No SSH — Pact Shell Replaces Remote Access

## Status: Accepted

## Context

SSH on diskless HPC nodes creates untracked, unaudited, unrestricted root shells.
Every SSH session is a potential source of unacknowledged configuration drift. The
drift detection system exists largely because SSH enables uncontrolled changes.

## Decision

**pact shell and pact exec are the sole remote access mechanisms for compute nodes.
SSH is not installed on compute node images. BMC/Redfish console (via OpenCHAMI) is
the out-of-band fallback for pact-agent failures.**

## Design

### pact exec (single command)
```
pact exec node042 -- nvidia-smi
pact exec node042 -- dmesg --since "5 minutes ago"
pact exec node042 -- cat /etc/resolv.conf
```
- Authenticated via OIDC token
- Authorized against caller's role + target node's vCluster
- Command checked against whitelist (configurable per vCluster)
- stdout/stderr streamed back to caller
- Full command + output logged to journal

### pact shell (interactive session)
```
pact shell node042
```
- Opens a **restricted bash** session on the node (not a custom shell)
- Authenticated + authorized (higher privilege than exec — separate permission)
- Restriction via environment control, not command parsing:
  - `rbash` (restricted bash): prevents changing PATH, running `/absolute/paths`,
    redirecting output to files
  - PATH limited to whitelisted commands via session-specific directory
  - `PROMPT_COMMAND` hook logs each executed command to pact audit
  - Optional mount namespace hides sensitive paths
  - Session-level cgroup for resource limits
- State changes detected by the existing drift observer (eBPF + inotify + netlink)
  and trigger commit windows — the shell doesn't pre-classify commands
- Session start/end recorded in journal

### Why restricted bash, not a custom shell
Implementing a shell that interprets pipes, redirects, globbing, quoting,
subshells, environment variables, job control, and signal handling is
reimplementing bash — poorly. And parsing commands before execution to
classify them is a security problem: `$(evil)`, backticks, `eval`, and
argument injection make pre-execution parsing unreliable.

Instead, pact controls what bash **can reach** (PATH, namespace, cgroup) and
**observes what happened** (PROMPT_COMMAND audit, drift detection). Bash handles
interactive shell semantics — it's been doing that for 35 years.

### Whitelist model
- Implemented as PATH restriction: only whitelisted binaries are symlinked
  into the session's bin directory
- Default whitelist: common diagnostics (nvidia-smi, dmesg, lspci, ip, ss, cat,
  journalctl, mount, df, free, top, ps, lsmod, sysctl -a, etc.)
- Learning mode: "command not found" errors are captured by the agent, which
  suggests adding the command to the vCluster whitelist
- vCluster-scoped: regulated vClusters may have tighter whitelists
- Platform admins: broader PATH (but still logged via PROMPT_COMMAND)

### State-changing command detection
The agent does **not** classify commands before execution in shell mode.
Instead, the existing drift observer (eBPF probes, inotify, netlink) detects
actual state changes and triggers commit windows. This is the same mechanism
used for any other source of drift — the shell session is not special.

For pact exec (single commands), the agent does classify commands upfront
because it controls the full invocation (no shell interpretation involved).

## Fallback

When pact-agent is unresponsive:
1. Admin uses OpenCHAMI/Manta to access BMC console (Redfish)
2. BMC console provides regular bash (unrestricted, not pact-managed)
3. Admin diagnoses and restarts pact-agent if needed
4. Changes made via BMC appear as unattributed drift once agent recovers
5. If the node is unrecoverable, admin triggers re-image via OpenCHAMI

## Trade-offs

- Admins lose the flexibility of arbitrary SSH access
- Whitelist maintenance is ongoing operational work (mitigated by learning mode)
- Slightly higher latency than direct SSH for some operations
- Requires trust in pact-agent reliability (mitigated by BMC fallback)
- rbash restrictions can be bypassed by some binaries (e.g. vi, python, less
  with `!cmd`) — whitelisted commands must be audited for shell escape vectors

## Security Benefits

- All remote access is authenticated (OIDC) and authorized (RBAC)
- Every command is logged with authenticated identity
- State changes are tracked and require commitment
- No unrestricted root shell — pact controls the environment via PATH, rbash,
  and optional mount namespace
- Attack surface reduced: no sshd, no SSH key management, no SSH vulnerabilities
