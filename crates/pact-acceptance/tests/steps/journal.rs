//! Journal operations + overlay management steps — wired to JournalState::apply_command().

use chrono::Utc;
use cucumber::{given, then, when};
use pact_common::types::{
    AdminOperation, AdminOperationType, BootOverlay, ConfigEntry, DeltaAction, DeltaItem, EntryType,
    Identity, PrincipalType, Scope, StateDelta, VClusterPolicy,
};
use pact_journal::JournalCommand;
use uuid::Uuid;

use super::helpers::{md5_simple, parse_admin_op_type, parse_config_state, parse_entry_type};
use crate::PactWorld;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_identity(principal: &str, role: &str) -> Identity {
    Identity {
        principal: principal.to_string(),
        principal_type: PrincipalType::Human,
        role: role.to_string(),
    }
}

fn make_entry(entry_type: EntryType, scope: Scope, author: Identity) -> ConfigEntry {
    ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type,
        scope,
        author,
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    }
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given("a journal with default state")]
async fn given_journal_default(world: &mut PactWorld) {
    assert!(world.journal.entries.is_empty());
}

#[given(regex = r#"^a boot overlay for vCluster "([\w-]+)" version (\d+) with data "(.*)"$"#)]
async fn given_boot_overlay(world: &mut PactWorld, vcluster: String, version: u64, data: String) {
    let overlay = BootOverlay {
        vcluster_id: vcluster.clone(),
        version,
        checksum: format!("sha256:{:x}", md5_simple(&data)),
        data: data.into_bytes(),
    };
    world.journal.apply_command(JournalCommand::SetOverlay {
        vcluster_id: vcluster,
        overlay,
    });
}

#[given(
    regex = r#"^a boot overlay for vCluster "([\w-]+)" with (?:base )?sysctl(?: and mount)? config$"#
)]
async fn given_boot_overlay_sysctl(world: &mut PactWorld, vcluster: String) {
    let overlay = BootOverlay {
        vcluster_id: vcluster.clone(),
        version: 1,
        data: b"sysctl.vm.swappiness=60\nmount./scratch=nfs".to_vec(),
        checksum: "sha256:abc".to_string(),
    };
    world.journal.apply_command(JournalCommand::SetOverlay {
        vcluster_id: vcluster,
        overlay,
    });
}

#[given(regex = r#"^a boot overlay for vCluster "([\w-]+)"$"#)]
async fn given_boot_overlay_simple(world: &mut PactWorld, vcluster: String) {
    let overlay = BootOverlay {
        vcluster_id: vcluster.clone(),
        version: 1,
        data: b"default-config".to_vec(),
        checksum: "sha256:default".to_string(),
    };
    world.journal.apply_command(JournalCommand::SetOverlay {
        vcluster_id: vcluster,
        overlay,
    });
}

#[given(regex = r#"^no overlay exists for vCluster "([\w-]+)"$"#)]
async fn given_no_overlay(world: &mut PactWorld, vcluster: String) {
    world.journal.overlays.remove(&vcluster);
}

#[given(
    regex = r#"^a committed node delta for node "([\w-]+)" with kernel change "([\w.]+)" to "(.*)"$"#
)]
async fn given_node_delta(world: &mut PactWorld, node_id: String, key: String, value: String) {
    let mut entry = make_entry(
        EntryType::Commit,
        Scope::Node(node_id),
        make_identity("admin@example.com", "pact-platform-admin"),
    );
    entry.state_delta = Some(StateDelta {
        mounts: vec![],
        files: vec![],
        network: vec![],
        services: vec![],
        kernel: vec![DeltaItem {
            action: DeltaAction::Modify,
            key,
            value: Some(value),
            previous: None,
        }],
        packages: vec![],
        gpu: vec![],
    });
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[given(regex = r#"^a committed node delta for node "([\w-]+)"$"#)]
async fn given_node_delta_simple(world: &mut PactWorld, node_id: String) {
    let entry = make_entry(
        EntryType::Commit,
        Scope::Node(node_id),
        make_identity("admin@example.com", "pact-platform-admin"),
    );
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[given(
    regex = r#"^node "([\w-]+)" is subscribed to config updates for vCluster "([\w-]+)" from sequence (\d+)$"#
)]
async fn given_subscription(world: &mut PactWorld, node: String, vcluster: String, seq: u64) {
    world.subscriptions.insert(
        node,
        crate::ConfigSubscription {
            vcluster_id: vcluster,
            from_sequence: seq,
        },
    );
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(regex = r#"^I append a commit entry for vCluster "([\w-]+)" by "([\w@.]+)"$"#)]
async fn when_append_commit(world: &mut PactWorld, vcluster: String, author: String) {
    let entry = make_entry(
        EntryType::Commit,
        Scope::VCluster(vcluster),
        make_identity(&author, "pact-platform-admin"),
    );
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when(
    regex = r#"^I append a commit entry for vCluster "([\w-]+)" by "([\w@.]+)" with role "([\w-]+)"$"#
)]
async fn when_append_commit_role(
    world: &mut PactWorld,
    vcluster: String,
    author: String,
    role: String,
) {
    let entry = make_entry(
        EntryType::Commit,
        Scope::VCluster(vcluster),
        make_identity(&author, &role),
    );
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when(regex = r#"^I append a rollback entry for vCluster "([\w-]+)" by "([\w@.]+)"$"#)]
async fn when_append_rollback(world: &mut PactWorld, vcluster: String, author: String) {
    let entry = make_entry(
        EntryType::Rollback,
        Scope::VCluster(vcluster),
        make_identity(&author, "pact-platform-admin"),
    );
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when(
    regex = r#"^I append a commit entry with a kernel sysctl change "([\w.]+)" from "(.*)" to "(.*)"$"#
)]
async fn when_append_commit_sysctl(world: &mut PactWorld, key: String, from: String, to: String) {
    let mut entry = make_entry(
        EntryType::Commit,
        Scope::Global,
        make_identity("admin@example.com", "pact-platform-admin"),
    );
    entry.state_delta = Some(StateDelta {
        mounts: vec![],
        files: vec![],
        network: vec![],
        services: vec![],
        kernel: vec![DeltaItem {
            action: DeltaAction::Modify,
            key,
            value: Some(to),
            previous: Some(from),
        }],
        packages: vec![],
        gpu: vec![],
    });
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when(regex = r#"^I append a commit entry with TTL (\d+) seconds$"#)]
async fn when_append_commit_ttl(world: &mut PactWorld, ttl: u32) {
    let mut entry = make_entry(
        EntryType::Commit,
        Scope::Global,
        make_identity("admin@example.com", "pact-platform-admin"),
    );
    entry.ttl_seconds = Some(ttl);
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when(regex = r#"^I set node "([\w-]+)" state to "(\w+)"$"#)]
async fn when_set_node_state(world: &mut PactWorld, node: String, state_str: String) {
    let state = parse_config_state(&state_str);
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: node,
        state,
    });
}

#[when(
    regex = r#"^I set policy for vCluster "([\w-]+)" with max drift (\d+\.\d+) and commit window (\d+)$"#
)]
async fn when_set_policy(world: &mut PactWorld, vcluster: String, max_drift: f64, window: u32) {
    let policy = VClusterPolicy {
        vcluster_id: vcluster.clone(),
        drift_sensitivity: max_drift,
        base_commit_window_seconds: window,
        emergency_allowed: true,
        two_person_approval: false,
        ..VClusterPolicy::default()
    };
    world.journal.apply_command(JournalCommand::SetPolicy {
        vcluster_id: vcluster,
        policy,
    });
}

#[when(
    regex = r#"^I store a boot overlay for vCluster "([\w-]+)" version (\d+) with checksum "(.*)"$"#
)]
async fn when_store_overlay(
    world: &mut PactWorld,
    vcluster: String,
    version: u64,
    checksum: String,
) {
    let overlay = BootOverlay {
        vcluster_id: vcluster.clone(),
        version,
        data: vec![1, 2, 3],
        checksum,
    };
    world.journal.apply_command(JournalCommand::SetOverlay {
        vcluster_id: vcluster,
        overlay,
    });
}

#[when(
    regex = r#"^I record an exec operation by "([\w@.]+)" on node "([\w-]+)" with detail "(.*)"$"#
)]
async fn when_record_exec(world: &mut PactWorld, actor: String, node: String, detail: String) {
    let op = AdminOperation {
        operation_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        actor: make_identity(&actor, "pact-platform-admin"),
        operation_type: AdminOperationType::Exec,
        scope: Scope::Node(node),
        detail,
    };
    world.journal.apply_command(JournalCommand::RecordOperation(op));
}

#[when(regex = r#"^I record a shell session start by "([\w@.]+)" on node "([\w-]+)"$"#)]
async fn when_record_shell_start(world: &mut PactWorld, actor: String, node: String) {
    let op = AdminOperation {
        operation_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        actor: make_identity(&actor, "pact-platform-admin"),
        operation_type: AdminOperationType::ShellSessionStart,
        scope: Scope::Node(node),
        detail: "session started".to_string(),
    };
    world.journal.apply_command(JournalCommand::RecordOperation(op));
}

#[when(regex = r#"^I record a shell session end by "([\w@.]+)" on node "([\w-]+)"$"#)]
async fn when_record_shell_end(world: &mut PactWorld, actor: String, node: String) {
    let op = AdminOperation {
        operation_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        actor: make_identity(&actor, "pact-platform-admin"),
        operation_type: AdminOperationType::ShellSessionEnd,
        scope: Scope::Node(node),
        detail: "session ended".to_string(),
    };
    world.journal.apply_command(JournalCommand::RecordOperation(op));
}

#[when("the journal state is serialized and deserialized")]
async fn when_serde_roundtrip(world: &mut PactWorld) {
    let json = serde_json::to_string(&world.journal).unwrap();
    world.journal = serde_json::from_str(&json).unwrap();
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r#"^the entry should be assigned sequence (\d+)$"#)]
async fn then_assigned_sequence(world: &mut PactWorld, seq: u64) {
    assert!(
        world.journal.entries.contains_key(&seq),
        "entry at sequence {seq} not found"
    );
}

#[then(regex = r#"^the journal should contain (\d+) entr(?:y|ies)$"#)]
async fn then_journal_count(world: &mut PactWorld, count: usize) {
    assert_eq!(world.journal.entries.len(), count);
}

#[then(regex = r#"^entry (\d+) should have type "(\w+)"$"#)]
async fn then_entry_type(world: &mut PactWorld, seq: u64, entry_type_str: String) {
    let entry = world.journal.entries.get(&seq).expect("entry not found");
    let expected = parse_entry_type(&entry_type_str);
    assert_eq!(entry.entry_type, expected);
}

#[then(regex = r#"^entry (\d+) should have author "([\w@.]+)"$"#)]
async fn then_entry_author(world: &mut PactWorld, seq: u64, author: String) {
    let entry = world.journal.entries.get(&seq).expect("entry not found");
    assert_eq!(entry.author.principal, author);
}

#[then(regex = r#"^entry (\d+) should have role "([\w-]+)"$"#)]
async fn then_entry_role(world: &mut PactWorld, seq: u64, role: String) {
    let entry = world.journal.entries.get(&seq).expect("entry not found");
    assert_eq!(entry.author.role, role);
}

#[then(regex = r#"^entry (\d+) should have a kernel delta with key "([\w.]+)"$"#)]
async fn then_entry_kernel_delta(world: &mut PactWorld, seq: u64, key: String) {
    let entry = world.journal.entries.get(&seq).expect("entry not found");
    let delta = entry.state_delta.as_ref().expect("no state delta");
    assert!(delta.kernel.iter().any(|d| d.key == key));
}

#[then(regex = r#"^the delta action should be "(\w+)"$"#)]
async fn then_delta_action(world: &mut PactWorld, action_str: String) {
    let last_entry = world.journal.entries.values().last().expect("no entries");
    let delta = last_entry.state_delta.as_ref().expect("no state delta");
    let expected = match action_str.as_str() {
        "Add" => DeltaAction::Add,
        "Remove" => DeltaAction::Remove,
        "Modify" => DeltaAction::Modify,
        _ => panic!("unknown delta action: {action_str}"),
    };
    assert!(delta.kernel.iter().any(|d| d.action == expected));
}

#[then(regex = r#"^entry (\d+) should have TTL (\d+)$"#)]
async fn then_entry_ttl(world: &mut PactWorld, seq: u64, ttl: u32) {
    let entry = world.journal.entries.get(&seq).expect("entry not found");
    assert_eq!(entry.ttl_seconds, Some(ttl));
}

#[then(regex = r#"^node "([\w-]+)" should have state "(\w+)"$"#)]
async fn then_node_state(world: &mut PactWorld, node: String, state_str: String) {
    let expected = parse_config_state(&state_str);
    assert_eq!(world.journal.node_states.get(&node), Some(&expected));
}

#[then(regex = r#"^vCluster "([\w-]+)" should have a policy with max drift (\d+\.\d+)$"#)]
async fn then_policy_drift(world: &mut PactWorld, vcluster: String, max_drift: f64) {
    let policy = world.journal.policies.get(&vcluster).expect("policy not found");
    assert!((policy.drift_sensitivity - max_drift).abs() < f64::EPSILON);
}

#[then(regex = r#"^vCluster "([\w-]+)" should have commit window (\d+)$"#)]
async fn then_policy_window(world: &mut PactWorld, vcluster: String, window: u32) {
    let policy = world.journal.policies.get(&vcluster).expect("policy not found");
    assert_eq!(policy.base_commit_window_seconds, window);
}

#[then(regex = r#"^vCluster "([\w-]+)" should have overlay version (\d+)$"#)]
async fn then_overlay_version(world: &mut PactWorld, vcluster: String, version: u64) {
    let overlay = world.journal.overlays.get(&vcluster).expect("overlay not found");
    assert_eq!(overlay.version, version);
}

#[then(regex = r#"^vCluster "([\w-]+)" overlay should have checksum "(.*)"$"#)]
async fn then_overlay_checksum(world: &mut PactWorld, vcluster: String, checksum: String) {
    let overlay = world.journal.overlays.get(&vcluster).expect("overlay not found");
    assert_eq!(overlay.checksum, checksum);
}

#[then(regex = r#"^the audit log should contain (\d+) entr(?:y|ies)$"#)]
async fn then_audit_count(world: &mut PactWorld, count: usize) {
    assert_eq!(world.journal.audit_log.len(), count);
}

#[then(regex = r#"^audit entry (\d+) should have type "(\w+)"$"#)]
async fn then_audit_type(world: &mut PactWorld, idx: usize, op_type: String) {
    let op = &world.journal.audit_log[idx];
    let expected = parse_admin_op_type(&op_type);
    assert_eq!(op.operation_type, expected);
}

#[then(regex = r#"^audit entry (\d+) should have detail "(.*)"$"#)]
async fn then_audit_detail(world: &mut PactWorld, idx: usize, detail: String) {
    assert_eq!(world.journal.audit_log[idx].detail, detail);
}
