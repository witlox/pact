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

  # --- Promote conflict acknowledgment (CR4) ---

  Scenario: Promote detects conflicts with other nodes' local changes
    Given node "node-001" has committed delta changing "vm.swappiness" to "10"
    And node "node-002" has a local change on "vm.swappiness" set to "30"
    When the admin runs "pact promote node-001"
    Then the promote should pause with a conflict report
    And the conflict should show node "node-002" has local value "30" vs promoted value "10"
    And the admin must acknowledge the conflict before proceeding

  Scenario: Promote proceeds after admin accepts overwrite
    Given a promote conflict on "vm.swappiness" between promoted value "10" and node-002 local value "30"
    When the admin accepts overwrite for node "node-002"
    Then the overlay should include "vm.swappiness=10"
    And node "node-002" local value should be superseded
    And the overwritten local value should be logged for audit

  Scenario: Promote proceeds after admin keeps local
    Given a promote conflict on "vm.swappiness" between promoted value "10" and node-002 local value "30"
    When the admin keeps the local value for node "node-002"
    Then the overlay should include "vm.swappiness=10"
    And node "node-002" should retain a per-node delta of "vm.swappiness=30"

  # --- vCluster homogeneity warning (ND3) ---

  Scenario: Heterogeneous nodes within vCluster trigger warning
    Given vCluster "ml-training" has 4 nodes
    And node "node-001" has a per-node delta on "vm.swappiness"
    And nodes "node-002", "node-003", "node-004" are converged to the overlay
    When the user runs "pact status --vcluster ml-training"
    Then the output should warn that node "node-001" diverges from vCluster homogeneity
    And the warning should recommend promoting or reverting the delta

  Scenario: Expired per-node delta triggers warning
    Given node "node-001" has a per-node delta with TTL that has expired
    When the user runs "pact status --vcluster ml-training"
    Then the output should warn that node "node-001" has an expired delta
    And the warning should recommend cleanup
