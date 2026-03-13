Feature: Partition Resilience
  pact follows an AP consistency model. Nodes use cached config and
  cached policy during network partitions. Degraded-mode decisions
  are logged locally and replayed when connectivity is restored.

  Background:
    Given a journal with default state
    And node "node-001" has cached config and policy for vCluster "ml-training"

  # --- Agent during journal partition ---

  Scenario: Agent uses cached config when journal is unreachable
    Given the journal is unreachable from node "node-001"
    When node "node-001" boots
    Then the agent should apply cached vCluster overlay
    And the agent should apply cached node delta
    And the boot should succeed with cached config

  Scenario: Agent uses cached policy for authorization during partition
    Given the journal is unreachable from node "node-001"
    When user "admin@example.com" executes "dmesg" on node "node-001"
    Then the command should be authorized using cached policy
    And the authorization should be logged as "degraded"

  Scenario: Two-person approval denied during partition
    Given the journal is unreachable from node "node-001"
    And vCluster "ml-training" requires two-person approval
    When user "admin@example.com" requests a state-changing operation
    Then the operation should be denied
    And the denial reason should be "two-person approval unavailable during partition"

  Scenario: Complex OPA rules denied during partition
    Given the journal is unreachable from node "node-001"
    When a policy evaluation requiring OPA Rego rules is requested
    Then the evaluation should fall back to cached RBAC
    And OPA-specific rules should be denied

  # --- Recovery after partition ---

  Scenario: Degraded-mode decisions replayed on reconnect
    Given the journal was unreachable from node "node-001"
    And 3 operations were performed in degraded mode
    When connectivity to the journal is restored
    Then all 3 operations should be replayed to the journal
    And the replay should preserve original timestamps

  Scenario: Drift detected during partition is reported on reconnect
    Given the journal was unreachable from node "node-001"
    And drift was detected during the partition
    When connectivity to the journal is restored
    Then the drift event should be reported to the journal
    And a DriftDetected entry should be recorded

  # --- Journal leader failover ---

  Scenario: Writes continue after leader failover
    Given a 3-node journal cluster with node 1 as leader
    When the leader node fails
    Then a new leader should be elected
    And writes should continue on the new leader

  Scenario: Reads available from followers during failover
    Given a 3-node journal cluster with node 1 as leader
    When the leader node fails
    Then boot config reads should still be available from followers
    And config queries should still work from followers

  # --- Platform admin override during partition ---

  Scenario: Platform admin can operate with cached role during partition
    Given the journal is unreachable from node "node-001"
    And user "admin@example.com" has cached role "pact-platform-admin"
    When user "admin@example.com" performs an operation
    Then the operation should be authorized using cached role
    And the operation should be logged as "degraded"

  # --- Config subscription resilience ---

  Scenario: Config subscription reconnects after partition heals
    Given node "node-001" was subscribed to config updates
    And the subscription was interrupted by a partition
    When connectivity is restored
    Then the subscription should reconnect with the last known sequence
    And missed updates should be delivered

  # --- Conflict resolution on reconnect (CR1-CR3) ---

  Scenario: Local changes fed back before journal sync on reconnect
    Given the journal is unreachable from node "node-001"
    And an admin changes "vm.swappiness" to "10" on node "node-001" via pact shell
    When connectivity to the journal is restored
    Then node "node-001" should report its local changes to the journal first
    And only after local changes are recorded should it accept the journal state stream

  Scenario: Merge conflict pauses agent convergence
    Given the journal is unreachable from node "node-001"
    And an admin changes "kernel.shmmax" to "68719476736" on node "node-001" via pact shell
    And meanwhile "kernel.shmmax" is committed as "34359738368" in the journal for vCluster "ml-training"
    When connectivity to the journal is restored
    Then node "node-001" should detect a merge conflict on "kernel.shmmax"
    And the agent should pause convergence for "kernel.shmmax"
    And non-conflicting config keys should sync normally

  Scenario: Merge conflict resolved by admin — accept local
    Given node "node-001" has a merge conflict on "kernel.shmmax"
    And the local value is "68719476736" and the journal value is "34359738368"
    When admin "ops@example.com" resolves the conflict by accepting local
    Then the journal should record "kernel.shmmax" as "68719476736" for node "node-001"
    And the agent should resume convergence

  Scenario: Merge conflict resolved by admin — accept journal
    Given node "node-001" has a merge conflict on "kernel.shmmax"
    And the local value is "68719476736" and the journal value is "34359738368"
    When admin "ops@example.com" resolves the conflict by accepting journal
    Then node "node-001" should apply "kernel.shmmax" as "34359738368"
    And the overwritten local value should be logged for audit
    And the agent should resume convergence

  Scenario: Grace period fallback to journal-wins when no admin resolves
    Given node "node-001" has a merge conflict on "kernel.shmmax"
    And the grace period is configured as the commit window duration
    When the grace period expires without admin resolution
    Then the system should fall back to journal-wins
    And "kernel.shmmax" on node "node-001" should be set to the journal value
    And the overwritten local value should be logged for audit
