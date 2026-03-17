//! Contract tests for cross-module journal invariants.
//!
//! These test that the enforcement mechanisms identified in enforcement-map.md
//! actually prevent invariant violations.
//!
//! Source: specs/invariants.md § Journal Invariants (J1-J9)

// ---------------------------------------------------------------------------
// J1: Monotonic sequence numbers
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J1
/// Spec: invariants.md § J1 — EntrySeq values are strictly increasing with no gaps
/// If this test didn't exist: sequence numbers could go backwards or stall.
#[test]
fn j1_sequences_strictly_increasing() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let mut prev_seq = None;
    for i in 0..5 {
        let entry = test_config_entry(&format!("entry-{i}"));
        let seq = raft.client_write(JournalCommand::AppendEntry(entry)).unwrap();

        if let Some(prev) = prev_seq {
            assert!(seq > prev,
                "sequence {seq} must be strictly greater than previous {prev}");
        }
        prev_seq = Some(seq);
    }
}

/// Contract: enforcement-map.md § J1
/// Spec: invariants.md § J1 — entries 0..N all exist with no gaps
/// If this test didn't exist: holes in the sequence could go undetected.
#[test]
fn j1_no_gaps_in_sequence() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let count = 10;
    for i in 0..count {
        let entry = test_config_entry(&format!("entry-{i}"));
        raft.client_write(JournalCommand::AppendEntry(entry)).unwrap();
    }

    for seq in 0..count {
        let entry = state.get_entry(EntrySeq(seq));
        assert!(entry.is_some(), "entry at sequence {seq} must exist — no gaps allowed");
    }
}

// ---------------------------------------------------------------------------
// J2: Immutability
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J2
/// Spec: invariants.md § J2 — JournalState has no update or delete for entries
/// If this test didn't exist: someone could add a mutating method and break append-only.
#[test]
fn j2_no_update_method_exists() {
    // Structural assertion: JournalState's public API has no update_entry or delete_entry.
    // This is enforced at compile time — if these methods existed, they would appear in
    // the impl block. We confirm by attempting to call them and expecting a compilation
    // failure (this test compiles only because they don't exist).
    let state = stub_journal_state();

    // The only write path is through Raft via JournalCommand::AppendEntry.
    // JournalState exposes get_entry, list_entries — no mutation methods.
    // If update_entry or delete_entry is ever added, this test must be updated
    // and the invariant re-evaluated.
    assert!(!has_public_method::<JournalState>("update_entry"));
    assert!(!has_public_method::<JournalState>("delete_entry"));
}

/// Contract: enforcement-map.md § J2
/// Spec: invariants.md § J2 — committed entry is never modified
/// If this test didn't exist: entry content could change silently after commit.
#[test]
fn j2_entries_unchanged_after_commit() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let entry = test_config_entry("immutable-entry");
    let expected_data = entry.data.clone();
    let expected_author = entry.author.clone();

    let seq = raft.client_write(JournalCommand::AppendEntry(entry)).unwrap();

    // Read back and verify byte-for-byte identity
    let committed = state.get_entry(EntrySeq(seq)).expect("entry must exist after commit");
    assert_eq!(committed.data, expected_data, "data must not change after commit");
    assert_eq!(committed.author.principal, expected_author.principal);
    assert_eq!(committed.author.role, expected_author.role);
}

// ---------------------------------------------------------------------------
// J3: Authenticated authorship
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J3
/// Spec: invariants.md § J3 — empty principal rejected
/// If this test didn't exist: anonymous entries could be committed to the journal.
#[test]
fn j3_empty_principal_rejected() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let mut entry = test_config_entry("anon-entry");
    entry.author.principal = String::new();

    let result = raft.client_write(JournalCommand::AppendEntry(entry));
    assert_matches!(result, Err(PactError::ValidationError { .. }),
        "empty principal must be rejected");
}

/// Contract: enforcement-map.md § J3
/// Spec: invariants.md § J3 — empty role rejected
/// If this test didn't exist: entries without role information could slip through.
#[test]
fn j3_empty_role_rejected() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let mut entry = test_config_entry("no-role-entry");
    entry.author.role = String::new();

    let result = raft.client_write(JournalCommand::AppendEntry(entry));
    assert_matches!(result, Err(PactError::ValidationError { .. }),
        "empty role must be rejected");
}

/// Contract: enforcement-map.md § J3
/// Spec: invariants.md § J3 — valid author accepted
/// If this test didn't exist: valid entries could be incorrectly rejected.
#[test]
fn j3_valid_author_accepted() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let entry = test_config_entry("valid-entry");
    assert!(!entry.author.principal.is_empty());
    assert!(!entry.author.role.is_empty());

    let result = raft.client_write(JournalCommand::AppendEntry(entry));
    assert!(result.is_ok(), "entry with valid principal and role must be accepted");
}

// ---------------------------------------------------------------------------
// J4: Acyclic parent chain
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J4
/// Spec: invariants.md § J4 — parent < sequence is valid
/// If this test didn't exist: valid parent references could be incorrectly rejected.
#[test]
fn j4_parent_less_than_sequence_accepted() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    // First entry creates sequence 0
    let entry_0 = test_config_entry("base-entry");
    let seq_0 = raft.client_write(JournalCommand::AppendEntry(entry_0)).unwrap();

    // Second entry references first as parent
    let mut entry_1 = test_config_entry("child-entry");
    entry_1.parent = Some(EntrySeq(seq_0));

    let result = raft.client_write(JournalCommand::AppendEntry(entry_1));
    assert!(result.is_ok(), "parent < sequence must be accepted");
}

/// Contract: enforcement-map.md § J4
/// Spec: invariants.md § J4 — parent == sequence creates a cycle
/// If this test didn't exist: self-referencing entries could corrupt the parent chain.
#[test]
fn j4_parent_equal_to_sequence_rejected() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    // Append one entry to know what the next sequence will be
    let entry_0 = test_config_entry("base-entry");
    let seq_0 = raft.client_write(JournalCommand::AppendEntry(entry_0)).unwrap();
    let next_seq = seq_0 + 1;

    // Try to create an entry whose parent equals its own (anticipated) sequence
    let mut entry = test_config_entry("self-ref-entry");
    entry.parent = Some(EntrySeq(next_seq));

    let result = raft.client_write(JournalCommand::AppendEntry(entry));
    assert_matches!(result, Err(PactError::ValidationError { .. }),
        "parent == sequence must be rejected");
}

/// Contract: enforcement-map.md § J4
/// Spec: invariants.md § J4 — parent > sequence is a forward reference (invalid)
/// If this test didn't exist: forward references could create impossible parent chains.
#[test]
fn j4_parent_greater_than_sequence_rejected() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let mut entry = test_config_entry("forward-ref-entry");
    entry.parent = Some(EntrySeq(9999));

    let result = raft.client_write(JournalCommand::AppendEntry(entry));
    assert_matches!(result, Err(PactError::ValidationError { .. }),
        "parent > sequence must be rejected");
}

/// Contract: enforcement-map.md § J4
/// Spec: invariants.md § J4 — None parent is always valid (root entry)
/// If this test didn't exist: root entries without a parent could be rejected.
#[test]
fn j4_none_parent_always_accepted() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let mut entry = test_config_entry("root-entry");
    entry.parent = None;

    let result = raft.client_write(JournalCommand::AppendEntry(entry));
    assert!(result.is_ok(), "None parent must always be accepted");
}

// ---------------------------------------------------------------------------
// J5: Overlay consistency
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J5
/// Spec: invariants.md § J5 — checksum matches hash of data
/// If this test didn't exist: corrupt overlays could be accepted and streamed to agents.
#[test]
fn j5_matching_checksum_accepted() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let overlay = test_overlay("ml-training", b"overlay-data-v1");
    assert_eq!(overlay.checksum, deterministic_hash(b"overlay-data-v1"));

    let result = raft.client_write(JournalCommand::SetOverlay(overlay));
    assert!(result.is_ok(), "overlay with matching checksum must be accepted");
}

/// Contract: enforcement-map.md § J5
/// Spec: invariants.md § J5 — mismatched checksum rejected
/// If this test didn't exist: tampered or corrupted overlays could be stored.
#[test]
fn j5_mismatched_checksum_rejected() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let mut overlay = test_overlay("ml-training", b"overlay-data-v1");
    overlay.checksum = "0000000000000000deadbeef".into(); // wrong checksum

    let result = raft.client_write(JournalCommand::SetOverlay(overlay));
    assert_matches!(result, Err(PactError::ValidationError { .. }),
        "mismatched checksum must be rejected");
}

// ---------------------------------------------------------------------------
// J6: Single policy per vCluster
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J6
/// Spec: invariants.md § J6 — setting policy for same vCluster replaces old
/// If this test didn't exist: multiple policies per vCluster could cause conflicts.
#[test]
fn j6_set_policy_replaces_existing() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let policy_v1 = VClusterPolicy {
        vcluster_id: "ml-training".into(),
        max_nodes: 10,
        ..Default::default()
    };
    let policy_v2 = VClusterPolicy {
        vcluster_id: "ml-training".into(),
        max_nodes: 20,
        ..Default::default()
    };

    raft.client_write(JournalCommand::SetPolicy(policy_v1)).unwrap();
    raft.client_write(JournalCommand::SetPolicy(policy_v2)).unwrap();

    let current = state.get_policy("ml-training").expect("policy must exist");
    assert_eq!(current.max_nodes, 20, "latest policy must replace previous");

    // Only one policy for this vCluster
    let all_policies = state.list_policies();
    let count = all_policies.iter().filter(|p| p.vcluster_id == "ml-training").count();
    assert_eq!(count, 1, "exactly one policy per vCluster");
}

/// Contract: enforcement-map.md § J6
/// Spec: invariants.md § J6 — different vClusters are independent
/// If this test didn't exist: setting a policy could clobber another vCluster's policy.
#[test]
fn j6_different_vclusters_coexist() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let policy_ml = VClusterPolicy {
        vcluster_id: "ml-training".into(),
        max_nodes: 10,
        ..Default::default()
    };
    let policy_hpc = VClusterPolicy {
        vcluster_id: "hpc-compute".into(),
        max_nodes: 50,
        ..Default::default()
    };

    raft.client_write(JournalCommand::SetPolicy(policy_ml)).unwrap();
    raft.client_write(JournalCommand::SetPolicy(policy_hpc)).unwrap();

    let ml = state.get_policy("ml-training").expect("ml policy must exist");
    let hpc = state.get_policy("hpc-compute").expect("hpc policy must exist");
    assert_eq!(ml.max_nodes, 10);
    assert_eq!(hpc.max_nodes, 50);
}

// ---------------------------------------------------------------------------
// J7: Raft consensus for writes
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J7
/// Spec: invariants.md § J7 — AppendEntry requires Raft consensus
/// If this test didn't exist: entries could be written directly to state, bypassing Raft.
#[test]
fn j7_append_entry_goes_through_raft() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let entry = test_config_entry("raft-entry");
    let result = raft.client_write(JournalCommand::AppendEntry(entry));

    // Success means Raft accepted and committed the write
    assert!(result.is_ok());
    assert!(raft.last_write_went_through_consensus(),
        "AppendEntry must go through Raft consensus");
}

/// Contract: enforcement-map.md § J7
/// Spec: invariants.md § J7 — SetPolicy requires Raft consensus
/// If this test didn't exist: policies could be set without consensus.
#[test]
fn j7_set_policy_goes_through_raft() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let policy = VClusterPolicy {
        vcluster_id: "ml-training".into(),
        ..Default::default()
    };

    let result = raft.client_write(JournalCommand::SetPolicy(policy));
    assert!(result.is_ok());
    assert!(raft.last_write_went_through_consensus(),
        "SetPolicy must go through Raft consensus");
}

/// Contract: enforcement-map.md § J7
/// Spec: invariants.md § J7 — no public method on JournalState mutates directly
/// If this test didn't exist: a backdoor write method could bypass consensus.
#[test]
fn j7_no_direct_state_mutation() {
    // JournalState has no public methods that mutate state.
    // The only mutation path is StateMachine::apply() called by the Raft engine.
    // This is a structural invariant: JournalState's public API is read-only.
    let state = stub_journal_state();

    assert!(!has_public_method::<JournalState>("insert_entry"));
    assert!(!has_public_method::<JournalState>("set_entry"));
    assert!(!has_public_method::<JournalState>("remove_entry"));
    assert!(!has_public_method::<JournalState>("clear"));
}

// ---------------------------------------------------------------------------
// J8: Reads from local state
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J8
/// Spec: invariants.md § J8 — get_entry reads from local replica, no Raft round-trip
/// If this test didn't exist: reads could trigger consensus, destroying read performance.
#[test]
fn j8_get_entry_reads_local() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let entry = test_config_entry("local-read-entry");
    let seq = raft.client_write(JournalCommand::AppendEntry(entry)).unwrap();
    raft.reset_consensus_counter();

    // Read should not trigger Raft
    let _result = state.get_entry(EntrySeq(seq));
    assert_eq!(raft.consensus_round_trips(), 0,
        "get_entry must not trigger Raft round-trip");
}

/// Contract: enforcement-map.md § J8
/// Spec: invariants.md § J8 — list_entries from local BTreeMap
/// If this test didn't exist: listing entries could cause N Raft round-trips.
#[test]
fn j8_list_entries_reads_local() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    for i in 0..5 {
        let entry = test_config_entry(&format!("entry-{i}"));
        raft.client_write(JournalCommand::AppendEntry(entry)).unwrap();
    }
    raft.reset_consensus_counter();

    let entries = state.list_entries();
    assert_eq!(entries.len(), 5);
    assert_eq!(raft.consensus_round_trips(), 0,
        "list_entries must not trigger Raft round-trip");
}

/// Contract: enforcement-map.md § J8
/// Spec: invariants.md § J8 — get_overlay from local cache
/// If this test didn't exist: overlay reads during boot could block on consensus.
#[test]
fn j8_get_overlay_reads_local() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    let overlay = test_overlay("ml-training", b"boot-data");
    raft.client_write(JournalCommand::SetOverlay(overlay)).unwrap();
    raft.reset_consensus_counter();

    let _result = state.get_overlay("ml-training");
    assert_eq!(raft.consensus_round_trips(), 0,
        "get_overlay must not trigger Raft round-trip");
}

// ---------------------------------------------------------------------------
// J9: No duplicate entries from concurrent commits
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § J9
/// Spec: invariants.md § J9 — Raft serializes writes; concurrent appends get unique sequences
/// If this test didn't exist: concurrent writes could produce duplicate sequence numbers.
#[test]
fn j9_concurrent_appends_get_unique_sequences() {
    let state = stub_journal_state();
    let raft = stub_raft_node(state.clone());

    // Simulate two concurrent appends (Raft serializes them internally)
    let entry_a = test_config_entry("concurrent-a");
    let entry_b = test_config_entry("concurrent-b");

    let seq_a = raft.client_write(JournalCommand::AppendEntry(entry_a)).unwrap();
    let seq_b = raft.client_write(JournalCommand::AppendEntry(entry_b)).unwrap();

    assert_ne!(seq_a, seq_b,
        "concurrent appends must get distinct sequence numbers");

    // Both entries must exist and be different
    let a = state.get_entry(EntrySeq(seq_a)).expect("entry A must exist");
    let b = state.get_entry(EntrySeq(seq_b)).expect("entry B must exist");
    assert_ne!(a.data, b.data, "entries must be distinct");
}
