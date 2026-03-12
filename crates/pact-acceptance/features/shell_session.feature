Feature: Shell Session
  pact shell replaces SSH with an authenticated, audited, restricted bash
  session. It uses rbash with PATH restriction, not command parsing.
  All sessions are logged to the journal. (ADR-007)

  Background:
    Given a journal with default state
    And a shell server with default whitelist

  # --- Authentication and authorization ---

  Scenario: Authenticated user can open shell session
    When user "admin@example.com" with role "pact-ops-ml-training" requests a shell on node "node-001"
    Then a shell session should be opened
    And a ShellSession entry should be recorded in the journal

  Scenario: Unauthenticated request is denied
    When an unauthenticated user requests a shell on node "node-001"
    Then the request should be denied with error "authorization denied"

  Scenario: Viewer role cannot open shell session
    When user "viewer@example.com" with role "pact-viewer-ml-training" requests a shell on node "node-001"
    Then the request should be denied with error "authorization denied"

  Scenario: Shell requires higher privilege than exec
    When user "limited@example.com" with exec-only permissions requests a shell on node "node-001"
    Then the request should be denied with error "authorization denied"

  # --- Restricted bash environment ---

  Scenario: Shell uses restricted bash
    When user "admin@example.com" opens a shell session
    Then the shell should be running rbash
    And PATH should be restricted to the session bin directory

  Scenario: Whitelisted commands are available
    When user "admin@example.com" opens a shell session
    Then the command "nvidia-smi" should be available
    And the command "dmesg" should be available
    And the command "lspci" should be available
    And the command "ip" should be available
    And the command "cat" should be available
    And the command "ps" should be available

  Scenario: Non-whitelisted commands are unavailable
    When user "admin@example.com" opens a shell session
    And the user tries to run "rm -rf /"
    Then the command should fail with "command not found"

  Scenario: Absolute path execution is blocked by rbash
    When user "admin@example.com" opens a shell session
    And the user tries to run "/usr/bin/dangerous-command"
    Then the command should be blocked by rbash restrictions

  Scenario: PATH modification is blocked by rbash
    When user "admin@example.com" opens a shell session
    And the user tries to modify PATH
    Then the modification should be blocked by rbash restrictions

  # --- Whitelist security audit ---

  Scenario: vi is excluded from default whitelist (shell escape vector)
    When user "admin@example.com" opens a shell session
    Then the command "vi" should not be available in the default whitelist

  Scenario: python is excluded from default whitelist (arbitrary execution)
    When user "admin@example.com" opens a shell session
    Then the command "python" should not be available in the default whitelist

  Scenario: less runs in secure mode if whitelisted
    When user "admin@example.com" opens a shell session
    And "less" is in the whitelist
    Then the LESSSECURE environment variable should be set to "1"

  # --- Command logging via PROMPT_COMMAND ---

  Scenario: Each command is logged to audit pipeline
    When user "admin@example.com" executes "dmesg" in a shell session
    Then the command "dmesg" should be logged to the audit pipeline
    And the log should include the authenticated identity

  Scenario: Multiple commands in session are all logged
    When user "admin@example.com" executes "dmesg" in a shell session
    And user "admin@example.com" executes "nvidia-smi" in the same session
    Then both commands should be logged to the audit pipeline

  # --- Session lifecycle ---

  Scenario: Session cleanup on disconnect
    When user "admin@example.com" opens a shell session
    And the user disconnects
    Then the session should be cleaned up
    And a ShellSessionEnd entry should be recorded in the journal

  Scenario: Session has unique identifier
    When user "admin@example.com" opens a shell session
    Then the session should have a unique session ID
    And the session ID should be returned to the client

  # --- Drift detection during shell session ---

  Scenario: State changes in shell trigger drift detection
    When user "admin@example.com" executes a state-changing command in a shell session
    Then the drift observer should detect the change
    And a commit window should be opened

  # --- Learning mode ---

  Scenario: Learning mode captures command-not-found events
    Given whitelist mode is "learning"
    When user "admin@example.com" tries to run "custom-tool" in a shell session
    Then the command should fail with "command not found"
    And a whitelist suggestion should be generated for "custom-tool"

  # --- vCluster-scoped whitelists ---

  Scenario: Different vClusters can have different whitelists
    Given vCluster "ml-training" has whitelist including "nvidia-smi"
    And vCluster "storage-ops" has whitelist including "zpool"
    When user opens shell on a node in vCluster "ml-training"
    Then "nvidia-smi" should be available
    And "zpool" should not be available
