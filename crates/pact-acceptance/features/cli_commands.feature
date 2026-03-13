Feature: CLI Commands
  The pact CLI provides configuration management, admin operations,
  delegation, and group management commands with well-defined exit codes.

  Background:
    Given a journal with default state

  # --- Configuration management ---

  Scenario: pact status shows node and vCluster state
    Given node "node-001" in vCluster "ml-training" with state "Committed"
    And node "node-002" in vCluster "ml-training" with state "Drifted"
    When the user runs "pact status --vcluster ml-training"
    Then the output should show node "node-001" as "Committed"
    And the output should show node "node-002" as "Drifted"
    And the exit code should be 0

  Scenario: pact diff shows declared vs actual state
    Given node "node-001" has drift in kernel parameter "vm.swappiness"
    When the user runs "pact diff node-001"
    Then the output should show the kernel parameter difference
    And the exit code should be 0

  Scenario: pact diff --committed shows uncommitted deltas
    Given node "node-001" has committed deltas not yet promoted to overlay
    When the user runs "pact diff --committed node-001"
    Then the output should show the committed but unpromoted deltas

  Scenario: pact commit records change in journal
    Given drift is detected on node "node-001"
    When the user runs "pact commit -m 'approved sysctl change'"
    Then a Commit entry should be recorded in the journal
    And the exit code should be 0

  Scenario: pact rollback reverts to previous state
    Given a committed change at sequence 5
    When the user runs "pact rollback 5"
    Then a Rollback entry should be recorded in the journal
    And the exit code should be 0

  Scenario: pact log shows configuration history
    Given 5 config entries in the journal
    When the user runs "pact log -n 3"
    Then the output should show the 3 most recent entries
    And entries should be ordered newest first

  Scenario: pact apply applies declarative config spec
    Given a valid config spec file "spec.toml"
    When the user runs "pact apply spec.toml"
    Then the config should be written through Raft
    And a BootConfig entry should be recorded

  Scenario: pact watch subscribes to live event stream
    Given node "node-001" in vCluster "ml-training"
    When the user runs "pact watch --vcluster ml-training"
    And a config change occurs
    Then the event should be displayed in the output

  Scenario: pact extend extends commit window
    Given an active commit window for node "node-001"
    When the user runs "pact extend 30"
    Then the commit window should be extended by 30 minutes

  # --- Promote command ---

  Scenario: pact promote exports node deltas as overlay TOML
    Given node "node-001" has committed deltas with kernel and mount changes
    When the user runs "pact promote node-001"
    Then the output should be valid TOML
    And the TOML should contain a sysctl section for kernel changes
    And the TOML should contain a mounts section for mount changes

  Scenario: pact promote --dry-run previews without applying
    Given node "node-001" has committed deltas
    When the user runs "pact promote node-001 --dry-run"
    Then the output should preview the generated TOML
    And no changes should be applied to the journal

  # --- Admin operations ---

  Scenario: pact exec runs remote command
    When the user runs "pact exec node-001 -- nvidia-smi"
    Then stdout from the remote command should be displayed
    And the exit code should be 0

  Scenario: pact shell opens interactive session
    When the user runs "pact shell node-001"
    Then an interactive shell session should be opened
    And the session should be authenticated

  Scenario: pact service shows service status
    When the user runs "pact service status chronyd"
    Then the output should show the service state
    And the exit code should be 0

  Scenario: pact cap shows capability report
    When the user runs "pact cap node-001"
    Then the output should show GPU, memory, and supervisor information

  # --- Exit codes ---

  Scenario: Exit code 2 for authentication failure
    Given an invalid OIDC token
    When the user runs any command
    Then the exit code should be 2

  Scenario: Exit code 3 for policy rejection
    Given a policy that denies the requested operation
    When the user runs "pact commit -m 'denied'"
    Then the exit code should be 3

  Scenario: Exit code 4 for concurrent modification conflict
    Given another admin is modifying the same node
    When the user runs "pact commit -m 'conflict'"
    Then the exit code should be 4

  Scenario: Exit code 5 for journal unreachable
    Given the journal is unreachable
    When the user runs "pact status"
    Then the exit code should be 5

  Scenario: Exit code 6 for command not whitelisted
    When the user runs "pact exec node-001 -- forbidden-cmd"
    Then the exit code should be 6

  Scenario: Exit code 10 for rollback with active consumers
    Given a mount with active consumers
    When the user runs "pact rollback 5"
    Then the exit code should be 10

  # --- Delegation commands ---

  Scenario: pact drain delegates to lattice
    When the user runs "pact drain node-001"
    Then the command should delegate to the lattice scheduler API

  Scenario: pact cordon delegates to lattice
    When the user runs "pact cordon node-001"
    Then the command should delegate to the lattice scheduler API

  Scenario: pact reboot delegates to OpenCHAMI
    When the user runs "pact reboot node-001"
    Then the command should delegate to the OpenCHAMI Manta API

  # --- Group management ---

  Scenario: pact group list shows all groups
    Given groups "ml-training" and "storage-ops" exist
    When the user runs "pact group list"
    Then the output should list both groups

  Scenario: pact group show displays group details
    When the user runs "pact group show ml-training"
    Then the output should show the group policy and member nodes

  # --- Admin notification on overwrite (CR5) ---

  Scenario: Active CLI session notified when local changes are overwritten
    Given admin "ops@example.com" has an active CLI session on node "node-001"
    And admin "ops@example.com" has uncommitted local changes on "kernel.shmmax"
    When another admin promotes a change that overwrites "kernel.shmmax"
    Then admin "ops@example.com" should receive a notification in their session
    And the notification should show which keys were overwritten and by whom

  Scenario: Active CLI session notified on grace period overwrite
    Given admin "ops@example.com" has an active CLI session on node "node-001"
    And node "node-001" has a merge conflict on "kernel.shmmax"
    When the grace period expires and journal-wins
    Then admin "ops@example.com" should receive a notification in their session
    And the notification should explain "grace period expired, journal value applied"

  # --- No cross-vCluster atomicity (CR6) ---

  Scenario: Cross-vCluster operations are independent commits
    Given vClusters "ml-training" and "storage-ops" both exist
    When admin commits a sysctl change to vCluster "ml-training"
    And admin commits a sysctl change to vCluster "storage-ops"
    Then each commit should be an independent journal entry
    And if one fails the other should still succeed

  Scenario: Partial failure across vClusters is reported
    Given vClusters "ml-training" and "storage-ops" both exist
    When admin commits a sysctl change to vCluster "ml-training" which succeeds
    And admin commits a sysctl change to vCluster "storage-ops" which fails
    Then the CLI should report success for "ml-training"
    And the CLI should report failure for "storage-ops"
    And no automatic rollback of the "ml-training" commit should occur
