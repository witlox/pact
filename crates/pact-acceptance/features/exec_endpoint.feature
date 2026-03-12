Feature: Exec Endpoint
  pact exec runs a single whitelisted command on a remote node.
  Unlike shell sessions, pact controls the full lifecycle: no shell
  interpretation, direct fork/exec, command classification upfront.

  Background:
    Given a journal with default state
    And a shell server with default whitelist

  # --- Basic execution ---

  Scenario: Execute a whitelisted read-only command
    When user "admin@example.com" with role "pact-ops-ml-training" executes "nvidia-smi" on node "node-001"
    Then the command should execute successfully
    And stdout should be streamed back
    And an ExecLog entry should be recorded in the journal

  Scenario: Execute a command with arguments
    When user "admin@example.com" executes "dmesg" with args "--since '5 minutes ago'" on node "node-001"
    Then the command should execute successfully
    And stdout should be streamed back

  # --- Whitelist enforcement ---

  Scenario: Non-whitelisted command is rejected
    When user "admin@example.com" executes "rm" on node "node-001"
    Then the command should be rejected with "command not whitelisted"
    And exit code should be 6

  Scenario: Platform admin can bypass whitelist
    When user "admin@example.com" with role "pact-platform-admin" executes "custom-diag" on node "node-001"
    Then the command should execute successfully
    And the bypass should be logged in the audit trail

  # --- Command classification ---

  Scenario: Read-only command executes immediately
    When user "admin@example.com" executes "cat /etc/resolv.conf" on node "node-001"
    Then the command should execute immediately
    And no commit window should be opened

  Scenario: State-changing command triggers commit window
    When user "admin@example.com" executes "sysctl -w vm.swappiness=10" on node "node-001"
    Then the command should execute
    And a commit window should be opened for the change
    And the state change should be classified as "state-changing"

  # --- Authorization ---

  Scenario: Viewer role can execute read-only commands
    When user "viewer@example.com" with role "pact-viewer-ml-training" executes "dmesg" on node "node-001"
    Then the command should execute successfully

  Scenario: Viewer role cannot execute state-changing commands
    When user "viewer@example.com" with role "pact-viewer-ml-training" executes "sysctl -w vm.swappiness=10" on node "node-001"
    Then the command should be rejected with "authorization denied"

  # --- Output streaming ---

  Scenario: Stdout is streamed in real time
    When user "admin@example.com" executes a long-running command on node "node-001"
    Then output should be streamed as it is produced
    And the stream should end with an exit code

  Scenario: Stderr is streamed separately
    When user "admin@example.com" executes a command that writes to stderr on node "node-001"
    Then stderr should be streamed back separately from stdout

  # --- Audit logging ---

  Scenario: Successful exec is logged with full detail
    When user "admin@example.com" executes "uname -a" on node "node-001"
    Then the ExecLog entry should contain the command "uname -a"
    And the entry should contain the actor identity
    And the entry should have scope node "node-001"

  Scenario: Failed exec is also logged
    When user "admin@example.com" executes a command that fails on node "node-001"
    Then the ExecLog entry should still be recorded
    And the entry should include the exit code

  # --- Degraded mode (PolicyService unreachable) ---

  Scenario: Exec falls back to cached policy when PolicyService is unavailable
    Given the PolicyService is unreachable
    And the cached policy allows "dmesg" for role "pact-ops-ml-training"
    When user "admin@example.com" with role "pact-ops-ml-training" executes "dmesg" on node "node-001"
    Then the command should execute successfully
    And the authorization should be logged as "degraded"

  Scenario: Two-person approval denied in degraded mode
    Given the PolicyService is unreachable
    And the vCluster requires two-person approval
    When user "admin@example.com" executes a state-changing command on node "node-001"
    Then the command should be rejected with "two-person approval unavailable in degraded mode"
