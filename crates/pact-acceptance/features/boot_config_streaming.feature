Feature: Boot Config Streaming
  When a node boots, pact-agent streams its configuration from the journal
  in two phases: a vCluster base overlay followed by a node-specific delta.
  This must complete in under 2 seconds for 10,000+ nodes.

  Background:
    Given a journal with default state

  # --- Phase 1: vCluster overlay ---

  Scenario: Stream base overlay for a vCluster
    Given a boot overlay for vCluster "ml-training" version 1 with data "sysctl config"
    When node "node-001" requests boot config for vCluster "ml-training"
    Then the boot stream should contain a base overlay chunk
    And the overlay data should match the stored overlay

  Scenario: Overlay includes version and checksum
    Given a boot overlay for vCluster "ml-training" version 3 with data "full config bundle"
    When node "node-001" requests boot config for vCluster "ml-training"
    Then the overlay version should be 3
    And the overlay should have a valid checksum

  # --- Phase 2: node-specific delta ---

  Scenario: Stream node delta after overlay
    Given a boot overlay for vCluster "ml-training" version 1 with data "base config"
    And a committed node delta for node "node-001" with kernel change "vm.swappiness" to "10"
    When node "node-001" requests boot config for vCluster "ml-training"
    Then the boot stream should contain a base overlay chunk
    And the boot stream should contain a node delta
    And the node delta should include the kernel change

  Scenario: Node without committed deltas gets overlay only
    Given a boot overlay for vCluster "ml-training" version 1 with data "base config"
    When node "node-002" requests boot config for vCluster "ml-training"
    Then the boot stream should contain a base overlay chunk
    And the boot stream should not contain a node delta

  # --- Stream completion ---

  Scenario: Boot stream ends with ConfigComplete message
    Given a boot overlay for vCluster "ml-training" version 1 with data "config"
    When node "node-001" requests boot config for vCluster "ml-training"
    Then the boot stream should end with a ConfigComplete message
    And the ConfigComplete should include the base version

  # --- Overlay lifecycle ---

  Scenario: Overlay rebuilt when config committed
    Given a boot overlay for vCluster "ml-training" version 1 with data "old config"
    When a config commit affects vCluster "ml-training"
    Then the overlay for vCluster "ml-training" should be rebuilt
    And the new overlay version should be greater than 1

  Scenario: Overlay built on demand when not cached
    Given no overlay exists for vCluster "new-cluster"
    When node "node-001" requests boot config for vCluster "new-cluster"
    Then an overlay should be built on demand for vCluster "new-cluster"

  # --- Config subscription (live updates after boot) ---

  Scenario: Agent receives config update after boot
    Given node "node-001" is subscribed to config updates for vCluster "ml-training" from sequence 0
    When a config commit is appended for vCluster "ml-training"
    Then node "node-001" should receive a config update notification
    And the update should include the new sequence number

  Scenario: Agent reconnects with last known sequence
    Given node "node-001" is subscribed to config updates for vCluster "ml-training" from sequence 5
    When config commits were appended at sequences 5, 6, and 7
    Then node "node-001" should receive updates starting from sequence 5

  Scenario: Policy change delivered to subscribed agents
    Given node "node-001" is subscribed to config updates for vCluster "ml-training" from sequence 0
    When the policy for vCluster "ml-training" is updated
    Then node "node-001" should receive a policy change notification

  Scenario: Blacklist change delivered to subscribed agents
    Given node "node-001" is subscribed to config updates for vCluster "ml-training" from sequence 0
    When the blacklist for vCluster "ml-training" is updated
    Then node "node-001" should receive a blacklist change notification
