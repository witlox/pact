Feature: Commit Window
  When drift is detected, a time-limited commit window opens. The admin
  must commit or rollback within the window. Window duration is inversely
  proportional to drift magnitude: large drift = shorter window.

  Formula: window_seconds = base_window / (1 + drift_magnitude * sensitivity)

  Background:
    Given a journal with default state
    And default commit window config with base 900 seconds and sensitivity 2.0

  # --- Window calculation ---

  Scenario: Tiny drift gets long commit window
    When drift is detected with magnitude 0.05
    Then the commit window should be approximately 818 seconds

  Scenario: Small drift gets moderate commit window
    When drift is detected with magnitude 0.15
    Then the commit window should be approximately 692 seconds

  Scenario: Moderate drift gets shorter commit window
    When drift is detected with magnitude 0.3
    Then the commit window should be approximately 562 seconds

  Scenario: Large drift gets short commit window
    When drift is detected with magnitude 0.8
    Then the commit window should be approximately 346 seconds

  Scenario: Higher sensitivity compresses windows more
    Given commit window config with base 900 seconds and sensitivity 5.0
    When drift is detected with magnitude 0.8
    Then the commit window should be approximately 180 seconds

  # --- Window lifecycle ---

  Scenario: Commit within window succeeds
    When drift is detected with magnitude 0.3
    And a commit window is opened
    And the admin commits within the window
    Then the commit should succeed
    And the node state should be "Committed"

  Scenario: Rollback within window succeeds
    When drift is detected with magnitude 0.3
    And a commit window is opened
    And the admin rolls back within the window
    Then the rollback should succeed
    And the node state should be "Committed"

  Scenario: Window expiry triggers automatic rollback
    When drift is detected with magnitude 0.3
    And a commit window is opened
    And the window expires without action
    Then an automatic rollback should be triggered
    And a rollback entry should be recorded in the journal

  # --- Active consumer protection ---

  Scenario: Rollback checks for active consumers
    When drift is detected for a mount change on "/scratch"
    And the mount "/scratch" has active consumers
    And the window expires
    Then the rollback should be deferred until consumers release
    And an alert should be raised about active consumers

  Scenario: Rollback proceeds when no active consumers
    When drift is detected for a mount change on "/scratch"
    And the mount "/scratch" has no active consumers
    And the window expires
    Then the automatic rollback should proceed

  # --- Journal recording ---

  Scenario: Drift detection is recorded in journal
    When drift is detected with magnitude 0.3 on node "node-001"
    Then a DriftDetected entry should be recorded in the journal
    And the entry should have scope node "node-001"

  Scenario: Commit is recorded in journal
    When drift is detected with magnitude 0.3 on node "node-001"
    And the admin commits with message "approved sysctl change"
    Then a Commit entry should be recorded in the journal
    And the entry should have the state delta

  Scenario: Rollback is recorded in journal
    When drift is detected with magnitude 0.3 on node "node-001"
    And the window expires
    Then a Rollback entry should be recorded in the journal

  # --- Node delta TTL ---

  Scenario: Committed delta without TTL persists indefinitely
    When a commit is made without specifying a TTL
    Then the committed delta should have no TTL
    And the delta should persist across reboots

  Scenario: Committed delta with TTL expires
    When a commit is made with TTL 3600 seconds
    And 3600 seconds have elapsed
    Then the committed delta should be expired
    And the delta should be cleaned up

  Scenario: Emergency mode changes get default TTL
    Given emergency mode is active with window 14400 seconds
    When a commit is made during emergency mode
    Then the committed delta should have TTL 14400

  # --- TTL bounds enforcement (ND1, ND2) ---

  Scenario: TTL below minimum is rejected
    When the user runs "pact commit -m 'too short' --ttl 300"
    Then the commit should be rejected
    And the error should say "TTL must be >= 900 seconds (15 minutes)"
    And no entry should be recorded in the journal

  Scenario: TTL at minimum boundary is accepted
    When the user runs "pact commit -m 'minimum ttl' --ttl 900"
    Then the commit should succeed
    And the committed delta should have TTL 900

  Scenario: TTL above maximum is rejected
    When the user runs "pact commit -m 'too long' --ttl 1000000"
    Then the commit should be rejected
    And the error should say "TTL must be <= 864000 seconds (10 days)"
    And no entry should be recorded in the journal

  Scenario: TTL at maximum boundary is accepted
    When the user runs "pact commit -m 'maximum ttl' --ttl 864000"
    Then the commit should succeed
    And the committed delta should have TTL 864000
