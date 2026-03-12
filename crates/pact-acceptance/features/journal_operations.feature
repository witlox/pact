Feature: Journal Operations
  The journal is the immutable append-only log at the heart of pact.
  All configuration changes, policy updates, and admin operations are
  recorded through Raft consensus.

  Background:
    Given a journal with default state

  # --- Entry lifecycle ---

  Scenario: Append a config commit entry
    When I append a commit entry for vCluster "ml-training" by "admin@example.com"
    Then the entry should be assigned sequence 0
    And the journal should contain 1 entry

  Scenario: Sequence numbers increment monotonically
    When I append a commit entry for vCluster "ml-training" by "admin@example.com"
    And I append a rollback entry for vCluster "ml-training" by "admin@example.com"
    And I append a commit entry for vCluster "dev-sandbox" by "ops@example.com"
    Then the journal should contain 3 entries
    And entry 0 should have type "Commit"
    And entry 1 should have type "Rollback"
    And entry 2 should have type "Commit"

  Scenario: Entry records author identity
    When I append a commit entry for vCluster "ml-training" by "admin@example.com" with role "pact-ops-ml-training"
    Then entry 0 should have author "admin@example.com"
    And entry 0 should have role "pact-ops-ml-training"

  Scenario: Entry records state delta
    When I append a commit entry with a kernel sysctl change "vm.swappiness" from "60" to "10"
    Then entry 0 should have a kernel delta with key "vm.swappiness"
    And the delta action should be "Modify"

  Scenario: Entry with TTL is recorded
    When I append a commit entry with TTL 3600 seconds
    Then entry 0 should have TTL 3600

  # --- Node state tracking ---

  Scenario: Update node config state
    When I set node "node-001" state to "Committed"
    Then node "node-001" should have state "Committed"

  Scenario: Node state transitions are tracked
    When I set node "node-001" state to "Committed"
    And I set node "node-001" state to "Drifted"
    Then node "node-001" should have state "Drifted"

  Scenario: Multiple nodes tracked independently
    When I set node "node-001" state to "Committed"
    And I set node "node-002" state to "Drifted"
    And I set node "node-003" state to "Emergency"
    Then node "node-001" should have state "Committed"
    And node "node-002" should have state "Drifted"
    And node "node-003" should have state "Emergency"

  # --- Policy management ---

  Scenario: Set vCluster policy
    When I set policy for vCluster "ml-training" with max drift 5.0 and commit window 900
    Then vCluster "ml-training" should have a policy with max drift 5.0

  Scenario: Policy update replaces previous
    When I set policy for vCluster "ml-training" with max drift 5.0 and commit window 900
    And I set policy for vCluster "ml-training" with max drift 3.0 and commit window 600
    Then vCluster "ml-training" should have a policy with max drift 3.0
    And vCluster "ml-training" should have commit window 600

  # --- Boot overlay management ---

  Scenario: Store boot overlay for vCluster
    When I store a boot overlay for vCluster "ml-training" version 1 with checksum "abc123"
    Then vCluster "ml-training" should have overlay version 1

  Scenario: Overlay update replaces previous version
    When I store a boot overlay for vCluster "ml-training" version 1 with checksum "abc123"
    And I store a boot overlay for vCluster "ml-training" version 2 with checksum "def456"
    Then vCluster "ml-training" should have overlay version 2
    And vCluster "ml-training" overlay should have checksum "def456"

  # --- Audit log ---

  Scenario: Record exec operation in audit log
    When I record an exec operation by "admin@example.com" on node "node-001" with detail "uname -a"
    Then the audit log should contain 1 entry
    And audit entry 0 should have type "Exec"
    And audit entry 0 should have detail "uname -a"

  Scenario: Record shell session lifecycle in audit log
    When I record a shell session start by "admin@example.com" on node "node-001"
    And I record a shell session end by "admin@example.com" on node "node-001"
    Then the audit log should contain 2 entries
    And audit entry 0 should have type "ShellSessionStart"
    And audit entry 1 should have type "ShellSessionEnd"

  # --- Serialization ---

  Scenario: Journal state survives serialization roundtrip
    When I append a commit entry for vCluster "ml-training" by "admin@example.com"
    And I set node "node-001" state to "Committed"
    And I set policy for vCluster "ml-training" with max drift 5.0 and commit window 900
    And the journal state is serialized and deserialized
    Then the journal should contain 1 entry
    And node "node-001" should have state "Committed"
    And vCluster "ml-training" should have a policy with max drift 5.0
