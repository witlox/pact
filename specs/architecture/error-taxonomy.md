# Error Taxonomy

Per-module error types, their semantics, and how they map to gRPC status codes and user-facing messages.

---

## PactError (pact-common)

The shared error type used across all crates. Defined via `thiserror` with structured variants.

```rust
pub enum PactError {
    // --- Lookup errors ---
    NodeNotFound(String),
    VClusterNotFound(String),
    EntryNotFound(u64),

    // --- State errors ---
    CommitWindowExpired { node: String, magnitude: f64 },
    EmergencyActive(String),

    // --- Auth/Policy errors ---
    Unauthorized { reason: String },
    PolicyError(String),

    // --- Infrastructure errors ---
    JournalUnavailable(String),
    ShellError(String),
    ServiceFailed { node: String, service: String, reason: String },

    // --- Drift errors ---
    DriftDetected { node: String, detail: String },

    // --- Serialization/Transport ---
    Serialization(String),
    Transport(tonic::Status),     // #[from]

    // --- Catch-all ---
    Internal(String),
}
```

### Error → gRPC Status Mapping

| PactError Variant | gRPC Status Code | When |
|-------------------|-----------------|------|
| NodeNotFound | NOT_FOUND | Node ID doesn't exist in journal |
| VClusterNotFound | NOT_FOUND | vCluster ID doesn't exist |
| EntryNotFound | NOT_FOUND | Sequence number not in log |
| CommitWindowExpired | FAILED_PRECONDITION | Commit attempted after window expiry |
| EmergencyActive | FAILED_PRECONDITION | Operation blocked by active emergency |
| Unauthorized | PERMISSION_DENIED | OIDC token invalid, role insufficient, or policy denies |
| PolicyError | INTERNAL | Policy engine failure (OPA unreachable without fallback) |
| JournalUnavailable | UNAVAILABLE | Raft quorum lost or journal unreachable |
| ShellError | INTERNAL | Shell session setup/teardown failure |
| ServiceFailed | INTERNAL | Service start/stop/health failure |
| DriftDetected | OK (informational) | Not an error per se — returned as event data |
| Serialization | INTERNAL | Protobuf or serde encoding failure |
| Transport | (preserved) | Underlying tonic status passed through |
| Internal | INTERNAL | Unexpected internal failure |

### Error → CLI Exit Code Mapping

| Scenario | Exit Code | PactError | User Message |
|----------|-----------|-----------|-------------|
| Success | 0 | — | — |
| Auth failure | 1 | Unauthorized | "Access denied: {reason}" |
| Not found | 2 | *NotFound variants | "Not found: {entity}" |
| Precondition | 3 | CommitWindowExpired, EmergencyActive | "Cannot proceed: {detail}" |
| Journal down | 5 | JournalUnavailable | "Journal unavailable — retry or check cluster health" |
| Rollback blocked | 10 | Internal (active consumers) | "Rollback blocked: active consumers on affected resources" |
| Internal | 99 | Internal, Serialization | "Internal error: {detail}" |

---

## Journal-Specific Errors

Errors produced by the Raft state machine's `apply()` method:

```rust
pub enum JournalResponse {
    Ok,
    EntryAppended { sequence: u64 },
    ValidationError { reason: String },
}
```

### Validation Errors (apply-time)

| Validation | Invariant | Trigger | JournalResponse |
|-----------|-----------|---------|-----------------|
| Empty author principal | J3 | `author.principal.is_empty()` | `ValidationError { reason: "author principal required" }` |
| Empty author role | J3 | `author.role.is_empty()` | `ValidationError { reason: "author role required" }` |
| Cyclic parent | J4 | `parent >= next_sequence` | `ValidationError { reason: "parent must precede entry" }` |
| Checksum mismatch | J5 | `checksum != hash(data)` | `ValidationError { reason: "overlay checksum mismatch" }` |

These are deterministic — same input always produces same validation result on every Raft replica.

---

## Policy-Specific Errors

### PolicyDecision (not an error, but a result)

```rust
pub enum PolicyDecision {
    Allow { policy_ref: String },
    Deny { policy_ref: String, reason: String },
    RequireApproval { policy_ref: String, approval_id: String },
}
```

`Deny` is not a system error — it's a correct policy evaluation result. Maps to `PERMISSION_DENIED` at the gRPC boundary.

### Policy Engine Failures

| Failure | Invariant | Behavior |
|---------|-----------|----------|
| OIDC token expired | P1 | `Unauthorized { reason: "token expired" }` |
| OIDC wrong audience | P1 | `Unauthorized { reason: "invalid audience" }` |
| OIDC signature invalid | P1 | `Unauthorized { reason: "invalid token signature" }` |
| OPA sidecar unreachable | P7, F7 | Fall back to cached VClusterPolicy. Log degraded mode to Loki. |
| OPA timeout (>1s) | — | Treat as unreachable, same fallback |
| JWKS rotation failure | — | Continue with cached JWKS. Alert. |

---

## Agent-Specific Errors

### Commit Window Errors

| Error | Invariant | Recovery |
|-------|-----------|----------|
| Window already open | A1 | Extend existing window with new drift |
| Window expired | A4 | Auto-rollback (unless emergency active) |
| Rollback blocked | A5 | Active consumers prevent rollback — alert admin |
| Journal unreachable for commit | F1 | Cache pending entry, retry on reconnect (A9) |

### Service Manager Errors

| Error | Invariant | Recovery |
|-------|-----------|----------|
| Start failure | A6 | Log ServiceFailed, respect restart policy |
| Restart limit exceeded | — | Mark service as failed, alert |
| Dependency not running | A6 | Wait or fail depending on dependency type |
| Stop timeout (SIGTERM) | — | Escalate to SIGKILL after grace period |

### Observer Errors

| Error | Recovery |
|-------|----------|
| eBPF probe attach failure | Fall back to inotify/netlink (reduced coverage) |
| inotify watch limit | Log warning, prioritize critical paths |
| Netlink socket error | Retry with backoff |

---

## Degraded Mode Error Semantics

When operating in degraded mode (journal partition, OPA unreachable), error handling changes:

| Normal Error | Degraded Behavior | Invariant |
|-------------|-------------------|-----------|
| JournalUnavailable | Cache entries, replay on reconnect | A9 |
| PolicyError (OPA) | Use cached VClusterPolicy | P7 |
| Two-person approval needed | Deny (fail-closed) | P7 |
| Complex OPA rule needed | Deny (fail-closed) | P7 |
| Platform admin auth | Authorized from cached role | P7 |
| Whitelist check | Honored from cached policy | P7 |

---

## Error Propagation Rules

1. **Journal → Agent**: gRPC Status codes. Agent maps to PactError for local handling.
2. **Agent → CLI**: gRPC Status codes. CLI maps to exit codes + human messages.
3. **Policy → Journal**: PactError within process (library crate). Journal maps to gRPC for external callers.
4. **Observer → Agent**: `mpsc` channel errors. If channel closed, observer stops gracefully.
5. **Raft → Journal**: openraft errors mapped to JournalResponse or PactError::JournalUnavailable.

### Never Panic

All crates use `Result<T, PactError>` or `Result<T, tonic::Status>`. Panics are bugs. The only acceptable panic is in test code (`unwrap()` in tests is fine).
