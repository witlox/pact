Feature: Federation
  pact integrates with Sovra for cross-site policy federation.
  Configuration state stays site-local; policy templates are
  federated. OPA/Rego chosen for Sovra compatibility.

  Background:
    Given a journal with default state

  # --- What federates vs stays local ---

  Scenario: Config state stays site-local
    Given a config entry for vCluster "ml-training"
    Then the entry should not be sent to Sovra
    And the entry should remain in the local journal only

  Scenario: Rego policy templates are federated
    Given a Rego policy template for "regulated-workload-requirements"
    When the template is synced from Sovra
    Then the template should be stored locally
    And the template should be loaded into OPA

  Scenario: Compliance reports are federated
    Given drift and audit data for vCluster "regulated-hpc"
    When a compliance report is generated
    Then the report should be sent to Sovra
    And the report should summarize drift and audit activity

  # --- Sovra sync ---

  Scenario: Policy templates synced on interval
    Given Sovra federation is configured with 300 second interval
    When the sync interval elapses
    Then pact-policy should fetch updated templates from Sovra
    And new templates should be loaded into OPA

  Scenario: Sync fails gracefully when Sovra unreachable
    Given Sovra federation is configured
    When Sovra is unreachable
    Then the sync should fail gracefully
    And existing policy templates should continue to work
    And a warning should be logged

  # --- Site-local data isolation ---

  Scenario: Drift events never leave site
    Given drift events for nodes in vCluster "ml-training"
    Then the drift events should not be sent to Sovra

  Scenario: Shell session logs never leave site
    Given shell session logs for admin operations
    Then the logs should not be sent to Sovra

  Scenario: Capability reports never leave site
    Given capability reports for nodes
    Then the reports should not be sent to Sovra

  # --- OPA data separation ---

  Scenario: OPA receives federated templates and local data separately
    Given a federated Rego template from Sovra
    And local role mappings for site
    When policies are loaded into OPA
    Then federated templates should be loaded as bundles
    And local data should be loaded separately
    And local data should never be sent upstream
