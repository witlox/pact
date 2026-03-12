Feature: Overlay Management
  Boot overlays are pre-computed, compressed (zstd) config bundles for
  vClusters. They use a hybrid strategy: rebuild on config commit +
  build on demand if cache miss.

  Background:
    Given a journal with default state

  # --- Overlay creation ---

  Scenario: Overlay created from vCluster config
    Given vCluster "ml-training" has config with sysctl, mounts, and services
    When an overlay is built for vCluster "ml-training"
    Then the overlay should contain the complete vCluster config
    And the overlay should have a version number
    And the overlay should have a checksum

  Scenario: Overlay data is compressed
    Given vCluster "ml-training" has a large config
    When an overlay is built for vCluster "ml-training"
    Then the overlay data should be compressed
    And the compressed size should be smaller than the raw config

  # --- Hybrid rebuild strategy ---

  Scenario: Overlay rebuilt on config commit
    Given an existing overlay for vCluster "ml-training" at version 1
    When a config commit affects vCluster "ml-training"
    Then the overlay should be rebuilt
    And the new overlay version should be 2

  Scenario: Overlay rebuilt on policy update
    Given an existing overlay for vCluster "ml-training" at version 1
    When the policy for vCluster "ml-training" is updated
    Then the overlay should be rebuilt

  Scenario: Overlay not rebuilt for unrelated vCluster changes
    Given an existing overlay for vCluster "ml-training" at version 1
    When a config commit affects vCluster "storage-ops"
    Then the overlay for vCluster "ml-training" should remain at version 1

  Scenario: Overlay built on demand when missing
    Given no overlay exists for vCluster "new-cluster"
    When a boot request arrives for vCluster "new-cluster"
    Then an overlay should be built on demand
    And the newly built overlay should be cached

  # --- Staleness detection ---

  Scenario: Stale overlay detected by sequence comparison
    Given an overlay for vCluster "ml-training" built at sequence 5
    And the latest config sequence for vCluster "ml-training" is 8
    When staleness is checked for vCluster "ml-training"
    Then the overlay should be detected as stale
    And a rebuild should be triggered

  Scenario: Fresh overlay passes staleness check
    Given an overlay for vCluster "ml-training" built at sequence 8
    And the latest config sequence for vCluster "ml-training" is 8
    When staleness is checked for vCluster "ml-training"
    Then the overlay should be detected as fresh

  # --- Node deltas in overlay context ---

  Scenario: Node delta supplements overlay at boot
    Given an overlay for vCluster "ml-training" with base sysctl config
    And node "node-001" has a committed delta changing "vm.swappiness" to "10"
    When node "node-001" boots
    Then the base overlay should be applied first
    And the node delta should be applied on top
    And "vm.swappiness" should end up as "10"

  Scenario: Promoted delta merges into overlay
    Given node "node-001" has committed delta changing "vm.swappiness" to "10"
    When the delta is promoted via "pact promote node-001"
    And the result is applied via "pact apply"
    Then the overlay for "ml-training" should include "vm.swappiness=10"
    And the node delta should no longer be needed for this setting
