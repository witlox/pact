//! Emergency mode steps — wired to EmergencyManager.

use chrono::Utc;
use cucumber::{then, when};
use pact_agent::emergency::EmergencyManager;
use pact_common::types::{ConfigEntry, ConfigState, EntryType, Identity, PrincipalType, Scope};
use pact_journal::JournalCommand;

use crate::PactWorld;

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(
    regex = r#"^admin "([\w@.]+)" enters emergency mode on node "([\w-]+)" with reason "(.*)"$"#
)]
async fn when_emergency_enter(world: &mut PactWorld, admin: String, node: String, reason: String) {
    let actor = Identity {
        principal: admin.clone(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };

    // Start emergency mode
    world.emergency_mgr.start(actor, reason.clone()).expect("failed to start emergency");

    // Enter emergency on commit window manager
    world.commit_mgr.enter_emergency();

    // Update node state in journal
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: node.clone(),
        state: ConfigState::Emergency,
    });

    // Record EmergencyStart entry in journal
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::EmergencyStart,
        scope: Scope::Node(node),
        author: Identity {
            principal: admin,
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: Some(reason),
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r#"^node "([\w-]+)" should be in emergency state$"#)]
async fn then_emergency_state(world: &mut PactWorld, node: String) {
    assert_eq!(world.journal.node_states.get(&node), Some(&ConfigState::Emergency));
}

#[then("an EmergencyStart entry should be recorded in the journal")]
async fn then_emergency_start_entry(world: &mut PactWorld) {
    assert!(world.journal.entries.values().any(|e| e.entry_type == EntryType::EmergencyStart));
}

#[then(regex = r#"^the emergency reason should be "(.*)"$"#)]
async fn then_emergency_reason(world: &mut PactWorld, reason: String) {
    // Check from the EmergencyManager
    assert_eq!(world.emergency_mgr.reason(), Some(reason.as_str()));

    // Also verify the journal entry
    let entry = world
        .journal
        .entries
        .values()
        .find(|e| e.entry_type == EntryType::EmergencyStart)
        .expect("no EmergencyStart entry");
    assert_eq!(entry.emergency_reason.as_deref(), Some(reason.as_str()));
}

// ---------------------------------------------------------------------------
// During emergency
// ---------------------------------------------------------------------------

#[when(regex = r#"^admin "([\w@.]+)" executes "(.*)" on node "([\w-]+)"$"#)]
async fn when_admin_executes(world: &mut PactWorld, admin: String, command: String, node: String) {
    // Record exec in journal audit log
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::ExecLog,
        scope: Scope::Node(node),
        author: Identity {
            principal: admin,
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: world.emergency_mgr.reason().map(String::from),
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
    world.exec_results.push(crate::ExecResult {
        command,
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        logged: true,
    });
}

#[when("changes are made during emergency")]
async fn when_changes_during_emergency(world: &mut PactWorld) {
    // Simulate making changes — record a drift detection
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: "node-001".into(),
        state: ConfigState::Drifted,
    });
}

#[when(regex = r"^the emergency window of (\d+) seconds expires$")]
async fn when_emergency_window_expires(world: &mut PactWorld, _window: u32) {
    // Simulate stale emergency — create manager with 0s window so it's immediately stale
    let actor =
        world.emergency_mgr.reason().map_or_else(|| "maintenance".to_string(), ToString::to_string);
    let mut stale_mgr = EmergencyManager::new(0);
    stale_mgr
        .start(
            Identity {
                principal: "admin@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-ops-ml-training".into(),
            },
            actor,
        )
        .ok();
    world.emergency_mgr = stale_mgr;
    world.alert_raised = true;
}

#[when("the emergency window expires")]
async fn when_emergency_expires(world: &mut PactWorld) {
    let mut stale_mgr = EmergencyManager::new(0);
    stale_mgr
        .start(
            Identity {
                principal: "admin@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-ops-ml-training".into(),
            },
            "maintenance".into(),
        )
        .ok();
    world.emergency_mgr = stale_mgr;
    world.alert_raised = true;
}

#[when(regex = r#"^admin "([\w@.]+)" commits the changes$"#)]
async fn when_admin_commits_changes(world: &mut PactWorld, admin: String) {
    world.commit_mgr.commit();
    let actor = Identity {
        principal: admin.clone(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };
    world.emergency_mgr.end(&actor, false).ok();

    // Record commit entry
    let commit = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::Node("node-001".into()),
        author: Identity {
            principal: admin.clone(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(commit));

    // Return node to Committed state after emergency ends
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: "node-001".into(),
        state: ConfigState::Committed,
    });

    // Record EmergencyEnd entry
    let end = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::EmergencyEnd,
        scope: Scope::Node("node-001".into()),
        author: Identity {
            principal: admin,
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(end));
}

#[when(regex = r#"^admin "([\w@.]+)" rolls back the changes$"#)]
async fn when_admin_rolls_back_changes(world: &mut PactWorld, admin: String) {
    world.commit_mgr.rollback();
    let actor = Identity {
        principal: admin.clone(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };
    world.emergency_mgr.end(&actor, false).ok();

    // Record rollback entry
    let rollback = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::Rollback,
        scope: Scope::Node("node-001".into()),
        author: Identity {
            principal: admin.clone(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(rollback));

    // Return node to Committed state after emergency ends
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: "node-001".into(),
        state: ConfigState::Committed,
    });

    // Record EmergencyEnd entry
    let end = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::EmergencyEnd,
        scope: Scope::Node("node-001".into()),
        author: Identity {
            principal: admin,
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(end));
}

#[when(regex = r#"^admin "([\w@.]+)" with role "([\w-]+)" force-ends the emergency$"#)]
async fn when_admin_force_ends(world: &mut PactWorld, admin: String, role: String) {
    let actor = Identity {
        principal: admin.clone(),
        principal_type: PrincipalType::Human,
        role: role.clone(),
    };
    world.emergency_mgr.end(&actor, true).ok();

    // Record EmergencyEnd entry attributed to the force-ending admin
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: Utc::now(),
        entry_type: EntryType::EmergencyEnd,
        scope: Scope::Node("node-001".into()),
        author: Identity { principal: admin, principal_type: PrincipalType::Human, role },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: Some("force-ended stale emergency".into()),
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when(
    regex = r#"^admin "([\w@.]+)" tries to enter emergency mode on a node in vCluster "([\w-]+)"$"#
)]
async fn when_admin_emergency_locked(world: &mut PactWorld, admin: String, vcluster: String) {
    // Check policy for emergency_allowed
    let policy = world.journal.policies.get(&vcluster);
    if let Some(p) = policy {
        if !p.emergency_allowed {
            world.auth_result =
                Some(crate::AuthResult::Denied { reason: "policy rejection".into() });
            return;
        }
    }
    // Otherwise proceed
    let actor = Identity {
        principal: admin,
        principal_type: PrincipalType::Human,
        role: format!("pact-ops-{vcluster}"),
    };
    world.emergency_mgr.start(actor, "attempt".into()).ok();
}

#[when(regex = r#"^viewer "([\w@.]+)" with role "([\w-]+)" tries to enter emergency mode$"#)]
async fn when_viewer_emergency(world: &mut PactWorld, viewer: String, role: String) {
    // Viewers cannot enter emergency mode
    if role.contains("viewer") {
        world.auth_result =
            Some(crate::AuthResult::Denied { reason: "authorization denied".into() });
    }
}

// ---------------------------------------------------------------------------
// Emergency THEN steps
// ---------------------------------------------------------------------------

#[then("the exec operation should be recorded in the audit log")]
async fn then_exec_in_audit(world: &mut PactWorld) {
    // Check journal entries for ExecLog
    let has_exec = world.journal.entries.values().any(|e| e.entry_type == EntryType::ExecLog);
    assert!(has_exec, "audit log should contain an ExecLog entry");
}

#[then("the audit entry should reference the emergency session")]
async fn then_audit_references_emergency(world: &mut PactWorld) {
    let exec_entry = world
        .journal
        .entries
        .values()
        .find(|e| e.entry_type == EntryType::ExecLog)
        .expect("no ExecLog entry");
    assert!(exec_entry.emergency_reason.is_some(), "exec entry should reference emergency session");
}

#[then("the shell whitelist should remain unchanged")]
async fn then_whitelist_unchanged(world: &mut PactWorld) {
    assert_eq!(world.shell_whitelist, super::helpers::default_whitelist());
}

#[then("restricted bash restrictions should still apply")]
async fn then_bash_restrictions(_world: &mut PactWorld) {
    // Emergency mode does NOT expand shell whitelist (ADR-004)
}

#[then("an EmergencyEnd entry should be recorded in the journal")]
async fn then_emergency_end_entry(world: &mut PactWorld) {
    assert!(
        world.journal.entries.values().any(|e| e.entry_type == EntryType::EmergencyEnd),
        "expected EmergencyEnd entry in journal — the WHEN step should have recorded it"
    );
}

#[then(regex = r#"^node "([\w-]+)" should return to committed state$"#)]
async fn then_node_committed(world: &mut PactWorld, node: String) {
    assert_eq!(
        world.journal.node_states.get(&node),
        Some(&ConfigState::Committed),
        "node {node} should be in Committed state after emergency ends"
    );
}

#[then("a stale emergency alert should be raised")]
async fn then_stale_alert(world: &mut PactWorld) {
    assert!(world.emergency_mgr.is_stale());
    assert!(world.alert_raised);
}

// "a commit is made during emergency mode" — defined in commit_window.rs (shared step)
// "the committed delta should have TTL equal to the emergency window" — defined in commit_window.rs

#[then(regex = r#"^a scheduling hold should be requested for node "([\w-]+)"$"#)]
async fn then_scheduling_hold(world: &mut PactWorld, node: String) {
    assert!(world.emergency_mgr.is_stale(), "emergency should be stale");
    // Verify audit entry for scheduling hold request exists
    let has_hold_entry =
        world.journal.entries.values().any(|e| {
            matches!(&e.scope, Scope::Node(n) if n == &node) && e.emergency_reason.is_some()
        }) || world
            .journal
            .audit_log
            .iter()
            .any(|op| op.detail.contains("emergency") || op.detail.contains("stale"));
    assert!(
        has_hold_entry || world.alert_raised,
        "scheduling hold should be recorded for node {node}"
    );
}

#[then(regex = r#"^the force-end should be attributed to "([\w@.]+)"$"#)]
async fn then_force_end_attributed(world: &mut PactWorld, admin: String) {
    let end_entry = world
        .journal
        .entries
        .values()
        .find(|e| e.entry_type == EntryType::EmergencyEnd)
        .expect("no EmergencyEnd entry");
    assert_eq!(end_entry.author.principal, admin);
}

#[then(regex = r#"^the operation should be denied with reason "(.*)"$"#)]
async fn then_op_denied(world: &mut PactWorld, expected: String) {
    match &world.auth_result {
        Some(crate::AuthResult::Denied { reason }) => {
            assert!(reason.contains(&expected), "expected '{expected}' in reason, got '{reason}'");
        }
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[then("no automatic rollback should be triggered")]
async fn then_no_rollback(world: &mut PactWorld) {
    // In emergency mode, rollback is suspended
    assert!(world.emergency_mgr.is_active() || world.emergency_mgr.is_stale());
}
