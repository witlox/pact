# Chunk 1: Shell/Exec Attack Surface

Reviewed: 2026-03-20
Files: shell/whitelist.rs, shell/exec.rs, shell/auth.rs, shell/session.rs, shell/mod.rs, shell/grpc_service.rs

---

## Finding: F1 — `sysctl` classified as read-only despite being in state-changing list as `sysctl-w`
Severity: Medium
Category: Correctness > Specification compliance
Location: `crates/pact-agent/src/shell/whitelist.rs:103`
Spec reference: ADR-007 (state-changing classification)
Description: `sysctl` (line 103) is whitelisted as `state_changing: false` (read-only query). But `sysctl -w` is the write form. The whitelist has a separate entry `sysctl-w` (line 108, `state_changing: true`) but this is a fictional binary name — there is no `sysctl-w` binary on Linux. The real `sysctl` binary handles both reads and writes via the `-w` flag. A user running `pact exec node-001 -- sysctl -w vm.swappiness=10` would bypass the state-changing classification because the whitelist matches on command name `sysctl` (read-only), not `sysctl-w`.
Evidence: `is_state_changing("sysctl")` returns `false`. But `sysctl -w key=value` changes kernel state.
Suggested resolution: Remove `sysctl-w` entry. Mark `sysctl` as `state_changing: true`, or add argument inspection to detect `-w` flag.

---

## Finding: F2 — `ip` command classified as read-only but can modify network state
Severity: Medium
Category: Security > Input validation
Location: `crates/pact-agent/src/shell/whitelist.rs:85`
Spec reference: ADR-007 (whitelist safety)
Description: `ip` is classified as `state_changing: false`. But `ip link set eth0 down`, `ip addr add`, `ip route add` all modify network state. The whitelist checks command name only, not arguments.
Evidence: `is_state_changing("ip")` returns `false`. User can run `pact exec node -- ip link set hsn0 down` which takes down the high-speed network.
Suggested resolution: Either mark `ip` as state-changing, or implement argument-level inspection for known dangerous subcommands (`link set`, `addr add/del`, `route add/del`).

---

## Finding: F3 — `mount` classified as read-only but can mount filesystems
Severity: Medium
Category: Security > Input validation
Location: `crates/pact-agent/src/shell/whitelist.rs:78`
Spec reference: ADR-007 (whitelist safety)
Description: `mount` with no arguments shows mount points (read-only). But `mount /dev/sda1 /mnt` mounts a filesystem. The command is classified as read-only.
Evidence: `is_state_changing("mount")` returns `false`.
Suggested resolution: Mark `mount` as state-changing, or limit to `mount -l` / read-only usage via argument filtering.

---

## Finding: F4 — exec PATH includes /usr/sbin which contains privileged binaries
Severity: Low
Category: Security > Trust boundaries
Location: `crates/pact-agent/src/shell/exec.rs:80`
Spec reference: ADR-007 (restricted execution)
Description: `execute_command()` sets `PATH=/usr/bin:/usr/sbin:/bin:/sbin`. The `/usr/sbin` directory contains system administration tools (e.g., `fdisk`, `mkfs`, `iptables`). While these are not whitelisted, the PATH makes them resolvable. If a future whitelist update adds any sbin command, it would run with full system privileges. The shell PATH (rbash) uses a restricted symlink directory, but `pact exec` uses this broader PATH.
Evidence: Line 80 in exec.rs.
Suggested resolution: Consider restricting exec PATH to match the shell PATH model (symlink directory), or at minimum remove `/usr/sbin` if no whitelisted commands live there.

---

## Finding: F5 — No argument validation on whitelisted commands
Severity: High
Category: Security > Input validation
Location: `crates/pact-agent/src/shell/whitelist.rs:124-126`
Spec reference: ADR-007 (whitelist enforcement)
Description: `is_exec_allowed()` checks only the command name, not its arguments. All whitelisted commands can be called with arbitrary arguments. This enables:
- `grep -r / --include="*.key"` — search for private keys on the entire filesystem
- `cat /etc/shadow` — read password hashes (if agent runs as root)
- `tail -f /var/log/secure` — monitor auth logs indefinitely
- `df -h | cat > /tmp/exfil` — pipe to file (though no shell interpretation mitigates this)
- `journalctl -u sshd` — read SSH logs

The fork/exec model (no shell interpretation) prevents pipe/redirect injection, which is good. But arbitrary file path access via whitelisted read commands is still a concern.
Evidence: `is_exec_allowed("cat")` returns `true`, `execute_command("cat", &["/etc/shadow"])` would execute.
Suggested resolution: Consider path restrictions (e.g., deny access to `/etc/shadow`, `/root`, private key directories). Or implement a file path allowlist per vCluster policy.

---

## Finding: F6 — Shell environment clears variables but sets HOME=/tmp
Severity: Low
Category: Security > Configuration
Location: `crates/pact-agent/src/shell/exec.rs:81`
Spec reference: None
Description: `execute_command()` sets `HOME=/tmp`. Any command that writes to `$HOME` (e.g., `nvidia-smi --query-gpu=... --filename=$HOME/output.csv`) would write to /tmp, which is world-readable. This is minor because exec commands are short-lived and the output is streamed, but configuration-writing commands could leave artifacts.
Evidence: Line 81.
Suggested resolution: Use a per-session temporary directory instead of /tmp.

---

## Finding: F7 — JWKS cache has no maximum key count
Severity: Low
Category: Robustness > Resource exhaustion
Location: `crates/pact-agent/src/shell/auth.rs:129-131`
Spec reference: None
Description: `JwksCache` stores all keys from the JWKS endpoint without limit. A malicious or misconfigured IdP returning thousands of keys could consume memory.
Evidence: `set_keys()` and `fetch()` accept unbounded `Vec<Jwk>`.
Suggested resolution: Cap at a reasonable maximum (e.g., 100 keys). Log warning if exceeded.

---

## Finding: F8 — Token validation trusts `pact_role` claim without cross-referencing IdP groups
Severity: Medium
Category: Security > Authentication & authorization
Location: `crates/pact-agent/src/shell/auth.rs:355-367`
Spec reference: OIDC role model in CLAUDE.md
Description: `claims_to_identity()` directly uses the `pact_role` custom claim from the JWT. If an IdP is compromised or misconfigured, an attacker can set `pact_role: "pact-platform-admin"` in the token and gain full access. The code has a `groups` field in the TokenClaims (pact-policy/src/iam/mod.rs:45) but it's never used for role derivation — the role comes directly from the token claim.
Evidence: `claims_to_identity()` reads `claims.pact_role` directly. No group→role mapping or cross-validation.
Suggested resolution: Consider deriving roles from IdP group membership (`groups` claim) rather than trusting an arbitrary custom claim. Or validate that `pact_role` matches expected patterns for the IdP configuration.

---

## Summary

| Severity | Count |
|----------|-------|
| Critical | 0 |
| High | 1 (F5: no argument validation) |
| Medium | 3 (F1: sysctl, F2: ip, F3: mount, F8: role trust) |
| Low | 3 (F4: PATH, F6: HOME, F7: JWKS cache) |
| **Total** | **7** |

Highest-risk finding: **F5 (no argument validation)** — whitelisted commands can read arbitrary files. Combined with F8 (role claim trust), a compromised IdP token with viewer role could read sensitive system files via `cat /etc/shadow` through pact exec.
