# Chunk 5: Unsafe Code + Trust Boundaries

Reviewed: 2026-03-20
Files: shell/session.rs, capability/storage.rs

---

## Finding: F26 — PTY slave fd leaked to parent if spawn fails after pre_exec setup
Severity: Low
Category: Robustness > Error handling
Location: `crates/pact-agent/src/shell/session.rs:447-457`
Spec reference: None
Description: If `cmd.spawn()` fails (line 449-454), the slave fd from `openpty()` may not be properly cleaned up. The `drop(pty.slave)` at line 457 only runs on the success path. However, Rust's drop semantics will clean up `pty.slave` when the `openpty` result goes out of scope, so this is actually safe — the `OwnedFd` will be dropped automatically.
Evidence: Reviewed — Rust's ownership model handles this correctly.
Suggested resolution: None needed. Rust drop semantics handle fd cleanup.

**Verdict: Not a finding** — Rust's ownership model prevents the leak.

---

## Finding: F27 — statvfs called on user-controlled mount paths without sanitization
Severity: Low
Category: Security > Input validation
Location: `crates/pact-agent/src/capability/storage.rs` (statvfs function)
Spec reference: None
Description: `statvfs_with_timeout()` calls `libc::statvfs()` on paths read from `/proc/mounts`. These paths are system-controlled (kernel mount table), not user input. However, if mount point paths contain unusual characters (e.g., spaces, newlines), the `CString::new()` conversion would fail gracefully (returning 0 for both sizes).
Evidence: Input is from `/proc/mounts`, not from user.
Suggested resolution: None needed — paths are kernel-controlled.

**Verdict: Not a finding** — paths are system-controlled.

---

## Summary

| Severity | Count |
|----------|-------|
| Findings | 0 |

The unsafe code in this codebase is minimal, well-documented with SAFETY comments, and limited to necessary kernel interactions (PTY ioctl, statvfs). The `pre_exec` closure uses only async-signal-safe functions as documented. The workspace-level `deny(unsafe_code)` ensures unsafe is explicitly opted-in per use site.
