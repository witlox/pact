# Journal Interfaces

gRPC service interfaces hosted by pact-journal. Internal trait interfaces for Raft state machine.

---

## ConfigService (journal.proto)

Handles config entry CRUD and overlay retrieval. Writes go through Raft, reads from local state.

```rust
#[tonic::async_trait]
impl ConfigService for JournalServer {
    /// Write config entry through Raft consensus.
    /// Validates: J3 (non-empty author), J4 (acyclic parent).
    /// Returns: sequence number of appended entry.
    /// Error: ValidationError if invariants violated, RaftError if quorum unavailable.
    async fn append_entry(&self, request: AppendEntryRequest)
        -> Result<AppendEntryResponse, Status>;

    /// Read single entry by sequence. From local state (no Raft). Invariant J8.
    async fn get_entry(&self, request: GetEntryRequest)
        -> Result<ConfigEntry, Status>;

    /// Read node config state. From local state.
    async fn get_node_state(&self, request: GetNodeStateRequest)
        -> Result<NodeStateResponse, Status>;

    /// Stream entries matching scope and range. BTreeMap range query. Invariant J8.
    /// Used by: pact log, pact diff, boot streaming
    type ListEntriesStream: Stream<Item = Result<ConfigEntry, Status>>;
    async fn list_entries(&self, request: ListEntriesRequest)
        -> Result<Response<Self::ListEntriesStream>, Status>;

    /// Read cached overlay. From local state.
    async fn get_overlay(&self, request: GetOverlayRequest)
        -> Result<OverlayResponse, Status>;
}
```

**Contract:**
- `append_entry` is the ONLY write path — all state mutations go through Raft (J7)
- `list_entries` returns entries ordered by sequence (BTreeMap guarantees)
- All reads served from local state machine replica (J8)
- Authentication: OIDC Bearer token in gRPC metadata (P1)

## PolicyService (policy.proto)

Hosted in journal process. Delegates to pact-policy library.

```rust
#[tonic::async_trait]
impl PolicyService for JournalServer {
    /// Evaluate authorization for an operation.
    /// Delegates to pact-policy library (RBAC + OPA).
    /// Returns: authorized, policy_ref, optional denial_reason, optional approval_required.
    /// Invariants: P1 (authenticated), P2 (authorized), P4 (two-person), P6 (platform admin).
    async fn evaluate(&self, request: PolicyEvalRequest)
        -> Result<PolicyEvalResponse, Status>;

    /// Get effective policy for a vCluster. From JournalState.policies.
    async fn get_effective_policy(&self, request: GetPolicyRequest)
        -> Result<VClusterPolicy, Status>;

    /// Update policy. Writes through Raft (SetPolicy command).
    async fn update_policy(&self, request: UpdatePolicyRequest)
        -> Result<UpdatePolicyResponse, Status>;
}
```

**Contract:**
- `evaluate` calls pact-policy library, which may call OPA sidecar (I7)
- If OPA unreachable, falls back to cached VClusterPolicy (F7, P7)
- `update_policy` goes through Raft consensus
- Two-person approval: returns `approval_required` with pending ID (P4)

## BootConfigService (stream.proto)

Streams boot configuration to agents. Reads from local state only.

```rust
#[tonic::async_trait]
impl BootConfigService for JournalServer {
    /// Stream boot config: Phase 1 (overlay) + Phase 2 (node delta).
    /// Returns: ConfigChunks (zstd compressed) + ConfigComplete (version + checksum).
    /// Served from local state (J8). Scales to 10k+ concurrent streams (F11).
    type StreamBootConfigStream: Stream<Item = Result<ConfigChunk, Status>>;
    async fn stream_boot_config(&self, request: BootConfigRequest)
        -> Result<Response<Self::StreamBootConfigStream>, Status>;

    /// Subscribe to live config updates after boot.
    /// Delivers: overlay changes, node deltas, policy updates, blacklist changes.
    /// Reconnect with from_sequence on interruption.
    type SubscribeConfigUpdatesStream: Stream<Item = Result<ConfigUpdate, Status>>;
    async fn subscribe_config_updates(&self, request: SubscribeRequest)
        -> Result<Response<Self::SubscribeConfigUpdatesStream>, Status>;
}
```

**Contract:**
- Boot streaming is the hot path — must handle 10k+ concurrent connections
- Overlay served from cache; on-demand rebuild if stale (F9)
- Config updates are push-based, not polling
- `from_sequence` enables resume after partition (F3)

## Raft State Machine Trait

```rust
impl StateMachineState<JournalTypeConfig> for JournalState {
    /// Apply a command to the state machine.
    /// ALL validation happens here (J3, J4, J5).
    /// Returns JournalResponse (Ok, EntryAppended, or ValidationError).
    fn apply(&mut self, cmd: JournalCommand) -> JournalResponse;

    fn blank_response() -> JournalResponse;
}
```

## Telemetry Interface

```rust
/// Metrics + health endpoint hosted on axum (port 9091, invariant O2).
pub trait TelemetryServer {
    /// GET /health → 200 + JSON { status, role }
    async fn health(&self) -> HealthResponse;
    /// GET /metrics → Prometheus text format
    async fn metrics(&self) -> String;
}

pub struct HealthResponse {
    pub status: String,         // "healthy" | "degraded"
    pub role: String,           // "leader" | "follower" | "candidate"
}
```
