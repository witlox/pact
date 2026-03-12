# Journal Data Models (pact-journal)

Runtime state managed by the journal's Raft state machine.

---

## JournalState (Raft State Machine)

```rust
pub struct JournalState {
    /// Config entries indexed by sequence. BTreeMap for ordered range queries.
    pub entries: BTreeMap<EntrySeq, ConfigEntry>,
    /// Next sequence number to assign. Invariant J1: monotonic, no gaps.
    pub next_sequence: EntrySeq,
    /// Per-node current config state.
    pub node_states: HashMap<NodeId, ConfigState>,
    /// Per-vCluster active policy. Invariant J6: at most one per vCluster.
    pub policies: HashMap<VClusterId, VClusterPolicy>,
    /// Pre-computed boot overlays per vCluster.
    pub overlays: HashMap<VClusterId, BootOverlay>,
    /// Admin operation audit log. Invariant O3: never interrupted.
    pub audit_log: Vec<AdminOperation>,
    /// Node-to-vCluster assignment mapping.
    pub node_assignments: HashMap<NodeId, VClusterId>,
}
```

## JournalCommand (Raft Commands)

All state mutations go through Raft consensus (invariant J7).

```rust
pub enum JournalCommand {
    /// Append config entry. Validates: J3 (non-empty author), J4 (acyclic parent).
    AppendEntry(ConfigEntry),
    /// Update node config state.
    UpdateNodeState { node_id: NodeId, state: ConfigState },
    /// Set/replace vCluster policy. Invariant J6.
    SetPolicy { vcluster_id: VClusterId, policy: VClusterPolicy },
    /// Store pre-computed boot overlay. Validates: J5 (checksum matches data).
    SetOverlay { vcluster_id: VClusterId, overlay: BootOverlay },
    /// Record admin operation in audit log.
    RecordOperation(AdminOperation),
    /// Assign node to vCluster.
    AssignNode { node_id: NodeId, vcluster_id: VClusterId },
}
```

## Validation Rules (enforced in apply())

| Command | Validation | Invariant |
|---------|-----------|-----------|
| AppendEntry | `author.principal` non-empty | J3 |
| AppendEntry | `author.role` non-empty | J3 |
| AppendEntry | `parent.is_none() \|\| parent < next_sequence` | J4 |
| SetOverlay | `checksum == hash(data)` | J5 |

## JournalResponse

```rust
pub enum JournalResponse {
    Ok,
    EntryAppended { sequence: u64 },
    ValidationError { reason: String },
}
```

## Overlay Cache

```rust
/// Overlay staleness: compare overlay version vs latest config sequence for vCluster.
/// Stale overlays trigger on-demand rebuild (failure-modes.md: F9).
pub struct OverlayCache {
    overlays: HashMap<VClusterId, BootOverlay>,
}

impl OverlayCache {
    /// Check if overlay is current for the given config sequence.
    fn is_stale(&self, vcluster_id: &VClusterId, latest_seq: EntrySeq) -> bool;
    /// Build overlay from entries. zstd compressed, checksummed.
    fn rebuild(&mut self, vcluster_id: &VClusterId, entries: &BTreeMap<EntrySeq, ConfigEntry>);
}
```
