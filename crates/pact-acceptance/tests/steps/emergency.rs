//! Emergency mode steps — wired to EmergencyManager.

use chrono::Utc;
use cucumber::{then, when};
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
    world
        .emergency_mgr
        .start(actor.clone(), reason.clone())
        .expect("failed to start emergency");

    // Enter emergency on commit window manager
    world.commit_mgr.enter_emergency();

    // Update node state in journal
    world
        .journal
        .apply_command(JournalCommand::UpdateNodeState {
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
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r#"^node "([\w-]+)" should be in emergency state$"#)]
async fn then_emergency_state(world: &mut PactWorld, node: String) {
    assert_eq!(
        world.journal.node_states.get(&node),
        Some(&ConfigState::Emergency)
    );
}

#[then("an EmergencyStart entry should be recorded in the journal")]
async fn then_emergency_start_entry(world: &mut PactWorld) {
    assert!(world
        .journal
        .entries
        .values()
        .any(|e| e.entry_type == EntryType::EmergencyStart));
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
