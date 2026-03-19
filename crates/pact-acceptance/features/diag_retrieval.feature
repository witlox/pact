Feature: Diagnostic Log Retrieval
  pact diag retrieves structured diagnostic logs from nodes. Pull model:
  agent collects on demand, CLI displays. Supports per-node and fleet-wide
  (vCluster) queries with server-side grep filtering.

  NOTE: pact service logs <name> already exists for single-service log
  streaming. pact diag is the broader structured diagnostic tool covering
  system + all services + fleet-wide fan-out.

  Background:
    Given a journal with default state
    And a node "node-001" enrolled in vCluster "ml-training"
    And the agent on "node-001" is running in PactSupervisor mode

  # --- Per-node diagnostic retrieval ---

  Scenario: Retrieve last 100 lines of system logs (default)
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001"
    Then the agent should collect logs from all sources
    And the output should contain at most 100 lines per source
    And the output should include dmesg lines
    And the output should include syslog lines
    And the output should include supervised service log lines

  Scenario: Retrieve last N lines with --lines flag
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --lines 500"
    Then the agent should collect logs from all sources
    And the output should contain at most 500 lines per source

  Scenario: Retrieve system logs only with --source system
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --source system"
    Then the output should include dmesg lines
    And the output should include syslog lines
    And the output should not include supervised service log lines

  Scenario: Retrieve service logs only with --source service
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --source service"
    Then the output should include supervised service log lines
    And the output should not include dmesg lines
    And the output should not include syslog lines

  Scenario: Retrieve specific service logs with --service flag
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --service nvidia-persistenced"
    Then the output should include only "nvidia-persistenced" service log lines
    And the output should not include dmesg lines
    And the output should not include syslog lines

  Scenario: Filter with grep pattern
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --grep 'ECC error'"
    Then the agent should apply the grep filter server-side
    And the output should contain only lines matching "ECC error"

  Scenario: Grep with no matches returns empty result
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --grep 'nonexistent-pattern-xyz'"
    Then the output should be empty
    And exit code should be 0

  Scenario: Unauthorized viewer role is rejected
    When user "viewer@example.com" with role "pact-viewer-ml-training" runs "pact diag node-001"
    Then the command should be rejected with "authorization denied"
    And exit code should be 6

  # --- Fleet-wide diagnostic retrieval ---

  Scenario: Retrieve diag from all nodes in vCluster
    Given nodes "node-001", "node-002", "node-003" enrolled in vCluster "ml-training"
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag --vcluster ml-training"
    Then the CLI should fan out CollectDiag to all 3 agents concurrently
    And the output should contain results from all 3 nodes

  Scenario: Fleet-wide with grep pattern
    Given nodes "node-001", "node-002", "node-003" enrolled in vCluster "ml-training"
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag --vcluster ml-training --grep 'GPU'"
    Then each agent should apply the grep filter server-side
    And only matching lines should be transmitted

  Scenario: Fleet-wide output prefixed with node_id per line
    Given nodes "node-001", "node-002" enrolled in vCluster "ml-training"
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag --vcluster ml-training"
    Then each output line should be prefixed with "[node-001]" or "[node-002]"

  Scenario: Fleet-wide with source filter
    Given nodes "node-001", "node-002" enrolled in vCluster "ml-training"
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag --vcluster ml-training --source system"
    Then the output from each node should include only system logs
    And the output should not include supervised service log lines

  Scenario: Empty vCluster returns no nodes found
    Given vCluster "empty-cluster" has no enrolled nodes
    When user "admin@example.com" with role "pact-ops-empty-cluster" runs "pact diag --vcluster empty-cluster"
    Then the output should contain "no nodes found"
    And exit code should be 0

  # --- Log sources ---

  Scenario: PactSupervisor mode collects from dmesg, syslog, and service stdout/stderr
    Given the agent on "node-001" is running in PactSupervisor mode
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001"
    Then the agent should read /dev/kmsg for dmesg
    And the agent should read /var/log/syslog or /var/log/messages for syslog
    And the agent should read /run/pact/logs/{service}.log for each supervised service

  Scenario: Systemd compat mode collects via journalctl
    Given the agent on "node-001" is running in systemd compat mode
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001"
    Then the agent should run "dmesg" for system dmesg
    And the agent should run "journalctl --no-pager" for system logs
    And the agent should run "journalctl -u {service} --no-pager" for each supervised service

  Scenario: Custom log source path in vCluster policy
    Given the vCluster "ml-training" policy includes extra log path "/var/log/nvidia/gpu-diag.log"
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001"
    Then the agent should also collect from "/var/log/nvidia/gpu-diag.log"

  Scenario: Service not found returns descriptive error
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --service nonexistent-svc"
    Then the output should contain "service 'nonexistent-svc' not found in supervisor"
    And exit code should be 0

  # --- Edge cases ---

  Scenario: Node unreachable during fleet-wide query shows partial results with warning
    Given nodes "node-001", "node-002", "node-003" enrolled in vCluster "ml-training"
    And node "node-002" is unreachable
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag --vcluster ml-training"
    Then the output should contain results from "node-001" and "node-003"
    And the output should contain "[WARN] node-002: unreachable"

  Scenario: Empty log source returns empty result
    Given node "node-001" has no dmesg output (freshly booted, buffer empty)
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --source system"
    Then the dmesg source should return an empty chunk
    And exit code should be 0

  Scenario: Output truncated at line limit with truncation indicator
    Given node "node-001" has more than 100 lines of dmesg
    When user "admin@example.com" with role "pact-ops-ml-training" runs "pact diag node-001 --source system"
    Then the dmesg output should contain exactly 100 lines
    And the output should indicate truncation for the dmesg source
