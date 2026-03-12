Feature: Boot Sequence
  pact-agent is the init system on diskless compute nodes. It starts as
  PID 1, authenticates to journal, streams config, applies it, starts
  services in dependency order, and reports capabilities. (ADR-006)

  Target: <2s from agent start to node ready.

  Background:
    Given a journal with default state

  # --- Init sequence ---

  Scenario: Agent starts and authenticates to journal
    When pact-agent starts on node "node-001" for vCluster "ml-training"
    Then the agent should authenticate to the journal via mTLS
    And the authentication should use the pact-service-agent identity

  Scenario: Agent pulls vCluster overlay (Phase 1)
    Given a boot overlay for vCluster "ml-training" with sysctl and mount config
    When pact-agent starts on node "node-001" for vCluster "ml-training"
    Then the agent should stream the vCluster overlay
    And the overlay should be applied (sysctl, modules, mounts)

  Scenario: Agent pulls node delta (Phase 2)
    Given a boot overlay for vCluster "ml-training"
    And a committed node delta for node "node-001"
    When pact-agent starts on node "node-001"
    Then the agent should apply the node-specific delta after the overlay

  Scenario: Agent starts services in dependency order
    Given a boot overlay with services:
      | name                  | order | depends_on |
      | chronyd               | 1     |            |
      | nvidia-persistenced   | 2     |            |
      | lattice-node-agent    | 10    | chronyd    |
    When pact-agent starts on node "node-001"
    Then "chronyd" should start first
    And "nvidia-persistenced" should start second
    And "lattice-node-agent" should start last

  Scenario: Agent reports capabilities after services start
    When pact-agent completes boot on node "node-001"
    Then a CapabilityReport should be written to tmpfs
    And the node should be ready for workloads

  Scenario: Agent starts config subscription after boot
    When pact-agent completes boot on node "node-001"
    Then the agent should subscribe to config updates
    And the subscription should start from the current sequence

  # --- Boot with no pre-existing overlay ---

  Scenario: First boot for new vCluster triggers on-demand overlay
    Given no overlay exists for vCluster "new-cluster"
    When pact-agent starts on node "node-001" for vCluster "new-cluster"
    Then the journal should build an overlay on demand
    And the boot should proceed normally

  # --- Boot with cached config (partition) ---

  Scenario: Agent boots with cached config when journal unreachable
    Given the journal is unreachable
    And cached config exists for node "node-001" in vCluster "ml-training"
    When pact-agent starts on node "node-001"
    Then the agent should apply cached config
    And the agent should start services from cached declarations
    And the agent should retry journal connection in background

  # --- Committed changes survive reboot ---

  Scenario: Committed node deltas persist across reboots
    Given node "node-001" had a committed change to "vm.swappiness=10"
    When node "node-001" reboots and pact-agent starts
    Then the node delta should include "vm.swappiness=10"
    And the setting should be applied during boot

  # --- Resource budget ---

  Scenario: Agent stays within resource budget
    When pact-agent is running in steady state
    Then RSS should be less than 50 MB
    And CPU usage should be less than 0.5 percent

  Scenario: Agent CPU during drift evaluation stays bounded
    When drift is being evaluated on node "node-001"
    Then CPU usage should be less than 2 percent
