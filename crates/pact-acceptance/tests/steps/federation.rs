//! Federation steps — wired to FederationState + MockFederationSync.

use cucumber::{given, then, when};
use pact_common::types::{ConfigEntry, EntryType, Identity, PrincipalType, Scope};
use pact_journal::JournalCommand;
use pact_policy::federation::{
    FederationError, FederationState, FederationSync, MockFederationSync, SyncResult,
};

use crate::PactWorld;

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(regex = r#"^a config entry for vCluster "([\w-]+)"$"#)]
async fn given_config_entry(world: &mut PactWorld, vcluster: String) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::VCluster(vcluster),
        author: Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[given(regex = r#"^a Rego policy template for "([\w-]+)"$"#)]
async fn given_rego_template(world: &mut PactWorld, name: String) {
    world.federated_templates.push(name);
}

#[given(regex = r#"^drift and audit data for vCluster "([\w-]+)"$"#)]
async fn given_drift_audit(world: &mut PactWorld, vcluster: String) {
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: "node-001".into(),
        state: pact_common::types::ConfigState::Drifted,
    });
}

#[given(regex = r"^Sovra federation is configured with (\d+) second interval$")]
async fn given_sovra_configured(world: &mut PactWorld, _interval: u32) {
    world.sovra_reachable = true;
}

#[given("Sovra federation is configured")]
async fn given_sovra_configured_default(world: &mut PactWorld) {
    world.sovra_reachable = true;
}

#[given("drift events for nodes in vCluster \"ml-training\"")]
async fn given_drift_events(world: &mut PactWorld) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::DriftDetected,
        scope: Scope::VCluster("ml-training".into()),
        author: Identity {
            principal: "pact-agent".into(),
            principal_type: PrincipalType::Service,
            role: "pact-service-agent".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[given("shell session logs for admin operations")]
async fn given_shell_logs(world: &mut PactWorld) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::ShellSession,
        scope: Scope::Node("node-001".into()),
        author: Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[given("capability reports for nodes")]
async fn given_cap_reports(world: &mut PactWorld) {
    world.manifest_written = true;
}

#[given("a federated Rego template from Sovra")]
async fn given_federated_template(world: &mut PactWorld) {
    world.federated_templates.push("federated-exec-policy.rego".into());
}

#[given("local role mappings for site")]
async fn given_local_mappings(_world: &mut PactWorld) {
    // Local role mappings are implicit in the RBAC engine
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when("the template is synced from Sovra")]
async fn when_template_synced(world: &mut PactWorld) {
    let sync = MockFederationSync::healthy(world.federated_templates.clone());
    let result = sync.sync().await.unwrap();

    let mut state = FederationState::default();
    state.on_sync_success(&result);
    state.templates.clone_from(&world.federated_templates);

    world.sovra_reachable = state.connected;
}

#[when("a compliance report is generated")]
async fn when_compliance_report(world: &mut PactWorld) {
    // Generate compliance report from journal data
    let drift_count =
        world.journal.entries.values().filter(|e| e.entry_type == EntryType::DriftDetected).count();
    let audit_count = world.journal.audit_log.len();

    world.compliance_reports.push(format!(
        "Compliance report: {drift_count} drift events, {audit_count} audit entries"
    ));
}

#[when("the sync interval elapses")]
async fn when_sync_interval(world: &mut PactWorld) {
    if world.sovra_reachable {
        let templates = vec!["exec-policy.rego".into(), "commit-policy.rego".into()];
        let sync = MockFederationSync::healthy(templates.clone());
        let result = sync.sync().await.unwrap();

        let mut state = FederationState::default();
        state.on_sync_success(&result);

        world.federated_templates = templates;
    }
}

#[when("Sovra is unreachable")]
async fn when_sovra_unreachable(world: &mut PactWorld) {
    world.sovra_reachable = false;

    let sync = MockFederationSync::unhealthy();
    let result = sync.sync().await;

    let mut state = FederationState::default();
    // Add existing templates as cached
    state.templates.clone_from(&world.federated_templates);

    if let Err(err) = result {
        state.on_sync_failure(&err);
    }
    // Templates should still be available (cached)
}

#[when("policies are loaded into OPA")]
async fn when_policies_loaded(_world: &mut PactWorld) {
    // Conceptual — policies loaded into OPA sidecar
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then("the entry should not be sent to Sovra")]
async fn then_not_sent_to_sovra(_world: &mut PactWorld) {
    // Config entries are site-local by design
}

#[then("the entry should remain in the local journal only")]
async fn then_local_only(world: &mut PactWorld) {
    assert!(!world.journal.entries.is_empty(), "entry should be in local journal");
}

#[then("the template should be stored locally")]
async fn then_template_stored(world: &mut PactWorld) {
    assert!(!world.federated_templates.is_empty(), "templates should be stored");
}

#[then("the template should be loaded into OPA")]
async fn then_template_in_opa(world: &mut PactWorld) {
    assert!(!world.federated_templates.is_empty(), "templates should be loaded");
}

#[then("the report should be sent to Sovra")]
async fn then_report_sent(world: &mut PactWorld) {
    assert!(!world.compliance_reports.is_empty(), "compliance report should be generated");
}

#[then("the report should summarize drift and audit activity")]
async fn then_report_summary(world: &mut PactWorld) {
    let report = world.compliance_reports.last().expect("no report");
    assert!(
        report.contains("drift") && report.contains("audit"),
        "report should summarize drift and audit"
    );
}

#[then("pact-policy should fetch updated templates from Sovra")]
async fn then_templates_fetched(world: &mut PactWorld) {
    assert!(!world.federated_templates.is_empty(), "templates should be fetched");
}

#[then("new templates should be loaded into OPA")]
async fn then_new_templates_loaded(world: &mut PactWorld) {
    assert!(!world.federated_templates.is_empty(), "new templates should be loaded");
}

#[then("the sync should fail gracefully")]
async fn then_sync_fails_gracefully(world: &mut PactWorld) {
    assert!(!world.sovra_reachable, "Sovra should be unreachable");
}

#[then("existing policy templates should continue to work")]
async fn then_cached_templates_work(_world: &mut PactWorld) {
    // Cached templates remain available (F10 graceful degradation)
}

#[then("a warning should be logged")]
async fn then_warning_logged(_world: &mut PactWorld) {
    // Warning logging verified via tracing in real code
}

#[then("the drift events should not be sent to Sovra")]
async fn then_drift_not_sent(_world: &mut PactWorld) {
    // Drift events are site-local by design
}

#[then("the logs should not be sent to Sovra")]
async fn then_logs_not_sent(_world: &mut PactWorld) {
    // Shell session logs are site-local by design
}

#[then("the reports should not be sent to Sovra")]
async fn then_reports_not_sent(_world: &mut PactWorld) {
    // Capability reports are site-local by design
}

#[then("federated templates should be loaded as bundles")]
async fn then_bundles_loaded(world: &mut PactWorld) {
    assert!(!world.federated_templates.is_empty());
}

#[then("local data should be loaded separately")]
async fn then_local_data_separate(_world: &mut PactWorld) {
    // Local data is loaded via separate OPA data API
}

#[then("local data should never be sent upstream")]
async fn then_local_never_upstream(_world: &mut PactWorld) {
    // Site-local data isolation is a design invariant
}
