# Adversarial Sweep Plan

Status: COMPLETE
Started: 2026-03-20

## Attack surface

| Surface | Entry points | Trust level | Fidelity |
|---------|-------------|-------------|----------|
| Shell/exec gRPC | ShellService.Exec, ShellService.Shell | Authenticated (OIDC) | MODERATE |
| Enrollment gRPC | EnrollmentService.Enroll, RegisterNode | Server-TLS-only (pre-auth) | HIGH |
| Config gRPC | ConfigService.AppendEntry, StreamBootConfig | Authenticated (mTLS) | HIGH |
| Policy gRPC | PolicyService.Evaluate | Internal (journal process) | HIGH |
| CLI args | clap-parsed user input | Local user | LOW |
| Metrics HTTP | GET /metrics, GET /health | Unauthenticated | LOW |
| MCP tools | dispatch_tool() | Machine identity (AI agent) | LOW |
| File I/O | /proc, /sys, tmpfs, config files | Local system | — |
| Unsafe code | shell/session.rs (PTY fork), storage.rs (statvfs) | Kernel boundary | — |
| Unix sockets | SPIRE workload API, lattice handoff | Local IPC | HIGH (workload_integration) |
| Dependencies | 36 direct (pact-agent) | Third-party | — |

## Chunks (ordered by exposure)

| # | Scope | Attack vectors | Status | Session |
|---|-------|---------------|--------|---------|
| 1 | Shell/exec: whitelist bypass, command injection, auth bypass | Security, Correctness | DONE | 2026-03-20 (8 findings: 1H 4M 3L) |
| 2 | Enrollment: identity spoofing, rate limit bypass, cert lifecycle | Security, Correctness | DONE | 2026-03-20 (7 findings: 2C 1H 3M 1L) |
| 3 | RBAC/policy: privilege escalation, scope bypass, degraded mode | Security, Correctness | DONE | 2026-03-20 (4 findings: 0C 0H 3M 1L) |
| 4 | Journal state machine: invariant violations, concurrency, immutability | Correctness, Robustness | DONE | 2026-03-20 (4 findings: 0C 0H 3M 1L) |
| 5 | Unsafe code + trust boundaries: PTY fork, statvfs, file permissions | Security | DONE | 2026-03-20 (0 findings — clean) |
| 6 | Commit window + conflict resolution: race conditions, timer abuse | Correctness, Robustness | DONE | 2026-03-20 (2 findings: 0C 0H 0M 2L) |
| 7 | Supply chain + resource exhaustion + observability gaps | Security, Robustness | DONE | 2026-03-20 (3 findings: 0C 0H 2M 1L) |
