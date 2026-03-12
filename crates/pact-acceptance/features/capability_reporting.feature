Feature: Capability Reporting
  pact-agent detects hardware capabilities (GPUs, memory, network, storage)
  and reports them via a tmpfs manifest and unix socket for lattice-node-agent.
  GPU detection is vendor-neutral (NVIDIA + AMD) behind a GpuBackend trait.

  Background:
    Given a journal with default state

  # --- GPU detection ---

  Scenario: Detect NVIDIA GPUs
    Given a node with 4 NVIDIA A100 GPUs
    When capability detection runs
    Then the capability report should contain 4 GPUs
    And all GPUs should have vendor "Nvidia"
    And all GPUs should have model "A100"

  Scenario: Detect AMD GPUs
    Given a node with 8 AMD MI300X GPUs
    When capability detection runs
    Then the capability report should contain 8 GPUs
    And all GPUs should have vendor "Amd"
    And all GPUs should have model "MI300X"

  Scenario: Mixed GPU vendors on single node
    Given a node with 2 NVIDIA A100 GPUs and 2 AMD MI250X GPUs
    When capability detection runs
    Then the capability report should contain 4 GPUs
    And 2 GPUs should have vendor "Nvidia"
    And 2 GPUs should have vendor "Amd"

  Scenario: Node without GPUs reports empty GPU list
    Given a node with no GPUs
    When capability detection runs
    Then the capability report should contain 0 GPUs

  # --- GPU health monitoring ---

  Scenario: Healthy GPU reported as healthy
    Given a node with 1 NVIDIA GPU in healthy state
    When capability detection runs
    Then the GPU health should be "Healthy"

  Scenario: Degraded GPU reported and triggers capability change
    Given a node with 1 NVIDIA GPU that reports degraded health
    When capability detection runs
    Then the GPU health should be "Degraded"
    And a CapabilityChange entry should be recorded in the journal

  Scenario: Failed GPU reported and triggers immediate update
    Given a node with 1 NVIDIA GPU that fails
    When capability detection runs
    Then the GPU health should be "Failed"
    And the capability report should be updated immediately

  # --- Memory and system info ---

  Scenario: Memory capacity is reported
    Given a node with 512 GB of memory
    When capability detection runs
    Then the capability report should show 549755813888 memory bytes

  # --- Capability report delivery ---

  Scenario: Report written to tmpfs manifest
    When capability detection runs
    Then the report should be written to the configured manifest path
    And the manifest should be valid JSON

  Scenario: Report available via unix socket
    When capability detection runs
    Then the report should be available via the configured unix socket
    And lattice-node-agent should be able to read it

  # --- Report timing ---

  Scenario: Report sent on change
    Given a stable capability report
    When GPU 0 transitions from healthy to degraded
    Then a new capability report should be sent immediately

  Scenario: Report sent periodically for confirmation
    Given a stable capability report
    When the configured poll interval elapses
    Then a new capability report should be sent

  # --- Config state in report ---

  Scenario: Report includes current config state
    Given node "node-001" is in state "Committed"
    When capability detection runs for node "node-001"
    Then the capability report config state should be "Committed"

  Scenario: Report includes supervisor status
    Given 3 declared services with 3 running and 0 failed
    When capability detection runs
    Then the supervisor status should show backend "Pact"
    And the supervisor status should show 3 declared, 3 running, 0 failed

  Scenario: Report reflects emergency state
    Given node "node-001" is in emergency mode
    When capability detection runs for node "node-001"
    Then the capability report config state should be "Emergency"
