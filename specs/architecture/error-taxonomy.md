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

    // --- Conflict errors ---
    MergeConflict { node: String, keys: Vec<String> },
    TtlOutOfBounds { value: u32, min: u32, max: u32 },
    PromoteConflict { node: String, conflicting_nodes: Vec<String>, keys: Vec<String> },

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
| MergeConflict | FAILED_PRECONDITION | Local changes conflict with journal state on reconnect |
| TtlOutOfBounds | INVALID_ARGUMENT | TTL value outside allowed range [900, 864000] seconds |
| PromoteConflict | FAILED_PRECONDITION | Promote blocked — target nodes have conflicting local changes |
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
| Merge conflict | 7 | MergeConflict | "Merge conflict on node {node}: keys {keys} differ from journal" |
| TTL out of bounds | 8 | TtlOutOfBounds | "TTL {value}s out of range [{min}, {max}]" |
| Promote conflict | 9 | PromoteConflict | "Promote blocked: nodes {nodes} have local changes on {keys}" |
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
| TTL below minimum | ND1 | `ttl > 0 && ttl < 900` | `ValidationError { reason: "TTL must be >= 900 seconds (15 minutes)" }` |
| TTL above maximum | ND2 | `ttl > 864000` | `ValidationError { reason: "TTL must be <= 864000 seconds (10 days)" }` |

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

---

## AuthError (hpc-auth)

Authentication errors from the shared auth library. Consumed by CLI commands.

| Variant | Meaning | CLI Exit Code | User Message |
|---------|---------|---------------|--------------|
| `IdpUnreachable(String)` | Cannot reach OIDC provider (F15) | 2 | "Cannot reach identity provider at {url}. Use BMC console for emergency access." |
| `NoSupportedFlow` | IdP discovery lists no compatible grant types | 2 | "No compatible authentication flow. Check IdP configuration." |
| `TokenExpired` | Access token expired, no refresh possible | 2 | "Session expired. Run `pact login` to authenticate." |
| `CacheCorrupted(String)` | Token cache file is invalid (F16) | 2 | "Token cache is corrupted. Run `pact login` to re-authenticate." |
| `CachePermissionDenied(String)` | Cache file has wrong permissions (strict mode) | 2 | "Token cache has incorrect permissions ({actual}). Expected 0600. Run `pact login`." |
| `OAuthFailed(String)` | OAuth2 exchange failed | 2 | "Authentication failed: {reason}" |
| `Timeout` | User didn't complete auth in time | 2 | "Authentication timed out. Try `--device-code` for headless environments." |
| `StaleDiscovery` | Cached discovery document is outdated (F17) | 2 | "IdP configuration may have changed. Retry when IdP is reachable." |

### Error Flow: hpc-auth → CLI

```
AuthClient::get_token() → Err(AuthError::TokenExpired)
  → CLI prints "Session expired. Run `pact login`."
  → CLI exits with code 2
```

### Never Panic

All crates use `Result<T, PactError>` or `Result<T, tonic::Status>`. Panics are bugs. The only acceptable panic is in test code (`unwrap()` in tests is fine).
