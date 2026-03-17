//! Contract tests for journal services (gRPC boundary).
//!
//! These tests verify the integration surface between:
//! - pact-agent (caller) ↔ pact-journal (ConfigService, BootConfigService)
//! - pact-cli (caller) ↔ pact-journal (ConfigService, PolicyService)
//! - pact-policy (linked) ↔ pact-journal (PolicyService, Raft state machine)
//!
//! All tests reference their source contract and spec requirement.
//! Tests use stubs — they define WHAT the interface must do,
//! not HOW it does it internally.

// ---------------------------------------------------------------------------
// ConfigService: append_entry RPC contracts
// ---------------------------------------------------------------------------

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J3 — reject entry with empty principal
/// If this test didn't exist: an anonymous actor could write config entries,
/// bypassing the audit trail requirement.
#[test]
fn append_entry_validates_non_empty_author() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();

    let entry = test_config_entry();
    let identity = Identity {
        principal: "".into(),
        principal_type: PrincipalType::Human,
        role: "pact-platform-admin".into(),
    };

    let result = config_service_append_entry(&journal, &raft, &identity, entry);
    assert_matches!(result, Err(PactError::InvalidArgument { field: "principal", .. }));
}

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J3 — reject entry with empty role
/// If this test didn't exist: an entry without a role could be appended,
/// making RBAC enforcement impossible on replay.
#[test]
fn append_entry_validates_non_empty_role() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();

    let entry = test_config_entry();
    let identity = Identity {
        principal: "admin@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "".into(),
    };

    let result = config_service_append_entry(&journal, &raft, &identity, entry);
    assert_matches!(result, Err(PactError::InvalidArgument { field: "role", .. }));
}

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J4 — reject parent >= sequence (acyclic parent reference)
/// If this test didn't exist: a forward-referencing parent could create
/// a cycle in the config DAG, breaking rollback traversal.
#[test]
fn append_entry_validates_acyclic_parent() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    let mut entry = test_config_entry();
    // Journal has 5 entries (sequences 1..5). Parent must be < next sequence (6).
    journal.seed_entries(5);
    entry.parent_sequence = Some(99); // Forward reference — invalid

    let result = config_service_append_entry(&journal, &raft, &identity, entry);
    assert_matches!(result, Err(PactError::InvalidParentReference { .. }));
}

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J1 — successful append returns EntryAppended with sequence
/// If this test didn't exist: callers wouldn't know whether their write
/// was committed or what sequence to reference in subsequent entries.
#[test]
fn append_entry_returns_sequence_number() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    let entry = test_config_entry();
    let response = config_service_append_entry(&journal, &raft, &identity, entry).unwrap();

    assert!(response.sequence > 0);
    assert!(response.timestamp > 0);
}

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J7 — all writes go through Raft consensus
/// If this test didn't exist: a write could be applied locally without
/// replication, causing split-brain config divergence across journal nodes.
#[test]
fn append_entry_goes_through_raft() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    let entry = test_config_entry();
    config_service_append_entry(&journal, &raft, &identity, entry).unwrap();

    assert_eq!(raft.proposals_received(), 1);
    assert!(raft.last_proposal_was_config_append());
}

// ---------------------------------------------------------------------------
// ConfigService: read RPC contracts
// ---------------------------------------------------------------------------

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J8 — reads don't go through Raft
/// If this test didn't exist: every read would require leader round-trip,
/// adding latency to boot config streaming and status queries.
#[test]
fn get_entry_reads_from_local_state() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    journal.seed_entries(3);
    let _entry = config_service_get_entry(&journal, &raft, &identity, 2).unwrap();

    assert_eq!(raft.proposals_received(), 0); // No Raft round-trip
}

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J1 — entries ordered by monotonic sequence
/// If this test didn't exist: list results could be unordered, breaking
/// diff/rollback which depends on causal ordering.
#[test]
fn list_entries_returns_ordered_by_sequence() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    journal.seed_entries(10);
    let entries = config_service_list_entries(&journal, &raft, &identity, "ml-training").unwrap();

    let sequences: Vec<u64> = entries.iter().map(|e| e.sequence).collect();
    assert_eq!(sequences, {
        let mut sorted = sequences.clone();
        sorted.sort();
        sorted
    });
    // Confirm BTreeMap ordering — no gaps in returned range
    for window in sequences.windows(2) {
        assert!(window[0] < window[1]);
    }
}

// ---------------------------------------------------------------------------
// ConfigService: overlay validation
// ---------------------------------------------------------------------------

/// Contract: journal-interfaces.md § ConfigService
/// Spec: J5 — SetOverlay validates checksum == hash(data)
/// If this test didn't exist: a corrupted overlay could be committed and
/// streamed to agents, causing silent boot failures.
#[test]
fn get_overlay_validates_checksum() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    let overlay_data = b"squashfs-image-bytes";
    let wrong_checksum = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    let entry = ConfigEntry {
        operation: ConfigOperation::SetOverlay {
            vcluster_id: "ml-training".into(),
            data: overlay_data.to_vec(),
            checksum: wrong_checksum.into(),
        },
        ..test_config_entry()
    };

    let result = config_service_append_entry(&journal, &raft, &identity, entry);
    assert_matches!(result, Err(PactError::ChecksumMismatch { .. }));
}

// ---------------------------------------------------------------------------
// PolicyService RPC contracts
// ---------------------------------------------------------------------------

/// Contract: journal-interfaces.md § PolicyService
/// Spec: P6 — platform admin always authorized
/// If this test didn't exist: a platform admin could be denied by an overly
/// restrictive OPA policy, locking out emergency recovery.
#[test]
fn evaluate_returns_allow_for_platform_admin() {
    let journal = stub_journal_state();
    let identity = platform_admin();

    let request = PolicyEvaluateRequest {
        principal: identity.clone(),
        action: "config.append".into(),
        resource: "vcluster:ml-training".into(),
    };

    let decision = policy_service_evaluate(&journal, request).unwrap();
    assert_eq!(decision.effect, PolicyEffect::Allow);
}

/// Contract: journal-interfaces.md § PolicyService
/// Spec: P2 — unauthorized operations denied with reason
/// If this test didn't exist: a viewer could execute write operations,
/// or denials could lack actionable context for the caller.
#[test]
fn evaluate_returns_deny_for_unauthorized() {
    let journal = stub_journal_state();

    let request = PolicyEvaluateRequest {
        principal: Identity {
            principal: "viewer@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-viewer-ml-training".into(),
        },
        action: "config.append".into(),
        resource: "vcluster:ml-training".into(),
    };

    let decision = policy_service_evaluate(&journal, request).unwrap();
    assert_eq!(decision.effect, PolicyEffect::Deny);
    assert!(!decision.reason.is_empty()); // Must explain why
}

/// Contract: journal-interfaces.md § PolicyService
/// Spec: P4 — two-person approval for regulated vClusters
/// If this test didn't exist: a single operator could make state-changing
/// operations on regulated workloads without peer review.
#[test]
fn evaluate_returns_require_approval_for_regulated() {
    let journal = stub_journal_state();

    let request = PolicyEvaluateRequest {
        principal: Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-regulated-bio-secure".into(),
        },
        action: "config.append".into(),
        resource: "vcluster:bio-secure".into(),
    };

    let decision = policy_service_evaluate(&journal, request).unwrap();
    assert_eq!(decision.effect, PolicyEffect::RequireApproval);
    assert!(decision.approvers_required >= 2);
}

/// Contract: journal-interfaces.md § PolicyService
/// Spec: F7/P7 — OPA unreachable → cached policy
/// If this test didn't exist: an OPA crash would cause all policy evaluations
/// to fail, blocking every admin operation cluster-wide.
#[test]
fn evaluate_falls_back_to_cached_on_opa_failure() {
    let journal = stub_journal_state();
    journal.set_opa_reachable(false);

    let request = PolicyEvaluateRequest {
        principal: platform_admin(),
        action: "config.append".into(),
        resource: "vcluster:ml-training".into(),
    };

    let decision = policy_service_evaluate(&journal, request).unwrap();
    // Cached whitelist should still allow platform admin
    assert_eq!(decision.effect, PolicyEffect::Allow);
    assert!(decision.from_cache);
}

/// Contract: journal-interfaces.md § PolicyService
/// Spec: J7 — policy updates via Raft
/// If this test didn't exist: a policy update on one journal node might not
/// replicate, causing inconsistent authorization across the quorum.
#[test]
fn update_policy_goes_through_raft() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    let policy_update = PolicyUpdate {
        vcluster_id: "ml-training".into(),
        rego_source: "package pact.ml_training\ndefault allow = false".into(),
    };

    policy_service_update(&journal, &raft, &identity, policy_update).unwrap();

    assert_eq!(raft.proposals_received(), 1);
    assert!(raft.last_proposal_was_policy_update());
}

// ---------------------------------------------------------------------------
// BootConfigService RPC contracts
// ---------------------------------------------------------------------------

/// Contract: journal-interfaces.md § BootConfigService
/// Spec: overlay zstd compressed
/// If this test didn't exist: agents would receive uncompressed overlays,
/// causing 10x longer boot times over the management network.
#[test]
fn stream_boot_config_returns_compressed_chunks() {
    let journal = stub_journal_state();
    let identity = test_identity();

    journal.seed_overlay("ml-training", test_overlay_data());

    let chunks: Vec<_> = boot_config_service_stream(&journal, &identity, "ml-training")
        .collect();

    assert!(!chunks.is_empty());
    for chunk in &chunks[..chunks.len() - 1] {
        assert_eq!(chunk.encoding, Encoding::Zstd);
        assert!(!chunk.data.is_empty());
    }
}

/// Contract: journal-interfaces.md § BootConfigService
/// Spec: ConfigComplete with version + checksum
/// If this test didn't exist: an agent wouldn't know when streaming is done,
/// or whether the received data matches what was committed.
#[test]
fn stream_boot_config_ends_with_complete_message() {
    let journal = stub_journal_state();
    let identity = test_identity();

    journal.seed_overlay("ml-training", test_overlay_data());

    let chunks: Vec<_> = boot_config_service_stream(&journal, &identity, "ml-training")
        .collect();

    let last = chunks.last().unwrap();
    assert_matches!(last, StreamChunk::Complete { version, checksum } => {
        assert!(!version.is_empty());
        assert!(checksum.starts_with("sha256:"));
    });
}

/// Contract: journal-interfaces.md § BootConfigService
/// Spec: resume from sequence on reconnect
/// If this test didn't exist: a reconnecting agent would re-download the entire
/// config from the beginning, wasting bandwidth on every network blip.
#[test]
fn subscribe_config_updates_supports_from_sequence() {
    let journal = stub_journal_state();
    let identity = test_identity();

    journal.seed_entries(10);

    let updates: Vec<_> = boot_config_service_subscribe(
        &journal, &identity, "ml-training", Some(7), // Resume from sequence 7
    ).take(3).collect();

    // Should only receive entries after sequence 7
    assert!(updates.iter().all(|u| u.sequence > 7));
}

/// Contract: journal-interfaces.md § BootConfigService
/// Spec: push-based policy delivery
/// If this test didn't exist: policy changes wouldn't reach agents until
/// their next full config pull, leaving a window where stale policies apply.
#[test]
fn subscribe_config_updates_delivers_policy_changes() {
    let journal = stub_journal_state();
    let identity = test_identity();

    let subscription = boot_config_service_subscribe(
        &journal, &identity, "ml-training", None,
    );

    // Simulate a policy update after subscription
    journal.apply_policy_update("ml-training", "package pact\ndefault allow = false");

    let update = subscription.next().unwrap();
    assert_matches!(update.payload, UpdatePayload::PolicyChange { vcluster_id, .. } => {
        assert_eq!(vcluster_id, "ml-training");
    });
}

// ---------------------------------------------------------------------------
// Raft state machine contracts
// ---------------------------------------------------------------------------

/// Contract: journal-interfaces.md § Raft State Machine
/// Spec: J1 — strict increasing, no gaps
/// If this test didn't exist: a gap in sequences would break rollback,
/// since rollback traverses parent links assuming contiguous sequences.
#[test]
fn apply_assigns_monotonic_sequence() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    let mut sequences = Vec::new();
    for _ in 0..5 {
        let response = config_service_append_entry(
            &journal, &raft, &identity, test_config_entry(),
        ).unwrap();
        sequences.push(response.sequence);
    }

    // Strictly increasing with no gaps
    for window in sequences.windows(2) {
        assert_eq!(window[1], window[0] + 1);
    }
}

/// Contract: journal-interfaces.md § Raft State Machine
/// Spec: J9 — Raft serializes, unique sequences
/// If this test didn't exist: two concurrent proposals could receive the
/// same sequence number, corrupting the log.
#[test]
fn apply_rejects_concurrent_duplicate() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    // Simulate two proposals arriving with the same client-assigned sequence hint
    let entry_a = test_config_entry_with_hint(42);
    let entry_b = test_config_entry_with_hint(42);

    let result_a = config_service_append_entry(&journal, &raft, &identity, entry_a);
    let result_b = config_service_append_entry(&journal, &raft, &identity, entry_b);

    // Both succeed, but with different sequences (Raft serialized them)
    assert!(result_a.is_ok());
    assert!(result_b.is_ok());
    assert_ne!(result_a.unwrap().sequence, result_b.unwrap().sequence);
}

/// Contract: journal-interfaces.md § Raft State Machine
/// Spec: J2 — no update/delete exists
/// If this test didn't exist: an entry could be silently modified after commit,
/// destroying the immutable audit trail.
#[test]
fn state_machine_is_append_only() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();
    let identity = platform_admin();

    let response = config_service_append_entry(
        &journal, &raft, &identity, test_config_entry(),
    ).unwrap();

    // Attempting to overwrite an existing sequence must fail
    let overwrite = ConfigEntry {
        sequence_override: Some(response.sequence),
        ..test_config_entry()
    };

    let result = config_service_append_entry(&journal, &raft, &identity, overwrite);
    assert_matches!(result, Err(PactError::AppendOnly { .. }));

    // Attempting to delete must fail
    let result = config_service_delete_entry(&journal, &raft, &identity, response.sequence);
    assert_matches!(result, Err(PactError::AppendOnly { .. }));
}

// ---------------------------------------------------------------------------
// Telemetry contracts
// ---------------------------------------------------------------------------

/// Contract: journal-interfaces.md § Telemetry
/// Spec: role in health endpoint (leader/follower/candidate)
/// If this test didn't exist: monitoring wouldn't know which journal node
/// is the Raft leader, breaking alerting on leader elections and quorum loss.
#[test]
fn health_endpoint_returns_role() {
    let journal = stub_journal_state();
    let raft = stub_raft_node();

    let health = journal_health_check(&journal, &raft).unwrap();

    assert!(matches!(
        health.raft_role.as_str(),
        "leader" | "follower" | "candidate"
    ));
    assert!(health.term > 0);
}

/// Contract: journal-interfaces.md § Telemetry
/// Spec: O2 — metrics on port 9091
/// If this test didn't exist: Prometheus scrape configs pointing at 9091
/// would silently get nothing, and no one would notice until alerting fails.
#[test]
fn metrics_endpoint_on_port_9091() {
    let journal = stub_journal_state();

    let config = journal.metrics_config();
    assert_eq!(config.port, 9091);
    assert_eq!(config.path, "/metrics");

    let metrics_output = journal_scrape_metrics(&journal).unwrap();
    assert!(metrics_output.contains("pact_journal_"));
}
