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
- Opens an interactive shell session on the node
- Authenticated + authorized (higher privilege than exec — separate permission)
- Every command logged to journal with caller identity
- State-changing commands trigger commit windows (same model as before)
- Read-only commands execute immediately with logging only
- Session recorded for audit trail

### Whitelist model
- Default whitelist: common diagnostics (nvidia-smi, dmesg, lspci, ip, ss, cat,
  journalctl, mount, df, free, top, ps, lsmod, sysctl -a, etc.)
- Learning mode: any command is allowed but non-whitelisted commands generate
  alerts and suggestions to add them
- vCluster-scoped: regulated vClusters may have tighter whitelists
- Platform admins can execute any command (whitelist bypass)

### State-changing command detection
The agent classifies commands as read-only or state-changing:
- **Read-only**: commands that only read system state (nvidia-smi, dmesg, cat, ps, ...)
- **State-changing**: commands that modify state (mount, umount, systemctl, sysctl -w,
  ip addr add, modprobe, ...)

State-changing commands go through the commit window model. Read-only commands
execute immediately.

## Fallback

When pact-agent is unresponsive:
1. Admin uses OpenCHAMI/Manta to access BMC console (Redfish)
2. BMC console provides serial console access to the node
3. On the node, pact shell is available locally (if agent is running but network is down)
4. If pact-agent is crashed, admin can restart it from BMC console
5. If the node is unrecoverable, admin triggers re-image via OpenCHAMI

## Trade-offs

- Admins lose the flexibility of arbitrary SSH access
- Whitelist maintenance is ongoing operational work (mitigated by learning mode)
- Slightly higher latency than direct SSH for some operations
- Requires trust in pact-agent reliability (mitigated by BMC fallback)

## Security Benefits

- All remote access is authenticated (OIDC) and authorized (RBAC)
- Every command is logged with authenticated identity
- State changes are tracked and require commitment
- No root shell escape — pact enforces policy even for platform admins
- Attack surface reduced: no sshd, no SSH key management, no SSH vulnerabilities
