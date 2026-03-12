Feature: Emergency Mode
  Emergency mode provides an extended commit window (default 4 hours),
  suspends automatic rollback, and maintains full audit logging.
  It must end with an explicit commit or rollback. (ADR-004)

  Background:
    Given a journal with default state
    And default commit window config with base 900 seconds and sensitivity 2.0

  # --- Entering emergency mode ---

  Scenario: Enter emergency mode with reason
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "GPU firmware update"
    Then node "node-001" should be in emergency state
    And an EmergencyStart entry should be recorded in the journal
    And the emergency reason should be "GPU firmware update"

  Scenario: Emergency mode extends commit window to 4 hours
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "maintenance"
    Then the commit window for node "node-001" should be 14400 seconds

  Scenario: Emergency mode suspends automatic rollback
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "maintenance"
    And drift is detected with magnitude 0.8 on node "node-001"
    And the normal commit window would have expired
    Then no automatic rollback should be triggered

  # --- During emergency ---

  Scenario: All operations are logged during emergency
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "debug"
    And admin "admin@example.com" executes "dmesg" on node "node-001"
    Then the exec operation should be recorded in the audit log
    And the audit entry should reference the emergency session

  Scenario: Emergency mode does not expand shell whitelist
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "debug"
    Then the shell whitelist should remain unchanged
    And restricted bash restrictions should still apply

  # --- Exiting emergency mode ---

  Scenario: Emergency mode ends with explicit commit
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "maintenance"
    And changes are made during emergency
    And admin "admin@example.com" commits the changes
    Then an EmergencyEnd entry should be recorded in the journal
    And node "node-001" should return to committed state

  Scenario: Emergency mode ends with explicit rollback
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "failed attempt"
    And changes are made during emergency
    And admin "admin@example.com" rolls back the changes
    Then an EmergencyEnd entry should be recorded in the journal
    And node "node-001" should return to committed state

  # --- Stale emergency handling ---

  Scenario: Stale emergency triggers alert
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "maintenance"
    And the emergency window of 14400 seconds expires
    Then a stale emergency alert should be raised
    And no automatic rollback should be triggered

  Scenario: Stale emergency triggers scheduling hold
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "maintenance"
    And the emergency window of 14400 seconds expires
    Then a scheduling hold should be requested for node "node-001"

  Scenario: Another admin can force-end stale emergency
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "maintenance"
    And the emergency window expires
    And admin "ops@example.com" with role "pact-ops-ml-training" force-ends the emergency
    Then an EmergencyEnd entry should be recorded in the journal
    And the force-end should be attributed to "ops@example.com"

  # --- Emergency and committed deltas ---

  Scenario: Emergency changes get default TTL matching window
    When admin "admin@example.com" enters emergency mode on node "node-001" with reason "hotfix"
    And a commit is made during emergency mode
    Then the committed delta should have TTL equal to the emergency window

  # --- Authorization ---

  Scenario: Only ops and platform admin can enter emergency
    When viewer "viewer@example.com" with role "pact-viewer-ml-training" tries to enter emergency mode
    Then the operation should be denied with reason "authorization denied"

  Scenario: Emergency mode requires emergency_allowed in policy
    Given vCluster "locked-down" has policy with emergency_allowed false
    When admin "admin@example.com" tries to enter emergency mode on a node in vCluster "locked-down"
    Then the operation should be denied with reason "policy rejection"
