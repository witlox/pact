# Shell Server Design

## Overview

The pact shell server replaces SSH (ADR-007) as the sole admin interface to compute
nodes. It provides authenticated, audited, policy-enforced command execution.

## Two Execution Modes

### 1. Single Command (`pact exec`)
- Fork/exec a single command directly (no shell interpretation)
- Whitelisted commands only (37 defaults + vCluster policy additions)
- Streaming output: stdout → stderr → exit code
- Timeout enforcement (5 minutes default, 10MB output limit)

### 2. Interactive Shell (`pact shell`)
- Allocates a PTY pair via `openpty()`
- Spawns `/bin/rbash` (restricted bash)
- Session-specific restricted PATH
- PROMPT_COMMAND audit logging
- Terminal resize support (TIOCSWINSZ)

## gRPC Service

```protobuf
service ShellService {
  rpc Exec(ExecRequest) returns (stream ExecOutput);
  rpc Shell(stream ShellInput) returns (stream ShellOutput);
  rpc ListCommands(ListCommandsRequest) returns (ListCommandsResponse);
  rpc ExtendCommitWindow(ExtendWindowRequest) returns (ExtendWindowResponse);
}
```

## Authentication Flow

```
gRPC metadata → extract Bearer token → validate JWT → extract Identity
                                         │
                                    HS256 (dev) or RS256/JWKS (prod)
```

## Authorization Flow

```
Identity → whitelist check → platform admin bypass? → role check → classify
              │                      │                    │            │
              ├─ allowed?            ├─ S2: admin can     ├─ ops?      ├─ state-changing?
              │   no → learning      │   exec anything    │   yes      │   yes → commit window
              │         mode record  │                    ├─ viewer?   │   no → read-only
              └─ yes                 │                    │   read-    │
                                     │                    │   only     │
                                     │                    │   cmds     │
                                     │                    └─ deny      │
```

## Default Whitelist (37 commands)

**Diagnostic**: nvidia-smi, rocm-smi, ps, top, htop, lspci, lsmod, lsblk, lscpu
**Network**: ip, ss, ping, traceroute, ethtool
**File inspection**: cat, head, tail, wc, ls, stat, file, md5sum, sha256sum, diff, grep
**System**: journalctl, sysctl, dmesg, uname, hostname, uptime, free, df, mount, echo
**State-changing**: systemctl, modprobe, umount, sysctl (write mode)

## Command Argument Validation

Before executing any command via `pact exec`, the shell server runs `validate_args()`
on the provided arguments. This blocks access to sensitive paths including
`/etc/shadow`, `/.ssh/`, `/root/`, and CA key material. Path arguments are
normalized (resolving `..`, symlinks, and percent-encoding) to prevent traversal
attacks before the check is applied.

## Session Security

| Control | Mechanism |
|---------|-----------|
| PATH restriction | Symlinks in `/run/pact/shell/{sid}/bin/` |
| Shell restriction | `/bin/rbash` prevents PATH changes |
| Startup injection | `BASH_ENV=""`, `ENV=""` |
| Home access | `HOME=/tmp` |
| Audit | `PROMPT_COMMAND='history 1 >> /var/log/pact/shell.log'` |
| Session limit | Configurable max concurrent sessions |
| Stale cleanup | Sessions in Closing state cleaned after timeout |
